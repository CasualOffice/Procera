//! Collections, owners and RACI (BPMS-041; FR-G01, docs/05 §3.2 and §6.1, docs/12 §7).
//!
//! - **Collections** form a tree (`path` = ancestors, root first, self last). Names are unique among
//!   siblings. A collection can only be deleted when it holds no documents and no sub-collections.
//! - **Visibility** is an audience on a collection (`None` = everyone). Readers with only `view`
//!   permission see a collection, and the documents filed under it, only when every collection on
//!   its path includes them (A-14 extended); authors and admins see everything.
//! - **Owner** is the accountable person for a document (`owner_membership_id`).
//! - **RACI** lists members or groups per role (responsible, accountable, consulted, informed) and
//!   is exportable as CSV.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Actor, Artifact, Library, LibraryError, Reader, WsTx};

#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct Collection {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub path: Vec<Uuid>,
    pub visibility: Option<Value>,
    pub revision: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Documents filed directly in it that the caller may see.
    #[sqlx(default)]
    pub documents: i64,
}

pub const RACI_ROLES: &[&str] = &["responsible", "accountable", "consulted", "informed"];

/// One RACI assignee: a member or a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignee {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    /// Display name (output only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// RACI of a document, role → assignees.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Raci {
    #[serde(default)]
    pub responsible: Vec<Assignee>,
    #[serde(default)]
    pub accountable: Vec<Assignee>,
    #[serde(default)]
    pub consulted: Vec<Assignee>,
    #[serde(default)]
    pub informed: Vec<Assignee>,
}

impl Raci {
    fn role(&self, r: &str) -> &[Assignee] {
        match r {
            "responsible" => &self.responsible,
            "accountable" => &self.accountable,
            "consulted" => &self.consulted,
            _ => &self.informed,
        }
    }
    fn role_mut(&mut self, r: &str) -> &mut Vec<Assignee> {
        match r {
            "responsible" => &mut self.responsible,
            "accountable" => &mut self.accountable,
            "consulted" => &mut self.consulted,
            _ => &mut self.informed,
        }
    }
}

const COLLECTION_COLS: &str = "c.id, c.parent_id, c.name, c.path, c.visibility, c.revision, c.created_at";

fn clean_name(name: &str) -> Result<String, LibraryError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 120 {
        return Err(LibraryError::Invalid("a collection name must be 1–120 characters".into()));
    }
    Ok(n.to_owned())
}

impl Library {
    /// Collections the reader may see (all for collection managers and authors), with how many
    /// visible documents each holds directly. Ordered by path.
    pub async fn collections(
        &self,
        tx: &mut WsTx<'_>,
        reader: &Reader<'_>,
        all: bool,
    ) -> Result<Vec<Collection>, LibraryError> {
        let full: Vec<String> = reader.full_kinds.iter().map(|k| (*k).to_owned()).collect();
        Ok(sqlx::query_as(&format!(
            "SELECT {COLLECTION_COLS},
                    (SELECT count(*) FROM artifacts a WHERE a.collection_id = c.id AND (a.kind = ANY($3) OR {})) AS documents
             FROM collections c
             WHERE ($1 OR procera_collection_visible(c.id, $4)) AND $2::int IS NULL
             ORDER BY (SELECT string_agg(lower(x.name), '/' ORDER BY array_position(c.path, x.id)) FROM collections x WHERE x.id = ANY(c.path)), c.id",
            crate::READER_VISIBLE
        ))
        .bind(all)
        .bind(None::<i32>) // keeps READER_VISIBLE's $4 = membership
        .bind(&full)
        .bind(reader.membership)
        .fetch_all(tx.conn())
        .await?)
    }

