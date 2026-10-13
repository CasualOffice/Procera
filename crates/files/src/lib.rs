//! Files (BPMS-060/062; FR-F01–F03, F05–F07).
//!
//! - **Upload** is resumable: `create` reserves the file (size and type policy, storage quota), each
//!   `append` stores one chunk at the expected offset, and `complete` verifies, hashes, sniffs and
//!   scans it. A file is unusable until `clean`; infected files are `quarantined`, disallowed types
//!   `rejected` (and their bytes removed).
//! - **At rest** every chunk is AES-256-GCM under a per-file DEK (AAD `files:<id>:<chunk>`); the DEK
//!   is an envelope under the workspace KEK (`files.wrapped_dek`, rewrapped on rotation, shredded with
//!   the workspace).
//! - **Downloads** use short-lived signed tokens bound to the file, workspace and member; the server
//!   re-authorizes on every request.
//! - **Previews** (BPMS-061, FR-F04) serve only clean PNG, JPEG, GIF, WebP, PDF, DOCX, XLSX and CSV files,
//!   through their own token purpose (`file-preview`) with a shorter lifetime; JPEG metadata is
//!   stripped on the way out when `files.exif_strip` is on.
//! - **Immutability**: a file linked to a version that is no longer a draft cannot be unlinked or
//!   deleted (FR-F05).

pub mod exif;
pub mod scan;
pub mod sniff;
pub mod store;

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{KeyProvider, aead_open, aead_seal, mac, mac_eq, random_bytes};
use procera_db::{Db, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_keys::Keys;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use zeroize::Zeroizing;

pub use scan::{Scanner, Verdict};
pub use store::{LocalStore, S3Config, Store};

/// Largest chunk accepted in one request.
pub const MAX_CHUNK: usize = 8 * 1024 * 1024;
/// Signed download links live this long (FR-F03: ≤ 5 min).
pub const DOWNLOAD_TTL: Duration = Duration::minutes(5);
/// Preview links live this long: the dialog loads them at once, so they need not outlast a download link.
pub const PREVIEW_TTL: Duration = Duration::minutes(2);

/// How a sniffed type previews in the app, with the exact content type to serve: `image` (an
/// `<img>`), `pdf` (pdf.js), `docx` (rendered to inert HTML in an isolated frame), `xlsx` and `csv`
/// (a read-only table). CSV is plain text named `*.csv`. A fixed allow-list: SVG, HTML and every
/// other type are never served for preview.
pub fn preview_kind(sniffed: Option<&str>, name: &str) -> Option<(&'static str, &'static str)> {
    match sniffed? {
        "image/png" => Some(("image", "image/png")),
        "image/jpeg" => Some(("image", "image/jpeg")),
        "image/gif" => Some(("image", "image/gif")),
        "image/webp" => Some(("image", "image/webp")),
        "application/pdf" => Some(("pdf", "application/pdf")),
        sniff::DOCX => Some(("docx", sniff::DOCX)),
        sniff::XLSX => Some(("xlsx", sniff::XLSX)),
        "text/plain" if name.to_ascii_lowercase().ends_with(".csv") => {
            Some(("csv", "text/csv; charset=utf-8"))
        }
        _ => None,
    }
}

/// What a signed file link lets its bearer do. Each purpose has its own MAC context, so a download
/// token never verifies as a preview token or the other way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPurpose {
    Download,
    Preview,
}

