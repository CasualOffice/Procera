//! Built-in template packs (BPMS-106 P0 subset, FR-G08, A-38).
//!
//! A pack is versioned, reviewed content shipped in code: draft documents (in the block model) and
//! an optional suggested acknowledgement campaign. Installing a pack creates each chosen document as
//! a **draft** owned by the installer, through [`Library::create`], so the normal lifecycle,
//! review and audit apply. One `template.installed` event records the whole install.
//!
//! Keys on conflict: a document whose key already exists is **skipped** (and reported, with the
//! existing artifact) unless the caller asks for a copy, which takes the first free `key-N` and
//! titles it "Title (N)". Installing twice therefore never duplicates silently.
//!
//! A campaign needs a published version (BPMS-046), so the suggested campaign is returned as a plan
//! for the caller to start once the document is published; nothing is stored for it.

mod content;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{Actor, Library, LibraryError, WsTx};

/// One document a pack creates.
#[derive(Debug, Clone, Copy)]
pub struct PackDocument {
    pub key: &'static str,
    pub kind: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub content: fn() -> Value,
}

/// The acknowledgement campaign a pack suggests for one of its documents.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CampaignSuggestion {
    pub document_key: &'static str,
    pub title: &'static str,
    /// Always everyone for the built-in packs.
    pub audience: &'static str,
    pub due_in_days: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct Pack {
    pub key: &'static str,
    pub version: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub documents: &'static [PackDocument],
    pub campaign: Option<CampaignSuggestion>,
}

pub const PACKS: &[Pack] = &[
    Pack {
        key: "policy-ack",
        version: "1.0.0",
        name: "Employee handbook & acknowledgement",
        description: "A short employee handbook and four starter policies everyone should know, with an acknowledgement campaign for the handbook. Adapt them before you publish.",
        documents: &[
            PackDocument {
                key: "employee-handbook",
                kind: "policy",
                title: "Employee handbook",
                summary: "Welcome, how to use the handbook, the policies that apply to everyone and the acknowledgement statement.",
                content: content::handbook,
            },
            PackDocument {
                key: "code-of-conduct",
                kind: "policy",
                title: "Code of conduct",
                summary: "Respect, honesty, conflicts of interest, gifts and how to raise a concern.",
                content: content::code_of_conduct,
            },
            PackDocument {
                key: "acceptable-use-of-it",
                kind: "policy",
                title: "Acceptable use of IT",
                summary: "Accounts and passwords, devices, data and reporting problems.",
                content: content::acceptable_use,
            },
            PackDocument {
                key: "leave-and-time-off",
                kind: "policy",
                title: "Leave and time off",
                summary: "Annual and sick leave, other leave and how to request time off.",
                content: content::leave,
            },
            PackDocument {
                key: "workplace-safety",
                kind: "policy",
                title: "Workplace safety",
                summary: "Everyone's responsibilities, emergencies, working from home and reporting incidents.",
                content: content::safety,
            },
        ],
        campaign: Some(CampaignSuggestion {
            document_key: "employee-handbook",
            title: "Acknowledge Employee handbook",
            audience: "everyone",
            due_in_days: 14,
        }),
    },
    Pack {
        key: "core-sops",
        version: "1.0.0",
        name: "Core procedures",
        description: "Two procedures most teams need from day one: welcoming a new starter and reporting an incident.",
        documents: &[
            PackDocument {
                key: "new-starter-onboarding",
                kind: "sop",
                title: "New starter onboarding",
                summary: "Roles, what to do before the first day, on day one and at the end of the first week.",
                content: content::onboarding_sop,
            },
            PackDocument {
                key: "reporting-an-incident",
                kind: "sop",
                title: "Reporting an incident",
                summary: "Five steps from making it safe to recording what will change.",
                content: content::incident_sop,
            },
        ],
        campaign: None,
    },
];

pub fn pack(key: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.key == key)
}

/// What to do when a document's key already exists in the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConflict {
    /// Leave the existing document alone and report it (default).
    #[default]
    Skip,
    /// Create a copy under the first free `key-N`.
    Copy,
}

impl OnConflict {
    pub fn parse(s: &str) -> Result<Self, LibraryError> {
        match s {
            "skip" => Ok(Self::Skip),
            "copy" => Ok(Self::Copy),
            _ => Err(LibraryError::Invalid("on_conflict must be skip or copy".into())),
        }
    }
}

