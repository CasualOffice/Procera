//! Announcement acknowledgements on the campaign engine (BPMS-047, FR-AN04, FR-AK01 "one engine").
//! When an announcement that requires acknowledgement goes live, its recipients get a campaign with
//! the workspace's reminder ladder and manager escalation (`campaign.*`). The publish and approve
//! handlers start it in their transaction (so the response links to it); the
//! `announcement.published.v1` consumer starts it for scheduled announcements, as the system. Both
//! go through [`start`], and the campaigns context keeps at most one campaign per announcement.

use procera_announcements::Announcements;
use procera_campaigns::{Actor, AnnouncementSubject, Campaign, Campaigns};
use procera_db::WsTx;
use procera_settings::Settings;
use uuid::Uuid;

/// Starts the acknowledgement campaign of published announcement `id` if it requires one and has
/// none yet. `actor` is the publisher or approver; `None` means the scheduler published it.
///
/// The deadline is the announcement's `ack_by`, else going live + `campaign.default_due`, never
/// later than its expiry (A-32).
pub async fn start(
    announcements: &Announcements,
    campaigns: &Campaigns,
    settings: &Settings,
    tx: &mut WsTx<'_>,
    id: Uuid,
    actor: Option<&Actor>,
) -> Result<Option<Campaign>, String> {
    let a = announcements.get(tx, id).await.map_err(|e| e.to_string())?;
    let (Some(live_at), Some(hash)) = (a.published_at, a.content_hash.clone()) else {
        return Ok(None);
    };
    if !a.require_ack || a.state != "published" {
        return Ok(None);
    }
    let defaults =
        crate::http::campaigns::campaign_defaults(settings, tx).await.map_err(|e| e.to_string())?;
    let due = a
        .ack_by
        .filter(|by| *by > live_at)
        .unwrap_or(live_at + time::Duration::seconds(defaults.due_in))
        .min(a.expires_at);
    // Who it is recorded as created by: the publisher, else the approver, else the author.
    let by =
        actor.map(|x| x.membership).or(a.published_by).or(a.decided_by).unwrap_or(a.author_membership_id);
    let subject = AnnouncementSubject {
        announcement_id: a.id,
        title: a.title.clone(),
        content_hash: hash,
        audience: a.audience.clone(),
        recipients: announcements.recipients(tx, a.id).await.map_err(|e| e.to_string())?,
        due_at: due,
    };
    campaigns.start_announcement(tx, actor, by, &subject, &defaults).await.map_err(|e| e.to_string())
}
