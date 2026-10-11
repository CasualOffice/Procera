//! Version numbers and re-acknowledgement on publish (BPMS-045; FR-PL03, FR-G03, docs/20 §2.6).
//!
//! - Version 1 publishes as `1.0`. Later versions are published as a **major** (`N+1.0`, the
//!   default) or a **minor** (`N.M+1`) change of the latest version that has a number. The number is
//!   fixed when the version is published, or when it is scheduled.
//! - Whether going live asks the applicable audience to acknowledge again is decided at the same
//!   moment from `governance.reack.policy`: `every_publish` (any version after the first),
//!   `major_only` (majors after the first) or `none`.
//! - The campaign itself belongs to the campaigns context; the publish handler and the
//!   `artifact.published.v1` consumer start it from [`ReackSubject`].

use serde::Serialize;
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{Library, LibraryError, WsTx};

/// How much a new version changes (the caller's choice; default major).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Change {
    #[default]
    Major,
    Minor,
}

impl Change {
    pub fn parse(s: &str) -> Result<Self, LibraryError> {
        match s {
            "major" => Ok(Self::Major),
            "minor" => Ok(Self::Minor),
            _ => Err(LibraryError::Invalid("change must be major or minor".into())),
        }
    }
}

/// `governance.reack.policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReackPolicy {
    MajorOnly,
    EveryPublish,
    None,
}

impl ReackPolicy {
    pub fn parse(s: &str) -> Self {
        match s {
            "every_publish" => Self::EveryPublish,
            "none" => Self::None,
            _ => Self::MajorOnly,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MajorOnly => "major_only",
            Self::EveryPublish => "every_publish",
            Self::None => "none",
        }
    }

    /// Whether publishing version `version_no` as `change` asks for a re-acknowledgement.
    pub fn requires(self, version_no: i32, change: Change) -> bool {
        version_no > 1
            && match self {
                Self::EveryPublish => true,
                Self::MajorOnly => change == Change::Major,
                Self::None => false,
            }
    }
}

/// The next number after `base` (`major.minor`) for `change`.
pub fn bump(base: &str, change: Change) -> String {
    let (major, minor) = base.split_once('.').unwrap_or((base, "0"));
    let (major, minor) = (major.parse::<u32>().unwrap_or(1), minor.parse::<u32>().unwrap_or(0));
    match change {
        Change::Major => format!("{}.0", major + 1),
        Change::Minor => format!("{major}.{}", minor + 1),
    }
}

/// What publishing a version would do, for the publish dialog.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PublishPlan {
    pub version_no: i32,
    pub next_semver_major: String,
    pub next_semver_minor: String,
    pub reack_policy: &'static str,
    pub reack_required_if_major: bool,
    pub reack_required_if_minor: bool,
    /// `governance.reack.window`, seconds.
    pub reack_window_secs: i64,
}

/// What the re-acknowledgement campaign of a published version needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ReackSubject {
    pub version_id: Uuid,
    pub lifecycle: String,
    pub reack_required: bool,
    pub semver: Option<String>,
    pub artifact_title: String,
    /// The artifact's applicability; `None` = everyone.
    pub applicability: Option<Value>,
    /// Who the campaign is created for: the publisher, else who scheduled it, else the owner.
    pub initiator: Option<Uuid>,
}

impl ReackSubject {
    /// "Re-acknowledge <title> v<semver>", within the 200-character campaign title limit.
    pub fn campaign_title(&self) -> String {
        let v = self.semver.as_deref().unwrap_or("");
        let room = 200 - "Re-acknowledge  v".len() - v.len();
        let title: String = self.artifact_title.chars().take(room).collect();
        format!("Re-acknowledge {title} v{v}")
    }
}

impl Library {
    pub async fn reack_policy(&self, tx: &mut WsTx<'_>) -> Result<ReackPolicy, LibraryError> {
        let v = self.settings.effective(tx, "governance.reack.policy").await?.value;
        Ok(ReackPolicy::parse(v.as_str().unwrap_or("major_only")))
    }

    /// `governance.reack.window`: how long the audience has to acknowledge again.
    pub async fn reack_window(&self, tx: &mut WsTx<'_>) -> Result<Duration, LibraryError> {
        let v = self.settings.effective(tx, "governance.reack.window").await?.value;
        Ok(Duration::seconds(v.as_i64().unwrap_or(14 * 86_400)))
    }