/// What the caller chose in the install wizard.
#[derive(Debug, Clone, Default)]
pub struct Install {
    /// Document keys to create; `None` = all of them.
    pub documents: Option<Vec<String>>,
    pub plan_campaign: bool,
    pub on_conflict: OnConflict,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledDocument {
    /// The pack's key for the document.
    pub template_document: &'static str,
    /// `created`, `copied` (key was taken; created under `key`) or `skipped` (key was taken).
    pub outcome: &'static str,
    pub kind: &'static str,
    pub key: String,
    pub title: String,
    pub artifact_id: Uuid,
    /// The created draft; `None` when skipped.
    pub version_id: Option<Uuid>,
    /// The artifact's state (`draft` when created).
    pub state: String,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CampaignPlan {
    pub title: &'static str,
    pub audience: &'static str,
    pub due_in_days: u16,
    pub artifact_id: Uuid,
    pub document_title: String,
    /// `publish_first` until the document has a published version, then `ready`.
    pub status: &'static str,
    /// The campaigns page with this document and deadline filled in.
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstallReport {
    pub template: &'static str,
    pub version: &'static str,
    pub documents: Vec<InstalledDocument>,
    pub campaign: Option<CampaignPlan>,
}

impl Pack {
    /// The documents `install` selects, in pack order. Unknown or no keys are refused.
    pub fn select(&self, install: &Install) -> Result<Vec<&'static PackDocument>, LibraryError> {
        let Some(keys) = &install.documents else { return Ok(self.documents.iter().collect()) };
        if keys.is_empty() {
            return Err(LibraryError::Invalid("choose at least one document".into()));
        }
        if let Some(k) = keys.iter().find(|k| !self.documents.iter().any(|d| d.key == k.as_str())) {
            return Err(LibraryError::Invalid(format!("this template has no document {k:?}")));
        }
        Ok(self.documents.iter().filter(|d| keys.iter().any(|k| k == d.key)).collect())
    }

    /// Kinds the selected documents create (the caller checks `<kind>:create` for each).
    pub fn kinds(docs: &[&PackDocument]) -> Vec<&'static str> {
        let mut k: Vec<&'static str> = docs.iter().map(|d| d.kind).collect();
        k.sort_unstable();
        k.dedup();
        k
    }
}

/// An existing artifact with a pack document's key, if any: `(id, state)`.
pub async fn existing(
    tx: &mut WsTx<'_>,
    kind: &str,
    key: &str,
) -> Result<Option<(Uuid, String)>, LibraryError> {
    Ok(sqlx::query_as("SELECT id, state FROM artifacts WHERE kind = $1 AND key = $2")
        .bind(kind)
        .bind(key)
        .fetch_optional(tx.conn())
        .await?)
}

impl Library {
    /// Installs `pack` (see the module docs). Runs in the caller's transaction, so the drafts and
    /// every audit record commit together or not at all.
    pub async fn install_pack(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        pack: &Pack,
        install: &Install,
    ) -> Result<InstallReport, LibraryError> {
        let mut out = vec![];
        for d in pack.select(install)? {
            let found = existing(tx, d.kind, d.key).await?;
            let (key, title, outcome) = match (&found, install.on_conflict) {
                (Some((id, state)), OnConflict::Skip) => {
                    out.push(InstalledDocument {
                        template_document: d.key,
                        outcome: "skipped",
                        kind: d.kind,
                        key: d.key.to_owned(),
                        title: d.title.to_owned(),
                        artifact_id: *id,
                        version_id: None,
                        state: state.clone(),
                        link: format!("/library/{id}"),
                    });
                    continue;
                }
                (Some(_), OnConflict::Copy) => {
                    let mut n = 2;
                    loop {
                        let k = format!("{}-{n}", d.key);
                        if existing(tx, d.kind, &k).await?.is_none() {
                            break (k, format!("{} ({n})", d.title), "copied");
                        }
                        n += 1;
                        if n > 99 {
                            return Err(LibraryError::Conflict(format!("too many copies of {}", d.key)));
                        }
                    }
                }
                (None, _) => (d.key.to_owned(), d.title.to_owned(), "created"),
            };
            let (a, v) = self.create(tx, actor, d.kind, &key, &title, "standard", &(d.content)()).await?;
            out.push(InstalledDocument {
                template_document: d.key,
                outcome,
                kind: d.kind,
                link: format!("/library/{}", a.id),
                key: a.key,
                title: a.title,
                artifact_id: a.id,
                version_id: Some(v.id),
                state: a.state,
            });
        }
        let campaign = match pack.campaign.filter(|_| install.plan_campaign) {
            None => None,
            Some(c) => match out.iter().find(|d| d.template_document == c.document_key) {
                // The suggestion follows the handbook only when it is part of this install.
                None => None,
                Some(d) => {
                    let published = d.state == "published"
                        || self.artifact(tx, d.artifact_id).await?.current_published_version_id.is_some();
                    Some(CampaignPlan {
                        title: c.title,
                        audience: c.audience,
                        due_in_days: c.due_in_days,
                        artifact_id: d.artifact_id,
                        document_title: d.title.clone(),
                        status: if published { "ready" } else { "publish_first" },
                        link: format!(
                            "/admin/campaigns?new=1&doc={}&due_days={}",
                            d.artifact_id, c.due_in_days
                        ),
                    })
                }
            },
        };
        let mut ev = AuditEvent::new(
            Some(actor.subject_ref),
            ActorKind::User,
            Category::Content,
            "template.installed",
            Outcome::Allowed,
        );
        ev.ip = actor.ip.clone();
        ev.user_agent = actor.user_agent.clone();
        ev.diff = Some(json!({
            "template": pack.key,
            "version": pack.version,
            "documents": out.iter().map(|d| json!({"template_document": d.template_document, "outcome": d.outcome, "key": d.key, "artifact_id": d.artifact_id})).collect::<Vec<_>>(),
            "campaign_planned": campaign.is_some(),
        }));
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(InstallReport { template: pack.key, version: pack.version, documents: out, campaign })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_pack_document_is_valid_block_content_with_the_template_notice() {
        for p in PACKS {
            for d in p.documents {
                let c = (d.content)();
                crate::check_content(&c).unwrap_or_else(|e| panic!("{}/{}: {e}", p.key, d.key));
                assert!(crate::blocks::image_ids(&c).is_empty(), "{}/{}: no images", p.key, d.key);
                let first = &c["blocks"][0];
                assert_eq!(
                    (first["type"].as_str(), first["tone"].as_str()),
                    (Some("callout"), Some("warning"))
                );
                assert!(first["text"].as_str().is_some_and(|t| t.starts_with("Template to adapt.")));
                assert!(crate::AUTHORABLE.contains(&d.kind));
                assert!(crate::permission(d.kind, "create").is_ok());
                let text = c.to_string().to_lowercase();
                for claim in ["compliant", "certified", "legally binding", "\u{2014}"] {
                    assert!(!text.contains(claim), "{}/{}: avoid {claim:?}", p.key, d.key);
                }
            }
        }
    }

    #[test]
    fn keys_are_unique_and_valid_and_room_is_left_for_copies() {
        let mut packs = HashSet::new();
        let mut docs = HashSet::new();
        for p in PACKS {
            assert!(packs.insert(p.key), "duplicate pack {}", p.key);
            assert!(
                p.version.split('.').count() == 3 && p.version.split('.').all(|n| n.parse::<u32>().is_ok())
            );
            for d in p.documents {
                // Unique across packs too, so two packs never fight over one artifact.
                assert!(docs.insert((d.kind, d.key)), "duplicate document {}", d.key);
                let copy = format!("{}-99", d.key);
                assert!(d.key.len() >= 2 && copy.len() <= 64, "{}: key and its copies fit 64", d.key);
                assert!(d.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
                assert!(!d.title.is_empty() && d.title.len() + 5 <= 200);
            }
            if let Some(c) = p.campaign {
                assert!(p.documents.iter().any(|d| d.key == c.document_key), "{}: campaign target", p.key);
                assert!(c.due_in_days > 0);
            }
        }
        assert!(pack("policy-ack").is_some_and(|p| p.campaign.is_some() && p.documents.len() >= 4));
    }

    #[test]
    fn selection_keeps_pack_order_and_refuses_unknown_or_empty() {
        let p = pack("policy-ack").copied().unwrap_or(PACKS[0]);
        let all = p.select(&Install::default()).map(|v| v.len()).unwrap_or(0);
        assert_eq!(all, p.documents.len());
        let pick = Install {
            documents: Some(vec!["workplace-safety".into(), "employee-handbook".into()]),
            ..Install::default()
        };
        let keys: Vec<_> = p.select(&pick).map(|v| v.iter().map(|d| d.key).collect()).unwrap_or_default();
        assert_eq!(keys, ["employee-handbook", "workplace-safety"]);
        for bad in [vec![], vec!["nope".to_owned()]] {
            let r = p.select(&Install { documents: Some(bad), ..Install::default() });
            assert!(matches!(r, Err(LibraryError::Invalid(_))));
        }
        assert_eq!(Pack::kinds(&p.select(&Install::default()).unwrap_or_default()), ["policy"]);
        assert_eq!(OnConflict::parse("copy").ok(), Some(OnConflict::Copy));
        assert!(OnConflict::parse("overwrite").is_err());
    }
}