    pub async fn collection(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Collection, LibraryError> {
        sqlx::query_as(&format!("SELECT {COLLECTION_COLS} FROM collections c WHERE c.id = $1"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(LibraryError::NotFound)
    }

    async fn sibling_taken(
        tx: &mut WsTx<'_>,
        parent: Option<Uuid>,
        name: &str,
        except: Option<Uuid>,
    ) -> Result<bool, LibraryError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM collections WHERE parent_id IS NOT DISTINCT FROM $1 AND lower(name) = lower($2) AND ($3::uuid IS NULL OR id <> $3))",
        )
        .bind(parent)
        .bind(name)
        .bind(except)
        .fetch_one(tx.conn())
        .await?)
    }

    pub async fn create_collection(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        name: &str,
        parent: Option<Uuid>,
    ) -> Result<Collection, LibraryError> {
        let name = clean_name(name)?;
        let mut path = match parent {
            Some(p) => {
                self.collection(tx, p)
                    .await
                    .map_err(|_| LibraryError::Invalid("the parent collection does not exist".into()))?
                    .path
            }
            None => vec![],
        };
        if Self::sibling_taken(tx, parent, &name, None).await? {
            return Err(LibraryError::Conflict(format!("a collection named \"{name}\" already exists here")));
        }
        let id = self.ids.new_id();
        path.push(id);
        sqlx::query("INSERT INTO collections (workspace_id, id, parent_id, name, path, created_at) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(tx.ctx.workspace_id).bind(id).bind(parent).bind(&name).bind(&path).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "collection.created",
            ("collection", id),
            json!({"name": name, "parent_id": parent}),
            None,
        )
        .await?;
        self.collection(tx, id).await
    }

    /// Renames and/or moves a collection (and its subtree). A collection cannot move under itself.
    pub async fn update_collection(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        name: Option<&str>,
        parent: Option<Option<Uuid>>,
    ) -> Result<Collection, LibraryError> {
        let before = self.collection(tx, id).await?;
        let name = match name {
            Some(n) => clean_name(n)?,
            None => before.name.clone(),
        };
        let new_parent = parent.unwrap_or(before.parent_id);
        let mut new_prefix = match new_parent {
            Some(p) => {
                let pc = self
                    .collection(tx, p)
                    .await
                    .map_err(|_| LibraryError::Invalid("the parent collection does not exist".into()))?;
                if pc.path.contains(&id) {
                    return Err(LibraryError::Invalid("a collection cannot move under itself".into()));
                }
                pc.path
            }
            None => vec![],
        };
        if Self::sibling_taken(tx, new_parent, &name, Some(id)).await? {
            return Err(LibraryError::Conflict(format!("a collection named \"{name}\" already exists here")));
        }
        new_prefix.push(id);
        // Rewrite the path of the collection and every descendant: new prefix + the part after `id`.
        sqlx::query(
            "UPDATE collections SET path = $2::uuid[] || path[array_position(path, $1) + 1:], revision = revision + 1 WHERE $1 = ANY(path)",
        )
        .bind(id)
        .bind(&new_prefix)
        .execute(tx.conn())
        .await?;
        sqlx::query("UPDATE collections SET name = $2, parent_id = $3 WHERE id = $1")
            .bind(id)
            .bind(&name)
            .bind(new_parent)
            .execute(tx.conn())
            .await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "collection.updated",
            ("collection", id),
            json!({"name": {"from": before.name, "to": name}, "parent_id": {"from": before.parent_id, "to": new_parent}}),
            None,
        )
        .await?;
        self.collection(tx, id).await
    }

    pub async fn delete_collection(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
    ) -> Result<(), LibraryError> {
        let c = self.collection(tx, id).await?;
        let (children, docs): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM collections WHERE parent_id = $1), (SELECT count(*) FROM artifacts WHERE collection_id = $1)",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if children > 0 || docs > 0 {
            return Err(LibraryError::Conflict("move its documents and sub-collections out first".into()));
        }
        sqlx::query("DELETE FROM collections WHERE id = $1").bind(id).execute(tx.conn()).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "collection.deleted",
            ("collection", id),
            json!({"name": c.name}),
            None,
        )
        .await?;
        Ok(())
    }

    /// Sets who may see the collection (an audience, validated by the caller; `None` = everyone).
    pub async fn set_collection_visibility(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        visibility: Option<&Value>,
    ) -> Result<Collection, LibraryError> {
        let before = self.collection(tx, id).await?;
        sqlx::query("UPDATE collections SET visibility = $2, revision = revision + 1 WHERE id = $1")
            .bind(id)
            .bind(visibility)
            .execute(tx.conn())
            .await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "collection.visibility_set",
            ("collection", id),
            json!({"from": before.visibility, "to": visibility}),
            None,
        )
        .await?;
        self.collection(tx, id).await
    }

    /// Files a document in a collection (`None` = top level).
    pub async fn set_artifact_collection(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
        collection: Option<Uuid>,
    ) -> Result<Artifact, LibraryError> {
        let before = self.artifact(tx, artifact_id).await?;
        if let Some(c) = collection {
            self.collection(tx, c)
                .await
                .map_err(|_| LibraryError::Invalid("the collection does not exist".into()))?;
        }
        sqlx::query("UPDATE artifacts SET collection_id = $2, revision = revision + 1 WHERE id = $1")
            .bind(artifact_id)
            .bind(collection)
            .execute(tx.conn())
            .await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.collection_set",
            ("artifact", artifact_id),
            json!({"from": before.collection_id, "to": collection}),
            None,
        )
        .await?;
        self.artifact(tx, artifact_id).await
    }

    /// Changes the accountable owner (an active member of the workspace).
    pub async fn set_owner(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
        owner: Uuid,
    ) -> Result<Artifact, LibraryError> {
        let before = self.artifact(tx, artifact_id).await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active')",
        )
        .bind(owner)
        .fetch_one(tx.conn())
        .await?;
        if !active {
            return Err(LibraryError::Invalid("the owner must be an active member".into()));
        }
        sqlx::query("UPDATE artifacts SET owner_membership_id = $2, revision = revision + 1 WHERE id = $1")
            .bind(artifact_id)
            .bind(owner)
            .execute(tx.conn())
            .await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.owner_set",
            ("artifact", artifact_id),
            json!({"from": before.owner_membership_id, "to": owner}),
            None,
        )
        .await?;
        self.artifact(tx, artifact_id).await
    }

    /// The RACI of a document with display names.
    pub async fn raci(&self, tx: &mut WsTx<'_>, artifact_id: Uuid) -> Result<Raci, LibraryError> {
        let rows: Vec<(String, Option<Uuid>, Option<Uuid>, Option<String>)> = sqlx::query_as(
            "SELECT r.role, r.membership_id, r.group_id, coalesce(u.display_name, g.name)
             FROM artifact_raci r
             LEFT JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id
             LEFT JOIN users u ON u.id = m.user_id
             LEFT JOIN groups g ON g.workspace_id = r.workspace_id AND g.id = r.group_id
             WHERE r.artifact_id = $1 ORDER BY r.role, lower(coalesce(u.display_name, g.name))",
        )
        .bind(artifact_id)
        .fetch_all(tx.conn())
        .await?;
        let mut raci = Raci::default();
        for (role, membership_id, group_id, name) in rows {
            raci.role_mut(&role).push(Assignee { membership_id, group_id, name });
        }
        Ok(raci)
    }

    /// Replaces the RACI of a document. At most one accountable assignee (docs/05 §6.1).
    pub async fn set_raci(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        artifact_id: Uuid,
        raci: &Raci,
    ) -> Result<Raci, LibraryError> {
        self.artifact(tx, artifact_id).await?;
        if raci.accountable.len() > 1 {
            return Err(LibraryError::Invalid(
                "a document has at most one accountable person or group".into(),
            ));
        }
        let before = self.raci(tx, artifact_id).await?;
        sqlx::query("DELETE FROM artifact_raci WHERE artifact_id = $1")
            .bind(artifact_id)
            .execute(tx.conn())
            .await?;
        for role in RACI_ROLES {
            for a in raci.role(role) {
                if a.membership_id.is_some() == a.group_id.is_some() {
                    return Err(LibraryError::Invalid("each assignee is a member or a group".into()));
                }
                let ok: bool = sqlx::query_scalar(
                    "SELECT CASE WHEN $1::uuid IS NOT NULL THEN EXISTS (SELECT 1 FROM memberships WHERE id = $1 AND status = 'active') ELSE EXISTS (SELECT 1 FROM groups WHERE id = $2) END",
                )
                .bind(a.membership_id)
                .bind(a.group_id)
                .fetch_one(tx.conn())
                .await?;
                if !ok {
                    return Err(LibraryError::Invalid(
                        "an assignee is not an active member or a group of this workspace".into(),
                    ));
                }
                sqlx::query("INSERT INTO artifact_raci (workspace_id, id, artifact_id, role, membership_id, group_id) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING")
                    .bind(tx.ctx.workspace_id).bind(self.ids.new_id()).bind(artifact_id).bind(role).bind(a.membership_id).bind(a.group_id).execute(tx.conn()).await?;
            }
        }
        let after = self.raci(tx, artifact_id).await?;
        self.audit_artifact(
            tx,
            Some(actor),
            "artifact.raci_set",
            ("artifact", artifact_id),
            json!({"from": before, "to": after}),
            None,
        )
        .await?;
        Ok(after)
    }

    /// RACI of every document the reader may see (optionally in one collection subtree) as CSV rows:
    /// collection path, document, kind, owner, responsible, accountable, consulted, informed.
    pub async fn raci_csv(
        &self,
        tx: &mut WsTx<'_>,
        reader: &Reader<'_>,
        kinds: &[&str],
        collection: Option<Uuid>,
    ) -> Result<String, LibraryError> {
        let docs = self.list(tx, kinds, None, collection, reader).await?;
        let mut out =
            String::from("collection,document,key,kind,owner,responsible,accountable,consulted,informed\n");
        let cell = |s: &str| {
            // RFC 4180 quoting, and neutralise spreadsheet formulas (CSV injection).
            let s =
                if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_owned() };
            format!("\"{}\"", s.replace('"', "\"\""))
        };
        for a in docs {
            let path: Option<String> = match a.collection_id {
                Some(c) => sqlx::query_scalar(
                    "SELECT string_agg(x.name, ' / ' ORDER BY array_position(c.path, x.id)) FROM collections c JOIN collections x ON x.id = ANY(c.path) WHERE c.id = $1",
                )
                .bind(c)
                .fetch_one(tx.conn())
                .await?,
                None => None,
            };
            let owner: Option<String> = match a.owner_membership_id {
                Some(m) => sqlx::query_scalar("SELECT u.display_name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1")
                    .bind(m)
                    .fetch_optional(tx.conn())
                    .await?,
                None => None,
            };
            let raci = self.raci(tx, a.id).await?;
            let names =
                |v: &[Assignee]| v.iter().filter_map(|x| x.name.clone()).collect::<Vec<_>>().join("; ");
            out.push_str(
                &[
                    cell(path.as_deref().unwrap_or("")),
                    cell(&a.title),
                    cell(&a.key),
                    cell(&a.kind),
                    cell(owner.as_deref().unwrap_or("")),
                    cell(&names(&raci.responsible)),
                    cell(&names(&raci.accountable)),
                    cell(&names(&raci.consulted)),
                    cell(&names(&raci.informed)),
                ]
                .join(","),
            );
            out.push('\n');
        }
        Ok(out)
    }
}