impl LinkPurpose {
    fn context(self) -> &'static str {
        match self {
            Self::Download => "file-download",
            Self::Preview => "file-preview",
        }
    }
    fn ttl(self) -> Duration {
        match self {
            Self::Download => DOWNLOAD_TTL,
            Self::Preview => PREVIEW_TTL,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("not found")]
    NotFound,
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("too large: {0}")]
    TooLarge(String),
    #[error("internal error: {0}")]
    Internal(String),
}
fn internal(e: impl std::fmt::Display) -> FileError {
    FileError::Internal(e.to_string())
}
impl From<sqlx::Error> for FileError {
    fn from(e: sqlx::Error) -> Self {
        internal(e)
    }
}
impl From<procera_audit::AuditError> for FileError {
    fn from(e: procera_audit::AuditError) -> Self {
        internal(e)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct File {
    pub id: Uuid,
    pub name: String,
    pub declared_type: Option<String>,
    pub sniffed_type: Option<String>,
    pub size: i64,
    pub received: i64,
    pub state: String,
    pub scan_result: Option<String>,
    #[serde(serialize_with = "hex_opt")]
    pub sha256: Option<Vec<u8>>,
    pub uploaded_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    /// Last download or preview (FR-F06).
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_accessed_at: Option<OffsetDateTime>,
}

fn hex_opt<S: serde::Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(b) => s.serialize_str(&b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
        None => s.serialize_none(),
    }
}

/// Record classes a file can carry (docs/14 §4).
pub const RETENTION_CLASSES: &[&str] =
    &["governance_record", "execution_record", "case_file", "operational", "audit", "draft"];

const COLS: &str = "id, name, declared_type, sniffed_type, size, received, state, scan_result, sha256, uploaded_by, created_at, completed_at, last_accessed_at";

/// Who acts, for audit records.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Clone)]
pub struct Files {
    keys: Keys,
    root: Arc<dyn KeyProvider>,
    store: Store,
    scanner: Scanner,
    settings: procera_settings::Settings,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Files").field("scanner", &self.scanner.name()).finish_non_exhaustive()
    }
}

impl Files {
    pub fn new(
        db: Db,
        root: Arc<dyn KeyProvider>,
        store: impl Into<Store>,
        scanner: Scanner,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
    ) -> Self {
        let keys = Keys::new(db.clone(), root.clone(), clock.clone(), ids.clone());
        let settings = procera_settings::Settings::new(clock.clone(), ids.clone());
        Self { keys, root, store: store.into(), scanner, settings, clock, ids }
    }

