//! Re-acknowledgement on publish (BPMS-045, FR-PL03): when a version that requires it goes live, the
//! artifact's applicable audience is asked to acknowledge that exact version. The publish handler
//! starts the campaign in the publish transaction (so the response can link to it); the
//! `artifact.published.v1` consumer starts it for scheduled versions, as the system. Both go through
//! [`start`], and the campaigns context keeps at most one re-acknowledgement campaign per version.

use procera_campaigns::{Actor, Campaign, Campaigns, NewCampaign};
use procera_db::WsTx;
use procera_library::{Library, ReackSubject};
use procera_members::audience::Audience;
use procera_settings::Settings;
use uuid::Uuid;

/// Who a re-acknowledgement goes to: the artifact's applicability, or everyone when it has none.
pub fn audience(subject: &ReackSubject) -> Audience {
    subject
        .applicability
        .clone()
        .and_then(|v| serde_json::from_value::<Audience>(v).ok())
        .filter(|a| !a.is_empty())
        .unwrap_or(Audience { all: true, ..Audience::default() })
}

/// Starts the re-acknowledgement campaign of published version `version_id` if it requires one and
/// has none yet. `actor` is the publisher; `None` means the scheduler published it.
pub async fn start(
    library: &Library,
    campaigns: &Campaigns,
    settings: &Settings,
    tx: &mut WsTx<'_>,
    version_id: Uuid,
    actor: Option<&Actor>,
) -> Result<Option<Campaign>, String> {
    let Some(subject) = library.reack_subject(tx, version_id).await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if !subject.reack_required {
        return Ok(None);
    }
    let Some(by) = actor.map(|a| a.membership).or(subject.initiator) else {
        return Ok(None); // nobody to record it for (no publisher, scheduler or owner)
    };
    let window = library.reack_window(tx).await.map_err(|e| e.to_string())?;
    let mut defaults =
        crate::http::campaigns::campaign_defaults(settings, tx).await.map_err(|e| e.to_string())?;
    // New joiners matched later get the same window to acknowledge.
    defaults.due_in = window.whole_seconds();
    let new = NewCampaign {
        title: Some(subject.campaign_title()),
        version_id,
        audience: audience(&subject),
        due_at: Some(library.reack_due(tx).await.map_err(|e| e.to_string())?),
        snapshot: None,
        ack_text: None,
    };
    campaigns.start_reack(tx, actor, by, &new, &defaults).await.map_err(|e| e.to_string())
}
