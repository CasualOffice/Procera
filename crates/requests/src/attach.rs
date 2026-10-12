//! Files on requests (BPMS-060, BPMS-101 `file` fields): the requester attaches files in a form
//! field, and a decider may attach files with a decision (a signed quote).
//!
//! - The form sends file ids. Each must be a file of this workspace (RLS), uploaded by the person
//!   attaching it, `clean` after the scan (a file still scanning is refused with "Waiting for the
//!   virus scan…", a quarantined one with a clear message) and of a type the request preview
//!   serves (images, PDF, Word, Excel, CSV; [`procera_files::preview_kind`]).
//! - The instance stores each as `{id, name, size, content_type, hash}`: the expression `file` shape
//!   plus its id.
//! - Every attached file is linked to the request (`file_links` object type `request`), so retention
//!   and holds treat it as part of the execution record and only people who may see the request may
//!   download or preview it.

use std::collections::BTreeSet;

use std::collections::BTreeMap;

use procera_db::WsTx;
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::catalogue::Form;
use crate::forms::FieldError;
use crate::{RequestError, Requests};

/// What a decision carries besides its outcome, comment and fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionExtras {
    /// Files the decider attaches (uploaded by them, clean).
    pub files: Vec<Uuid>,
    /// The published policy version the decider was shown.
    pub policy_version: Option<Uuid>,
}

/// File ids held by the form's file fields.
pub(crate) fn in_vars(form: &Form, vars: &BTreeMap<String, Value>) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = Vec::new();
    for f in form.fields.iter().filter(|f| f.ty == "file") {
        for id in vars.get(&f.name).map(ids_in).unwrap_or_default() {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// The most files one field (or one decision) takes.
pub const MAX_FILES: usize = 10;

/// An attachment as the request page and decision panel list it, with the file's current state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Attachment {
    pub id: Uuid,
    pub name: String,
    pub size: i64,
    pub state: String,
    pub sniffed_type: Option<String>,
    /// Where it was attached: the form field's label, or the step it was decided with.
    pub source: String,
    /// The form field it belongs to (`None` for files attached with a decision).
    pub field: Option<String>,
}

/// File ids named by a stored or submitted value: an id, a `{id}` object or a list of either.
pub fn ids_in(v: &Value) -> Vec<Uuid> {
    match v {
        Value::String(s) => Uuid::parse_str(s).ok().into_iter().collect(),
        Value::Object(o) => o.get("id").map(ids_in).unwrap_or_default(),
        Value::Array(items) => items.iter().flat_map(ids_in).collect(),
        _ => vec![],
    }
}

type FileRow = (Uuid, String, i64, String, Option<String>, Option<Vec<u8>>, Uuid);

/// Checks files someone attaches and returns their stored shape, or `(code, message)`. Files in
/// `existing` (already on the request) are accepted whoever uploaded them.
pub(crate) async fn check(
    tx: &mut WsTx<'_>,
    ids: &[Uuid],
    uploader: Uuid,
    existing: &BTreeSet<Uuid>,
) -> Result<Result<Vec<Value>, (&'static str, String)>, sqlx::Error> {
    let mut out = Vec::new();
    for id in ids {
        let row: Option<FileRow> = sqlx::query_as(
            "SELECT id, name, size, state, sniffed_type, sha256, uploaded_by FROM files WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        let Some((id, name, size, state, sniffed, sha, by)) = row else {
            return Ok(Err(("unknown_file", "This file is not in this workspace. Upload it again.".into())));
        };
        if by != uploader && !existing.contains(&id) {
            return Ok(Err(("not_yours", format!("{name}: attach files you uploaded yourself"))));
        }
        match state.as_str() {
            "clean" => {}
            "uploading" | "scanning" => {
                return Ok(Err((
                    "scanning",
                    format!("Waiting for the virus scan of {name} to finish. Try again in a moment."),
                )));
            }
            "quarantined" => {
                return Ok(Err((
                    "quarantined",
                    format!("{name} was blocked by the virus scan and can't be attached. Remove it."),
                )));
            }
            _ => return Ok(Err(("rejected", format!("{name} can't be attached ({state}). Remove it.")))),
        }
        let Some((_, content_type)) = procera_files::preview_kind(sniffed.as_deref(), &name) else {
            return Ok(Err(("file_type", format!("{name}: attach images, PDF, Word, Excel or CSV files"))));
        };
        let hash: String = sha.unwrap_or_default().iter().map(|b| format!("{b:02x}")).collect();
        out.push(json!({ "id": id, "name": name, "size": size, "content_type": content_type, "hash": hash }));
    }
    Ok(Ok(out))
}

/// Links files to a request (idempotent).
pub(crate) async fn link(
    tx: &mut WsTx<'_>,
    request: Uuid,
    ids: &[Uuid],
    by: Uuid,
    at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO file_links (workspace_id, file_id, object_type, object_id, linked_by, linked_at)
         SELECT $1, f, 'request', $2, $3, $4 FROM unnest($5::uuid[]) AS f ON CONFLICT DO NOTHING",
    )
    .bind(tx.ctx.workspace_id)
    .bind(request)
    .bind(by)
    .bind(at)
    .bind(ids)
    .execute(tx.conn())
    .await?;
    Ok(())
}