    pub fn scanner_name(&self) -> &'static str {
        self.scanner.name()
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        action: &str,
        file: Uuid,
        diff: serde_json::Value,
        outcome: Outcome,
    ) -> Result<(), FileError> {
        let mut ev = AuditEvent::new(
            actor.map(|a| a.subject_ref),
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::File,
            action,
            outcome,
        )
        .object("file", file);
        ev.ip = actor.and_then(|a| a.ip.clone());
        ev.user_agent = actor.and_then(|a| a.user_agent.clone());
        ev.diff = Some(diff);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    async fn dek(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Zeroizing<[u8; 32]>, FileError> {
        let wrapped: Vec<u8> = sqlx::query_scalar("SELECT wrapped_dek FROM files WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(FileError::NotFound)?;
        let pt = self.keys.open(tx, &format!("files:{id}"), &wrapped).await.map_err(internal)?;
        let mut k = Zeroizing::new([0u8; 32]);
        if pt.len() != 32 {
            return Err(internal("bad file key"));
        }
        k.copy_from_slice(&pt);
        Ok(k)
    }

    pub async fn get(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<File, FileError> {
        sqlx::query_as(&format!("SELECT {COLS} FROM files WHERE id = $1 AND state <> 'deleted'"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(FileError::NotFound)
    }

    /// Storage used by the workspace (bytes of files that are not deleted or rejected).
    pub async fn used_bytes(&self, tx: &mut WsTx<'_>) -> Result<i64, FileError> {
        Ok(sqlx::query_scalar(
            "SELECT coalesce(sum(size), 0)::bigint FROM files WHERE state NOT IN ('deleted', 'rejected')",
        )
        .fetch_one(tx.conn())
        .await?)
    }

    /// Reserves a file: checks the size limit (`files.max_size`) and the workspace storage quota.
    pub async fn create(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        name: &str,
        size: i64,
        declared: Option<&str>,
    ) -> Result<File, FileError> {
        let name = name.trim().replace(['/', '\\', '\0'], "_");
        if name.is_empty() || name.chars().count() > 255 {
            return Err(FileError::Invalid("a file name is 1–255 characters".into()));
        }
        if size < 1 {
            return Err(FileError::Invalid("the file is empty".into()));
        }
        let max = self
            .settings
            .effective(tx, "files.max_size")
            .await
            .map_err(internal)?
            .value
            .as_i64()
            .unwrap_or(100 << 20);
        if size > max {
            return Err(FileError::TooLarge(format!("files may be at most {} MB here", max >> 20)));
        }
        let quota: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT max_storage_bytes FROM workspace_quotas WHERE target_workspace_id = $1",
        )
        .bind(tx.ctx.workspace_id)
        .fetch_optional(tx.conn())
        .await?;
        if let Some(Some(q)) = quota
            && self.used_bytes(tx).await? + size > q
        {
            return Err(FileError::TooLarge("the workspace's storage quota is full".into()));
        }
        let id = self.ids.new_id();
        let dek: Zeroizing<[u8; 32]> = Zeroizing::new(random_bytes());
        let wrapped = self.keys.seal(tx, &format!("files:{id}"), dek.as_ref()).await.map_err(internal)?;
        let key = format!("ws/{}/{id}", tx.ctx.workspace_id);
        sqlx::query("INSERT INTO files (workspace_id, id, name, declared_type, size, wrapped_dek, storage_key, uploaded_by, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(tx.ctx.workspace_id).bind(id).bind(&name).bind(declared).bind(size).bind(wrapped).bind(&key).bind(actor.membership).bind(self.clock.now())
            .execute(tx.conn()).await?;
        self.get(tx, id).await
    }

    /// Stores one chunk at `offset` (must equal what was received so far: resumable, never overlapping).
    pub async fn append(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        offset: i64,
        bytes: &[u8],
    ) -> Result<File, FileError> {
        let (state, size, received, chunks, key, by): (String, i64, i64, i32, String, Uuid) =
            sqlx::query_as("SELECT state, size, received, chunks, storage_key, uploaded_by FROM files WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(tx.conn()).await?.ok_or(FileError::NotFound)?;
        if by != actor.membership {
            return Err(FileError::NotFound);
        }
        if state != "uploading" {
            return Err(FileError::Conflict(format!("the upload is {state}")));
        }
        if offset != received {
            return Err(FileError::Conflict(format!("expected offset {received}")));
        }
        if bytes.is_empty() || bytes.len() > MAX_CHUNK {
            return Err(FileError::Invalid(format!("a chunk is 1 byte to {} MB", MAX_CHUNK >> 20)));
        }
        let len = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        if received + len > size {
            return Err(FileError::Invalid("more bytes than the declared size".into()));
        }
        let dek = self.dek(tx, id).await?;
        let sealed = aead_seal(&dek, &format!("files:{id}:{chunks}"), bytes).map_err(internal)?;
        self.store.put_chunk(&key, chunks, &sealed).await.map_err(internal)?;
        sqlx::query("UPDATE files SET received = received + $2, chunks = chunks + 1 WHERE id = $1")
            .bind(id)
            .bind(len)
            .execute(tx.conn())
            .await?;
        self.get(tx, id).await
    }

    /// Replaces a file's stored bytes (sealed again, chunk by chunk) and its recorded size.
    async fn rewrite(&self, tx: &mut WsTx<'_>, id: Uuid, bytes: &[u8]) -> Result<(), FileError> {
        let key: String = sqlx::query_scalar("SELECT storage_key FROM files WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
        let dek = self.dek(tx, id).await?;
        self.store.delete(&key).await.map_err(internal)?;
        let mut n = 0i32;
        for part in bytes.chunks(MAX_CHUNK) {
            let sealed = aead_seal(&dek, &format!("files:{id}:{n}"), part).map_err(internal)?;
            self.store.put_chunk(&key, n, &sealed).await.map_err(internal)?;
            n += 1;
        }
        let len = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        sqlx::query("UPDATE files SET size = $2, received = $2, chunks = $3 WHERE id = $1")
            .bind(id)
            .bind(len)
            .bind(n)
            .execute(tx.conn())
            .await?;
        Ok(())
    }

    /// The plaintext of a file (every chunk, in order).
    async fn read_all(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Vec<u8>, FileError> {
        let (chunks, key, size): (i32, String, i64) =
            sqlx::query_as("SELECT chunks, storage_key, size FROM files WHERE id = $1")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(FileError::NotFound)?;
        let dek = self.dek(tx, id).await?;
        let mut out = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        for n in 0..chunks {
            let sealed = self.store.get_chunk(&key, n).await.map_err(internal)?;
            out.extend_from_slice(&aead_open(&dek, &format!("files:{id}:{n}"), &sealed).map_err(internal)?);
        }
        Ok(out)
    }

    /// Finishes an upload: hash, sniff the type (the platform deny list always applies), scan.
    pub async fn complete(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> Result<File, FileError> {
        let f = self.get(tx, id).await?;
        if f.uploaded_by != actor.membership {
            return Err(FileError::NotFound);
        }
        if f.state != "uploading" {
            return Err(FileError::Conflict(format!("the upload is {}", f.state)));
        }
        if f.received != f.size {
            return Err(FileError::Conflict(format!("{} of {} bytes received", f.received, f.size)));
        }
        let mut data = self.read_all(tx, id).await?;
        let sniffed = sniff::sniff(&data[..data.len().min(4096)]);
        let now = self.clock.now();
        let (state, result) = if sniff::denied(sniffed) {
            ("rejected", format!("type not allowed: {sniffed}"))
        } else {
            match self.scanner.scan(&data).await {
                Ok(Verdict::Clean) => ("clean", format!("clean ({})", self.scanner.name())),
                Ok(Verdict::Infected(sig)) => ("quarantined", format!("infected: {sig}")),
                Err(e) => {
                    tracing::error!(error = %e, "malware scan failed");
                    ("scanning", format!("scan failed, retry later: {e}"))
                }
            }
        };
        // A clean JPEG loses its metadata (GPS, device, timestamps) before anyone can download it, when
        // `files.exif_strip` is on (BPMS-209). The original was scanned; the stripped bytes replace it.
        let mut metadata_removed = 0usize;
        if state == "clean"
            && sniffed == "image/jpeg"
            && self.exif_strip(tx).await?
            && let Some(stripped) = exif::strip_jpeg(&data).filter(|x| x.len() < data.len())
        {
            metadata_removed = data.len() - stripped.len();
            self.rewrite(tx, id, &stripped).await?;
            data = stripped;
        }
        let sha: Vec<u8> = Sha256::digest(&data).to_vec();
        sqlx::query("UPDATE files SET state = $2, scan_result = $3, sha256 = $4, sniffed_type = $5, completed_at = $6 WHERE id = $1")
            .bind(id).bind(state).bind(&result).bind(&sha).bind(sniffed).bind(now).execute(tx.conn()).await?;
        if state == "rejected" {
            let key: String = sqlx::query_scalar("SELECT storage_key FROM files WHERE id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
            self.store.delete(&key).await.map_err(internal)?;
        }
        let (action, outcome) = match state {
            "clean" => ("file.uploaded", Outcome::Allowed),
            "quarantined" => ("file.quarantined", Outcome::Denied),
            "rejected" => ("file.rejected", Outcome::Denied),
            _ => ("file.scan_failed", Outcome::Error),
        };
        self.audit(
            tx,
            Some(actor),
            action,
            id,
            json!({"name": f.name, "size": data.len(), "type": sniffed, "result": result, "metadata_removed": metadata_removed}),
            outcome,
        )
        .await?;
        self.get(tx, id).await
    }

    /// Plaintext of a clean file, for downloads.
    pub async fn content(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<(File, Vec<u8>), FileError> {
        let f = self.get(tx, id).await?;
        if f.state != "clean" {
            return Err(FileError::Conflict(format!("the file is {}", f.state)));
        }
        let data = self.read_all(tx, id).await?;
        Ok((f, data))
    }

    pub async fn record_download(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> Result<(), FileError> {
        self.touch(tx, id).await?;
        self.audit(tx, Some(actor), "file.downloaded", id, json!({}), Outcome::Allowed).await
    }

    /// The bytes to preview and the content type to serve: clean files of an allow-listed type only,
    /// with JPEG metadata removed when `files.exif_strip` is on. `Invalid` means "no preview".
    pub async fn preview(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
    ) -> Result<(File, &'static str, Vec<u8>), FileError> {
        let (f, data) = self.content(tx, id).await?;
        // The recorded type and a fresh sniff of the stored bytes must agree before anything is inline.
        let recorded = preview_kind(f.sniffed_type.as_deref(), &f.name);
        let fresh = preview_kind(Some(sniff::sniff(&data[..data.len().min(4096)])), &f.name);
        let Some((kind, ct)) = recorded.filter(|r| Some(*r) == fresh) else {
            return Err(FileError::Invalid("this file type has no preview".into()));
        };
        // An Office file must really be one: its central directory names the main part.
        if matches!(kind, "docx" | "xlsx") && sniff::ooxml(&data) != Some(ct) {
            return Err(FileError::Invalid("this file is not a valid Office document".into()));
        }
        if ct == "image/jpeg" && self.exif_strip(tx).await? {
            let stripped =
                exif::strip_jpeg(&data).ok_or_else(|| FileError::Invalid("the JPEG is malformed".into()))?;
            return Ok((f, ct, stripped));
        }
        Ok((f, ct, data))
    }

    async fn exif_strip(&self, tx: &mut WsTx<'_>) -> Result<bool, FileError> {
        Ok(self
            .settings
            .effective(tx, "files.exif_strip")
            .await
            .map_err(internal)?
            .value
            .as_bool()
            .unwrap_or(true))
    }

    pub async fn record_preview(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> Result<(), FileError> {
        self.touch(tx, id).await?;
        self.audit(tx, Some(actor), "file.previewed", id, json!({}), Outcome::Allowed).await
    }

    async fn touch(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<(), FileError> {
        sqlx::query("UPDATE files SET last_accessed_at = $2 WHERE id = $1")
            .bind(id)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        Ok(())
    }

    /// The record class a file follows (FR-F09, docs/14 §4), from what holds it: an audit export is
    /// `audit`, a document version past draft `governance_record`, a request `execution_record`, a
    /// draft `draft`; none when nothing
    /// holds it (orphan rules apply).
    pub async fn retention_class(tx: &mut WsTx<'_>, id: Uuid) -> Result<Option<&'static str>, FileError> {
        let set: Option<String> =
            sqlx::query_scalar("SELECT retention_class_override FROM files WHERE id = $1")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .flatten();
        if let Some(c) = set.and_then(|c| RETENTION_CLASSES.iter().find(|k| **k == c)) {
            return Ok(Some(c));
        }
        let rank: Option<i32> = sqlx::query_scalar(
            "SELECT max(CASE WHEN l.object_type = 'audit_export' THEN 4
                             WHEN v.lifecycle IN ('approved', 'scheduled', 'published', 'deprecated', 'retired') THEN 3
                             WHEN l.object_type = 'request' THEN 2
                             WHEN v.lifecycle IS NOT NULL THEN 1 END)
             FROM file_links l LEFT JOIN artifact_versions v ON l.object_type = 'artifact_version' AND v.workspace_id = l.workspace_id AND v.id = l.object_id
             WHERE l.file_id = $1",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        Ok(match rank {
            Some(4) => Some("audit"),
            Some(3) => Some("governance_record"),
            Some(2) => Some("execution_record"),
            Some(1) => Some("draft"),
            _ => None,
        })
    }

    /// Whether the file's retention class was set explicitly by an admin.
    pub async fn retention_overridden(tx: &mut WsTx<'_>, id: Uuid) -> Result<bool, FileError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT retention_class_override IS NOT NULL FROM files WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?
        .unwrap_or(false))
    }

    /// Sets (or with `None` clears) the retention class of several files, with a reason; one audit
    /// record per file (E2E-R1-12).
    pub async fn set_retention(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        ids: &[Uuid],
        class: Option<&str>,
        reason: &str,
    ) -> Result<usize, FileError> {
        if let Some(c) = class
            && !RETENTION_CLASSES.contains(&c)
        {
            return Err(FileError::Invalid(format!("unknown retention class {c}")));
        }
        if reason.trim().is_empty() {
            return Err(FileError::Invalid("a reason is required to change retention".into()));
        }
        let mut n = 0;
        for id in ids {
            let before = Self::retention_class(tx, *id).await?;
            let changed = sqlx::query(
                "UPDATE files SET retention_class_override = $2 WHERE id = $1 AND state <> 'deleted'",
            )
            .bind(id)
            .bind(class)
            .execute(tx.conn())
            .await?
            .rows_affected();
            if changed == 0 {
                return Err(FileError::NotFound);
            }
            n += 1;
            let mut ev = AuditEvent::new(
                Some(actor.subject_ref),
                ActorKind::User,
                Category::File,
                "file.retention_changed",
                Outcome::Allowed,
            )
            .object("file", *id);
            ev.reason = Some(reason.trim().to_owned());
            ev.ip = actor.ip.clone();
            ev.user_agent = actor.user_agent.clone();
            ev.diff = Some(json!({"retention_class": {"before": before, "after": class}}));
            append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        }
        Ok(n)
    }

    // ---- signed download links ----

    /// A download token for `file` in `workspace` for `membership`, valid for [`DOWNLOAD_TTL`].
    pub fn download_token(
        &self,
        workspace: Uuid,
        file: Uuid,
        membership: Uuid,
    ) -> Result<(String, OffsetDateTime), FileError> {
        self.link_token(LinkPurpose::Download, workspace, file, membership)
    }

    /// Verifies a download token: (workspace, file, membership) when the MAC matches and it has not expired.
    pub fn verify_token(&self, token: &str) -> Option<(Uuid, Uuid, Uuid)> {
        self.verify_link(LinkPurpose::Download, token)
    }

    /// A token for `file` in `workspace` for `membership` and `purpose`, valid for the purpose's TTL.
    pub fn link_token(
        &self,
        purpose: LinkPurpose,
        workspace: Uuid,
        file: Uuid,
        membership: Uuid,
    ) -> Result<(String, OffsetDateTime), FileError> {
        let exp = self.clock.now() + purpose.ttl();
        let mut body = Vec::with_capacity(56);
        body.extend_from_slice(workspace.as_bytes());
        body.extend_from_slice(file.as_bytes());
        body.extend_from_slice(membership.as_bytes());
        body.extend_from_slice(&exp.unix_timestamp().to_be_bytes());
        let m = mac(self.root.as_ref(), purpose.context(), &body).map_err(internal)?;
        body.extend_from_slice(&m);
        Ok((URL_SAFE_NO_PAD.encode(body), exp))
    }

    /// Verifies a token for `purpose`: (workspace, file, membership) when the MAC matches and it has
    /// not expired.
    pub fn verify_link(&self, purpose: LinkPurpose, token: &str) -> Option<(Uuid, Uuid, Uuid)> {
        let b = URL_SAFE_NO_PAD.decode(token).ok()?;
        if b.len() != 16 * 3 + 8 + 32 {
            return None;
        }
        let (body, m) = b.split_at(56);
        let expect = mac(self.root.as_ref(), purpose.context(), body).ok()?;
        if !mac_eq(&expect, m) {
            return None;
        }
        let exp = i64::from_be_bytes(body[48..56].try_into().ok()?);
        if self.clock.now().unix_timestamp() > exp {
            return None;
        }
        Some((
            Uuid::from_slice(&body[0..16]).ok()?,
            Uuid::from_slice(&body[16..32]).ok()?,
            Uuid::from_slice(&body[32..48]).ok()?,
        ))
    }

    // ---- links ----

    /// Links a clean file to an object (a draft version for documents).
    pub async fn link(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        object_type: &str,
        object_id: Uuid,
    ) -> Result<(), FileError> {
        let f = self.get(tx, id).await?;
        if f.state != "clean" {
            return Err(FileError::Conflict(format!(
                "only clean files can be attached (this one is {})",
                f.state
            )));
        }
        sqlx::query("INSERT INTO file_links (workspace_id, file_id, object_type, object_id, linked_by, linked_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING")
            .bind(tx.ctx.workspace_id).bind(id).bind(object_type).bind(object_id).bind(actor.membership).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "file.linked",
            id,
            json!({"object_type": object_type, "object_id": object_id}),
            Outcome::Allowed,
        )
        .await
    }

    /// Whether the file is held by an object that can no longer change (a version past draft, a
    /// request or a signing envelope: its files are part of the record).
    async fn held(tx: &mut WsTx<'_>, id: Uuid) -> Result<bool, FileError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM file_links l JOIN artifact_versions v ON v.workspace_id = l.workspace_id AND v.id = l.object_id
                            WHERE l.file_id = $1 AND l.object_type = 'artifact_version' AND v.lifecycle <> 'draft')
                 OR EXISTS (SELECT 1 FROM file_links l WHERE l.file_id = $1 AND l.object_type IN ('request', 'envelope'))",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?)
    }

    pub async fn unlink(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        object_type: &str,
        object_id: Uuid,
    ) -> Result<(), FileError> {
        let lifecycle: Option<String> =
            sqlx::query_scalar("SELECT lifecycle FROM artifact_versions WHERE id = $1")
                .bind(object_id)
                .fetch_optional(tx.conn())
                .await?;
        if object_type == "artifact_version" && lifecycle.as_deref().is_some_and(|l| l != "draft") {
            return Err(FileError::Conflict("attachments of a version past draft cannot change".into()));
        }
        let n =
            sqlx::query("DELETE FROM file_links WHERE file_id = $1 AND object_type = $2 AND object_id = $3")
                .bind(id)
                .bind(object_type)
                .bind(object_id)
                .execute(tx.conn())
                .await?
                .rows_affected();
        if n == 0 {
            return Err(FileError::NotFound);
        }
        self.audit(
            tx,
            Some(actor),
            "file.unlinked",
            id,
            json!({"object_type": object_type, "object_id": object_id}),
            Outcome::Allowed,
        )
        .await
    }

    /// Files linked to an object.
    pub async fn linked(
        &self,
        tx: &mut WsTx<'_>,
        object_type: &str,
        object_id: Uuid,
    ) -> Result<Vec<File>, FileError> {
        Ok(sqlx::query_as(&format!(
            "SELECT {} FROM files f JOIN file_links l ON l.workspace_id = f.workspace_id AND l.file_id = f.id
             WHERE l.object_type = $1 AND l.object_id = $2 AND f.state <> 'deleted' ORDER BY l.linked_at, f.id",
            COLS.split(", ").map(|c| format!("f.{c}")).collect::<Vec<_>>().join(", ")
        ))
        .bind(object_type)
        .bind(object_id)
        .fetch_all(tx.conn())
        .await?)
    }

    /// Objects a file is linked to.
    pub async fn links_of(&self, tx: &mut WsTx<'_>, id: Uuid) -> Result<Vec<(String, Uuid)>, FileError> {
        Ok(sqlx::query_as(
            "SELECT object_type, object_id FROM file_links WHERE file_id = $1 ORDER BY linked_at",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?)
    }

    /// Deletes a file unless a version past draft holds it.
    pub async fn delete(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> Result<(), FileError> {
        let f = self.get(tx, id).await?;
        if Self::held(tx, id).await? {
            return Err(FileError::Conflict("a published or approved version uses this file".into()));
        }
        let key: String = sqlx::query_scalar("SELECT storage_key FROM files WHERE id = $1")
            .bind(id)
            .fetch_one(tx.conn())
            .await?;
        sqlx::query("DELETE FROM file_links WHERE file_id = $1").bind(id).execute(tx.conn()).await?;
        sqlx::query("UPDATE files SET state = 'deleted', deleted_at = $2 WHERE id = $1")
            .bind(id)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        self.store.delete(&key).await.map_err(internal)?;
        self.audit(
            tx,
            Some(actor),
            "file.deleted",
            id,
            json!({"name": f.name, "size": f.size}),
            Outcome::Allowed,
        )
        .await
    }

    /// Every file of the workspace (newest first), optionally by state.
    pub async fn list(
        &self,
        tx: &mut WsTx<'_>,
        state: Option<&str>,
        uploaded_by: Option<Uuid>,
    ) -> Result<Vec<File>, FileError> {
        Ok(sqlx::query_as(&format!("SELECT {COLS} FROM files WHERE state <> 'deleted' AND ($1::text IS NULL OR state = $1) AND ($2::uuid IS NULL OR uploaded_by = $2) ORDER BY created_at DESC, id DESC LIMIT 1000"))
            .bind(state)
            .bind(uploaded_by)
            .fetch_all(tx.conn())
            .await?)
    }

    /// Completed files nothing links to, older than `files.orphan_purge_after` (FR-F07).
    pub async fn orphans(&self, tx: &mut WsTx<'_>) -> Result<Vec<File>, FileError> {
        let after = self
            .settings
            .effective(tx, "files.orphan_purge_after")
            .await
            .map_err(internal)?
            .value
            .as_i64()
            .unwrap_or(7 * 86_400);
        Ok(sqlx::query_as(&format!(
            "SELECT {COLS} FROM files f WHERE f.state IN ('clean', 'quarantined', 'rejected', 'scanning') AND f.completed_at < $1
               AND NOT EXISTS (SELECT 1 FROM file_links l WHERE l.workspace_id = f.workspace_id AND l.file_id = f.id)
               AND NOT EXISTS (SELECT 1 FROM workspace_settings w WHERE w.workspace_id = f.workspace_id AND w.key LIKE 'branding.%' AND w.value = to_jsonb(f.id::text))
             ORDER BY f.completed_at"
        ))
        .bind(self.clock.now() - Duration::seconds(after))
        .fetch_all(tx.conn())
        .await?)
    }

    /// Deletes every orphan. Returns how many.
    pub async fn purge_orphans(&self, tx: &mut WsTx<'_>, actor: &Actor) -> Result<usize, FileError> {
        let list = self.orphans(tx).await?;
        for f in &list {
            self.delete(tx, actor, f.id).await?;
        }
        let mut ev = AuditEvent::new(
            Some(actor.subject_ref),
            ActorKind::User,
            Category::File,
            "file.orphans_purged",
            Outcome::Allowed,
        );
        ev.ip = actor.ip.clone();
        ev.user_agent = actor.user_agent.clone();
        ev.diff = Some(json!({"purged": list.len()}));
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(list.len())
    }

    /// Storage usage: total, quota, by type and by uploader (FR-F07).
    pub async fn usage(&self, tx: &mut WsTx<'_>) -> Result<serde_json::Value, FileError> {
        let total = self.used_bytes(tx).await?;
        let quota: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT max_storage_bytes FROM workspace_quotas WHERE target_workspace_id = $1",
        )
        .bind(tx.ctx.workspace_id)
        .fetch_optional(tx.conn())
        .await?;
        let by_type: Vec<(Option<String>, i64, i64)> = sqlx::query_as("SELECT sniffed_type, count(*), coalesce(sum(size), 0)::bigint FROM files WHERE state NOT IN ('deleted', 'rejected') GROUP BY 1 ORDER BY 3 DESC").fetch_all(tx.conn()).await?;
        let by_owner: Vec<(Uuid, i64, i64)> = sqlx::query_as("SELECT uploaded_by, count(*), coalesce(sum(size), 0)::bigint FROM files WHERE state NOT IN ('deleted', 'rejected') GROUP BY 1 ORDER BY 3 DESC").fetch_all(tx.conn()).await?;
        let states: Vec<(String, i64)> =
            sqlx::query_as("SELECT state, count(*) FROM files WHERE state <> 'deleted' GROUP BY 1")
                .fetch_all(tx.conn())
                .await?;
        Ok(json!({
            "used_bytes": total,
            "quota_bytes": quota.flatten(),
            "by_type": by_type.iter().map(|(t, n, b)| json!({"type": t, "files": n, "bytes": b})).collect::<Vec<_>>(),
            "by_uploader": by_owner.iter().map(|(m, n, b)| json!({"membership_id": m, "files": n, "bytes": b})).collect::<Vec<_>>(),
            "states": states.into_iter().collect::<std::collections::HashMap<_, _>>(),
            "scanner": self.scanner.name(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_only_allow_listed_types() {
        assert_eq!(preview_kind(Some("image/png"), "a.png"), Some(("image", "image/png")));
        assert_eq!(preview_kind(Some("image/webp"), "a.webp"), Some(("image", "image/webp")));
        assert_eq!(preview_kind(Some("application/pdf"), "a.pdf"), Some(("pdf", "application/pdf")));
        assert_eq!(preview_kind(Some(sniff::DOCX), "a.docx"), Some(("docx", sniff::DOCX)));
        assert_eq!(preview_kind(Some(sniff::XLSX), "a.xlsx"), Some(("xlsx", sniff::XLSX)));
        assert_eq!(preview_kind(Some("text/plain"), "Rota.CSV"), Some(("csv", "text/csv; charset=utf-8")));
        for t in ["image/svg+xml", "text/html", "text/plain", "application/zip", "application/octet-stream"] {
            assert_eq!(preview_kind(Some(t), "a.txt"), None, "{t}");
        }
        assert_eq!(preview_kind(None, "a.csv"), None);
        // An SVG or HTML payload never sniffs as a previewable type, whatever it is named.
        assert_eq!(preview_kind(Some(sniff::sniff(b"<svg onload=alert(1)>")), "x.csv"), None);
        assert_eq!(preview_kind(Some(sniff::sniff(b"<html><script>x</script>")), "x.csv"), None);
        // A plain zip is not previewable; a zip that only looks like a document sniffs as one but
        // fails the central-directory check `preview` applies.
        let plain = sniff::tests::zip_of(&[("notes.txt", "hi")]);
        assert_eq!(preview_kind(Some(sniff::sniff(&plain)), "notes.docx"), None);
        let fake = sniff::tests::zip_of(&[("word/readme.txt", "x")]);
        assert_eq!(sniff::ooxml(&fake), None);
    }

    #[test]
    fn link_purposes_are_distinct_and_previews_are_shorter() {
        assert_ne!(LinkPurpose::Download.context(), LinkPurpose::Preview.context());
        assert!(LinkPurpose::Preview.ttl() < LinkPurpose::Download.ttl());
    }
}
