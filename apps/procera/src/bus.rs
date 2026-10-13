//! The in-process event bus (BPMS-029 consumers): turns outbox events into notifications
//! (BPMS-048), enrols new joiners in dynamic campaigns (BPMS-046) and delivers published
//! announcements (BPMS-047) and starts their acknowledgement campaigns when they were scheduled,
//! starts re-acknowledgement campaigns for scheduled versions that go
//! live (BPMS-045) and drops cached effective settings (CFG-004). Each consumer records the message in the inbox in the same
//! transaction as its effect, so redelivery is harmless.

use procera_announcements::Announcements;
use procera_campaigns::Campaigns;
use procera_db::dispatch::{Bus, Message};
use procera_db::outbox::mark_consumed;
use procera_db::{Db, WsCtx};
use procera_library::Library;
use procera_members::Members;
use procera_notify::Notify;
use procera_notify::delivery::Urgency;
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
    pub announcements: Announcements,
    pub library: Library,
    pub settings: procera_settings::Settings,
    /// Public base URL for links in emails, without a trailing slash.
    pub public_url: String,
    /// Effective-settings cache to invalidate on `settings.changed.v1` (CFG-004), when shared.
    pub settings_cache: Option<procera_settings::cache::SettingsCache>,
    /// Outgoing webhooks (BPMS-143): every event is offered to the workspace's endpoints.
    pub webhooks: Option<procera_webhooks::Webhooks>,
}

const WEBHOOKS: &str = "webhooks";

