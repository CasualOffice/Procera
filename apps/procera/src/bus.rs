//! The in-process event bus (BPMS-029 consumers): turns outbox events into notifications
//! (BPMS-048) and enrols new joiners in dynamic campaigns (BPMS-046). Each consumer records the message in the inbox in the same transaction as its effect,
//! so redelivery is harmless.

use procera_campaigns::Campaigns;
use procera_db::dispatch::{Bus, Message};
use procera_db::outbox::mark_consumed;
use procera_db::{Db, WsCtx};
use procera_members::Members;
use procera_notify::Notify;
use serde_json::json;
use uuid::Uuid;

const CONSUMER: &str = "notify";
const CAMPAIGNS: &str = "campaigns";

#[derive(Clone)]
pub struct AppBus {
    pub db: Db,
    pub members: Members,
    pub notify: Notify,
    pub campaigns: Campaigns,
    /// Public base URL for links in emails, without a trailing slash.
    pub public_url: String,
}

impl std::fmt::Debug for AppBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBus").field("public_url", &self.public_url).finish_non_exhaustive()
    }
}

fn uuid_field(m: &Message, name: &str) -> Result<Uuid, String> {
    m.payload[name]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("{}: payload lacks {name}", m.topic))
}

impl AppBus {
    async fn handle(&self, m: &Message) -> Result<(), String> {
        let ctx = WsCtx { workspace_id: m.workspace_id };
        match m.topic.as_str() {
            "invitation.created.v1" => {
                let id = uuid_field(m, "invitation_id")?;
                // Read outside the consumer transaction (decryption happens in the members service).
                let Some((email, token, expires, inviter, workspace)) =
                    self.members.invitation_for_mail(ctx, id).await.map_err(|e| e.to_string())?
                else {
                    return Ok(()); // accepted or revoked before we got to it
                };
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())? {
                    let link = format!("{}/invite?token={token}", self.public_url);
                    let expires =
                        expires.format(&time::format_description::well_known::Rfc2822).unwrap_or_default();
                    let vars =
                        json!({"inviter": inviter, "workspace": workspace, "link": link, "expires": expires});
                    self.notify
                        .queue_email(&mut tx, "invitation", &email, &vars, &m.id.to_string())
                        .await
                        .map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            "membership.activated.v1" => {
                let membership = uuid_field(m, "membership_id")?;
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())?
                    && let Some((inviter, name, workspace)) =
                        self.members.joined_context(&mut tx, membership).await.map_err(|e| e.to_string())?
                    && inviter != membership
                {
                    let vars = json!({"name": name, "workspace": workspace});
                    self.notify
                        .notify_member(
                            &mut tx,
                            inviter,
                            "member_joined",
                            &vars,
                            Some("/members"),
                            Some(("membership", membership)),
                            &m.id.to_string(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())?;
                // New joiners receive the dynamic campaigns whose audience they match (FR-AK03).
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CAMPAIGNS, m.id).await.map_err(|e| e.to_string())? {
                    self.campaigns.enroll_joiner(&mut tx, membership).await.map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            _ => Ok(()), // no consumer for this topic yet
        }
    }
}

impl Bus for AppBus {
    async fn deliver(&self, msg: &Message) -> Result<(), String> {
        self.handle(msg).await
    }
}