    /// The number the latest earlier numbered version carries (`1.0` when there is none), which a
    /// new version of `artifact_id` numbered `version_no` bumps.
    pub(crate) async fn semver_base(
        tx: &mut WsTx<'_>,
        artifact_id: Uuid,
        version_no: i32,
    ) -> Result<Option<String>, LibraryError> {
        if version_no <= 1 {
            return Ok(None);
        }
        let base: Option<String> = sqlx::query_scalar(
            "SELECT semver FROM artifact_versions WHERE artifact_id = $1 AND version_no < $2 AND semver IS NOT NULL ORDER BY version_no DESC LIMIT 1",
        )
        .bind(artifact_id)
        .bind(version_no)
        .fetch_optional(tx.conn())
        .await?;
        Ok(Some(base.unwrap_or_else(|| format!("{}.0", version_no - 1))))
    }

    /// The number a version gets when published as `change`.
    pub(crate) async fn next_semver(
        tx: &mut WsTx<'_>,
        artifact_id: Uuid,
        version_no: i32,
        change: Change,
    ) -> Result<String, LibraryError> {
        Ok(match Self::semver_base(tx, artifact_id, version_no).await? {
            None => "1.0".to_owned(),
            Some(base) => bump(&base, change),
        })
    }

    /// What publishing version `id` would do (numbers and re-acknowledgement), for the dialog.
    pub async fn publish_plan(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<PublishPlan, LibraryError> {
        let (artifact_id, version_no): (Uuid, i32) =
            sqlx::query_as("SELECT artifact_id, version_no FROM artifact_versions WHERE id = $1")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(LibraryError::NotFound)?;
        let policy = self.reack_policy(tx).await?;
        Ok(PublishPlan {
            version_no,
            next_semver_major: Self::next_semver(tx, artifact_id, version_no, Change::Major).await?,
            next_semver_minor: Self::next_semver(tx, artifact_id, version_no, Change::Minor).await?,
            reack_policy: policy.as_str(),
            reack_required_if_major: policy.requires(version_no, Change::Major),
            reack_required_if_minor: policy.requires(version_no, Change::Minor),
            reack_window_secs: self.reack_window(tx).await?.whole_seconds(),
        })
    }

    /// The re-acknowledgement subject of version `id` (`None` if it does not exist).
    pub async fn reack_subject(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
    ) -> Result<Option<ReackSubject>, LibraryError> {
        type Row = (String, bool, Option<String>, String, Option<Value>, Option<Uuid>);
        let r: Option<Row> = sqlx::query_as(
            "SELECT v.lifecycle, v.reack_required, v.semver, a.title, a.applicability,
                    coalesce(v.published_by, v.scheduled_by, a.owner_membership_id)
             FROM artifact_versions v JOIN artifacts a ON a.workspace_id = v.workspace_id AND a.id = v.artifact_id
             WHERE v.id = $1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        Ok(r.map(|(lifecycle, reack_required, semver, artifact_title, applicability, initiator)| {
            ReackSubject {
                version_id: id,
                lifecycle,
                reack_required,
                semver,
                artifact_title,
                applicability,
                initiator,
            }
        }))
    }

    /// When a re-acknowledgement started now would be due.
    pub async fn reack_due(&self, tx: &mut WsTx<'_>) -> Result<OffsetDateTime, LibraryError> {
        Ok(self.clock.now() + self.reack_window(tx).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_bump_by_change() {
        assert_eq!(bump("1.0", Change::Major), "2.0");
        assert_eq!(bump("1.0", Change::Minor), "1.1");
        assert_eq!(bump("2.9", Change::Minor), "2.10");
        assert_eq!(bump("2.10", Change::Major), "3.0");
    }

    #[test]
    fn policy_decides_reack() {
        use Change::*;
        for (p, v, c, want) in [
            (ReackPolicy::MajorOnly, 1, Major, false),
            (ReackPolicy::MajorOnly, 2, Major, true),
            (ReackPolicy::MajorOnly, 2, Minor, false),
            (ReackPolicy::EveryPublish, 1, Major, false),
            (ReackPolicy::EveryPublish, 3, Minor, true),
            (ReackPolicy::None, 2, Major, false),
        ] {
            assert_eq!(p.requires(v, c), want, "{p:?} v{v} {c:?}");
        }
        assert_eq!(ReackPolicy::parse("bogus"), ReackPolicy::MajorOnly);
    }

    #[test]
    fn campaign_title_fits() {
        let s = ReackSubject {
            version_id: Uuid::nil(),
            lifecycle: "published".into(),
            reack_required: true,
            semver: Some("12.3".into()),
            artifact_title: "x".repeat(200),
            applicability: None,
            initiator: None,
        };
        assert!(s.campaign_title().chars().count() <= 200);
        assert!(s.campaign_title().ends_with(" v12.3"));
    }
}