/// The files listed with `(source, field)`, in order, with their current state.
pub(crate) async fn listed(
    tx: &mut WsTx<'_>,
    files: &[(Uuid, String, Option<String>)],
) -> Result<Vec<Attachment>, sqlx::Error> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (id, source, field) in files {
        if !seen.insert((*id, field.clone())) {
            continue;
        }
        let row: Option<(String, i64, String, Option<String>)> =
            sqlx::query_as("SELECT name, size, state, sniffed_type FROM files WHERE id = $1")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?;
        if let Some((name, size, state, sniffed_type)) = row {
            out.push(Attachment {
                id: *id,
                name,
                size,
                state,
                sniffed_type,
                source: source.clone(),
                field: field.clone(),
            });
        }
    }
    Ok(out)
}

impl Requests {
    /// Replaces the ids in the form's file fields with the stored file shape, or records why a
    /// file cannot be attached.
    pub(crate) async fn attach_values(
        &self,
        tx: &mut WsTx<'_>,
        form: &Form,
        vals: &mut BTreeMap<String, Value>,
        uploader: Uuid,
        existing: &BTreeSet<Uuid>,
        errors: &mut Vec<FieldError>,
    ) -> Result<(), RequestError> {
        for f in form.fields.iter().filter(|f| f.ty == "file") {
            let Some(v) = vals.get(&f.name) else { continue };
            let ids = ids_in(v);
            match check(tx, &ids, uploader, existing).await? {
                Ok(refs) if f.multiple => {
                    vals.insert(f.name.clone(), Value::Array(refs));
                }
                Ok(refs) => {
                    vals.insert(f.name.clone(), refs.into_iter().next().unwrap_or(Value::Null));
                }
                Err((code, msg)) => errors.push(FieldError::new(&f.name, code, &msg)),
            }
        }
        Ok(())
    }

    /// Checks the files a decider attaches to their decision (errors point at `files`).
    pub(crate) async fn decision_files(
        &self,
        tx: &mut WsTx<'_>,
        ids: &[Uuid],
        by: Uuid,
    ) -> Result<Vec<Value>, RequestError> {
        let mut uniq: Vec<Uuid> = Vec::new();
        for i in ids {
            if !uniq.contains(i) {
                uniq.push(*i);
            }
        }
        if uniq.len() > MAX_FILES {
            let msg = format!("Attach at most {MAX_FILES} files");
            return Err(RequestError::Invalid(vec![FieldError::new("files", "too_many", &msg)]));
        }
        match check(tx, &uniq, by, &BTreeSet::new()).await? {
            Ok(refs) => Ok(refs),
            Err((code, msg)) => Err(RequestError::Invalid(vec![FieldError::new("files", code, &msg)])),
        }
    }

    /// Every file on a request: its file fields (labelled), then files decided with, step by step.
    pub(crate) async fn attachments(
        &self,
        tx: &mut WsTx<'_>,
        instance: Uuid,
        form: &Form,
        vars: &BTreeMap<String, Value>,
    ) -> Result<Vec<Attachment>, RequestError> {
        let mut wanted: Vec<(Uuid, String, Option<String>)> = Vec::new();
        for f in form.fields.iter().filter(|f| f.ty == "file") {
            for id in vars.get(&f.name).map(ids_in).unwrap_or_default() {
                wanted.push((id, f.label.clone(), Some(f.name.clone())));
            }
        }
        let decided: Vec<(String, Value)> = sqlx::query_as(
            "SELECT name, decision FROM work_items WHERE instance_id = $1 AND decision ? 'files' ORDER BY completed_at, id",
        )
        .bind(instance)
        .fetch_all(tx.conn())
        .await?;
        for (step, d) in decided {
            for id in ids_in(&d["files"]) {
                wanted.push((id, format!("{step} decision"), None));
            }
        }
        Ok(listed(tx, &wanted).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_come_from_ids_objects_and_lists() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        assert_eq!(ids_in(&json!([a.to_string(), {"id": b.to_string(), "name": "x"}])), [a, b]);
        assert_eq!(ids_in(&json!(a.to_string())), [a]);
        assert!(ids_in(&json!(["nope", 3])).is_empty());
    }
}