impl AppBus {
    /// Queues the event for the workspace's webhook endpoints, once (inbox key `webhooks`).
    async fn queue_webhooks(&self, m: &Message) -> Result<(), String> {
        let Some(w) = &self.webhooks else { return Ok(()) };
        if procera_webhooks::public_type(&m.topic).is_none() {
            return Ok(());
        }
        let mut tx =
            self.db.begin_ws(WsCtx { workspace_id: m.workspace_id }).await.map_err(|e| e.to_string())?;
        if mark_consumed(&mut tx, WEBHOOKS, m.id).await.map_err(|e| e.to_string())? {
            let at: time::OffsetDateTime = sqlx::query_scalar("SELECT created_at FROM outbox WHERE id = $1")
                .bind(m.id)
                .fetch_optional(tx.conn())
                .await
                .map_err(|e| e.to_string())?
                .unwrap_or_else(time::OffsetDateTime::now_utc);
            w.enqueue(&mut tx, m.id, &m.topic, m.aggregate_id, &m.payload, at)
                .await
                .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    }
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
            // Leavers' open acknowledgements close as "left organisation" (BPMS-046).
            "membership.deactivated.v1" => {
                let membership = uuid_field(m, "membership_id")?;
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CAMPAIGNS, m.id).await.map_err(|e| e.to_string())? {
                    self.campaigns.close_for_leaver(&mut tx, membership).await.map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            "announcement.published.v1" => {
                let id = uuid_field(m, "announcement_id")?;
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())?
                    && let Some(d) =
                        self.announcements.delivery(&mut tx, id).await.map_err(|e| e.to_string())?
                {
                    let link = format!("/announcements/{id}");
                    let vars = json!({
                        "title": d.title, "author": d.author, "workspace": d.workspace, "critical": d.critical,
                        "require_ack": d.require_ack, "link": format!("{}{link}", self.public_url),
                    });
                    let in_app = d.channels.iter().any(|c| c == "in_app");
                    let email = d.channels.iter().any(|c| c == "email");
                    // Dedupe keys are per announcement and recipient, so a redelivery adds nothing.
                    for (membership, address) in &d.recipients {
                        if in_app {
                            self.notify
                                .notify_member(
                                    &mut tx,
                                    *membership,
                                    "announcement",
                                    &vars,
                                    Some(&link),
                                    Some(("announcement", id)),
                                    &format!("announcement:{id}:{membership}"),
                                )
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        if email {
                            // Critical announcements may break quiet hours; ones that need an acknowledgement
                            // are never digested; the rest follow the member's digest.
                            let urgency = if d.critical {
                                Urgency::Critical
                            } else if d.require_ack {
                                Urgency::Urgent
                            } else {
                                Urgency::Normal
                            };
                            self.notify
                                .email_member(
                                    &mut tx,
                                    *membership,
                                    "announcement",
                                    address,
                                    &vars,
                                    &format!("announcement:{id}:{membership}:email"),
                                    urgency,
                                )
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
                tx.commit().await.map_err(|e| e.to_string())?;
                // FR-AN04: an announcement that asks for acknowledgement runs on the campaign engine.
                // Immediate publishes started it already (found and skipped); scheduled ones start
                // it here, as the system.
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CAMPAIGNS, m.id).await.map_err(|e| e.to_string())? {
                    crate::announcement_ack::start(
                        &self.announcements,
                        &self.campaigns,
                        &self.settings,
                        &mut tx,
                        id,
                        None,
                    )
                    .await?;
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // BPMS-044: the owner of an artifact whose review date passed is asked to recertify it.
            "artifact.review_overdue.v1" => {
                let id = uuid_field(m, "artifact_id")?;
                let owner = m
                    .payload
                    .get("owner_membership_id")
                    .and_then(|v| v.as_str())
                    .and_then(|v| v.parse::<Uuid>().ok());
                let title =
                    m.payload.get("title").and_then(|v| v.as_str()).unwrap_or("A document").to_owned();
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())?
                    && let Some(owner) = owner
                {
                    let link = format!("/library/{id}");
                    self.notify
                        .notify_member(
                            &mut tx,
                            owner,
                            "review_overdue",
                            &json!({"title": title}),
                            Some(&link),
                            Some(("artifact", id)),
                            &format!("review_overdue:{}", m.id),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // BPMS-045: a version that requires it asks its audience to acknowledge again. Immediate
            // publishes started the campaign already (it is found and skipped); scheduled ones start
            // it here, as the system.
            "artifact.published.v1" => {
                if m.payload.get("reack_required").and_then(|v| v.as_bool()) != Some(true) {
                    return Ok(());
                }
                let version = uuid_field(m, "version_id")?;
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CAMPAIGNS, m.id).await.map_err(|e| e.to_string())? {
                    crate::reack::start(
                        &self.library,
                        &self.campaigns,
                        &self.settings,
                        &mut tx,
                        version,
                        None,
                    )
                    .await?;
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // BPMS-040: a draft waits for review: tell everyone who may review it, except its author.
            "version.submitted.v1" => {
                let (artifact, author, kind) = (
                    uuid_field(m, "artifact_id")?,
                    uuid_field(m, "author_membership_id")?,
                    m.payload["kind"].as_str().unwrap_or("policy").to_owned(),
                );
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())? {
                    let name: Option<String> = sqlx::query_scalar(
                        "SELECT u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
                    )
                    .bind(author)
                    .fetch_optional(tx.conn())
                    .await
                    .map_err(|e| e.to_string())?;
                    let reviewers = self
                        .members
                        .access()
                        .members_holding(&mut tx, &[&format!("{kind}:review"), &format!("{kind}:approve")])
                        .await
                        .map_err(|e| e.to_string())?;
                    let link = format!("/library/{artifact}");
                    let mut vars = json!({"title": m.payload["title"], "version": m.payload["version_no"], "author": name.unwrap_or_else(|| "Someone".into()), "kind": kind, "link": link});
                    for (membership, email) in reviewers.iter().filter(|(id, _)| *id != author) {
                        let dedupe = format!("review_requested:{}:{membership}", m.id);
                        vars["link"] = json!(link);
                        self.notify
                            .notify_member(
                                &mut tx,
                                *membership,
                                "review_requested",
                                &vars,
                                Some(&link),
                                Some(("artifact", artifact)),
                                &dedupe,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                        vars["link"] = json!(format!("{}{link}", self.public_url));
                        // A review is asked for: urgent, never held for a digest.
                        self.notify
                            .email_member(
                                &mut tx,
                                *membership,
                                "review_requested",
                                email,
                                &vars,
                                &dedupe,
                                Urgency::Urgent,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // BPMS-040: the author hears the outcome of the review.
            "version.reviewed.v1" => {
                let (artifact, author) =
                    (uuid_field(m, "artifact_id")?, uuid_field(m, "author_membership_id")?);
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())? {
                    let email: Option<String> = sqlx::query_scalar(
                        "SELECT u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1 AND m.status = 'active'",
                    )
                    .bind(author)
                    .fetch_optional(tx.conn())
                    .await
                    .map_err(|e| e.to_string())?;
                    let link = format!("/library/{artifact}");
                    let dedupe = format!("review_decided:{}", m.id);
                    let mut vars = json!({"title": m.payload["title"], "version": m.payload["version_no"], "kind": m.payload["kind"].as_str().unwrap_or("policy"), "outcome": m.payload["outcome"], "comment": m.payload["comment"], "link": link});
                    self.notify
                        .notify_member(
                            &mut tx,
                            author,
                            "review_decided",
                            &vars,
                            Some(&link),
                            Some(("artifact", artifact)),
                            &dedupe,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    if let Some(email) = email {
                        vars["link"] = json!(format!("{}{link}", self.public_url));
                        self.notify
                            .email_member(
                                &mut tx,
                                author,
                                "review_decided",
                                &email,
                                &vars,
                                &dedupe,
                                Urgency::Normal,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // BPMS-047: an announcement waits for approval: tell those who may publish, except its author.
            "announcement.submitted.v1" => {
                let (id, author) =
                    (uuid_field(m, "announcement_id")?, uuid_field(m, "author_membership_id")?);
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())? {
                    let name: Option<String> = sqlx::query_scalar(
                        "SELECT u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
                    )
                    .bind(author)
                    .fetch_optional(tx.conn())
                    .await
                    .map_err(|e| e.to_string())?;
                    let approvers = self
                        .members
                        .access()
                        .members_holding(&mut tx, &["announcement:publish", "announcement:manage"])
                        .await
                        .map_err(|e| e.to_string())?;
                    let link = format!("/admin/announcements?id={id}");
                    let mut vars = json!({"title": m.payload["title"], "author": name.unwrap_or_else(|| "Someone".into()), "link": link});
                    for (membership, email) in approvers.iter().filter(|(x, _)| *x != author) {
                        let dedupe = format!("announcement_approval:{}:{membership}", m.id);
                        vars["link"] = json!(link);
                        self.notify
                            .notify_member(
                                &mut tx,
                                *membership,
                                "announcement_approval",
                                &vars,
                                Some(&link),
                                Some(("announcement", id)),
                                &dedupe,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                        vars["link"] = json!(format!("{}{link}", self.public_url));
                        self.notify
                            .email_member(
                                &mut tx,
                                *membership,
                                "announcement_approval",
                                email,
                                &vars,
                                &dedupe,
                                Urgency::Urgent,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // BPMS-047: the author hears whether their announcement was approved or sent back.
            "announcement.decided.v1" => {
                let (id, author) =
                    (uuid_field(m, "announcement_id")?, uuid_field(m, "author_membership_id")?);
                let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
                if mark_consumed(&mut tx, CONSUMER, m.id).await.map_err(|e| e.to_string())? {
                    let email: Option<String> = sqlx::query_scalar(
                        "SELECT u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1 AND m.status = 'active'",
                    )
                    .bind(author)
                    .fetch_optional(tx.conn())
                    .await
                    .map_err(|e| e.to_string())?;
                    let link = format!("/admin/announcements?id={id}");
                    let dedupe = format!("announcement_decided:{}", m.id);
                    let mut vars = json!({"title": m.payload["title"], "outcome": m.payload["outcome"], "comment": m.payload["comment"], "link": link});
                    self.notify
                        .notify_member(
                            &mut tx,
                            author,
                            "announcement_decided",
                            &vars,
                            Some(&link),
                            Some(("announcement", id)),
                            &dedupe,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    if let Some(email) = email {
                        vars["link"] = json!(format!("{}{link}", self.public_url));
                        self.notify
                            .email_member(
                                &mut tx,
                                author,
                                "announcement_decided",
                                &email,
                                &vars,
                                &dedupe,
                                Urgency::Normal,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                tx.commit().await.map_err(|e| e.to_string())
            }
            // Writers already invalidate after commit; this covers a node that failed to (CFG-004).
            procera_settings::CHANGED_TOPIC => {
                if let Some(c) = &self.settings_cache {
                    if m.payload["floors"].as_bool() == Some(true) {
                        c.invalidate_floors().await;
                    } else {
                        c.invalidate_workspace(m.workspace_id).await;
                    }
                }
                Ok(())
            }
            _ => Ok(()), // no consumer for this topic yet
        }
    }
}

impl Bus for AppBus {
    async fn deliver(&self, msg: &Message) -> Result<(), String> {
        self.handle(msg).await?;
        self.queue_webhooks(msg).await
    }
}
