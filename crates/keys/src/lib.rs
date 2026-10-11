//! Workspace key hierarchy (BPMS-030; docs/04 §8, docs/06 §4).
//!
//! ```text
//! root key (KeyProvider: sealed local key today; KMS / OpenBao later)
//!  └── workspace KEK, versioned (workspace_keys.wrapped = root-sealed, AAD workspace_keys:<ws>:<v>)
//!       └── DEK per value (procera_crypto::envelope_*), wrapped by the KEK version it names
//! ```
//! - The first use of a workspace creates KEK v1. Rotation retires the active version and creates the
//!   next one; the rewrap job then rewraps every registered envelope column to the active version, and a
//!   retired KEK that no value references any more is destroyed.
//! - Crypto-shredding destroys every KEK of the workspace: its envelopes can no longer be opened.

use std::sync::Arc;

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{
    CryptoError, KeyProvider, envelope_open, envelope_rewrap, envelope_seal, envelope_version, open,
    random_bytes, seal,
};
use procera_db::{Db, DbError, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::Zeroizing;

const PURPOSE: &str = "workspace-kek";

/// Envelope-encrypted columns, so rotation can rewrap them: (table, id column, value column, AAD prefix).
/// The AAD of a value is `<prefix>:<id>`.
pub const REGISTRY: &[(&str, &str, &str, &str)] =
    &[("email_messages", "id", "body_sealed", "email_messages"), ("files", "id", "wrapped_dek", "files")];

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("the workspace's keys were destroyed")]
    Destroyed,
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("internal error: {0}")]
    Internal(String),
}
impl From<sqlx::Error> for KeyError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<DbError> for KeyError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for KeyError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyVersion {
    pub version: i32,
    pub state: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub retired_at: Option<OffsetDateTime>,
    /// Values still wrapped with this version.
    pub values: i64,
}

#[derive(Clone)]
pub struct Keys {
    db: Db,
    root: Arc<dyn KeyProvider>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys")
    }
}

impl Keys {
    pub fn new(db: Db, root: Arc<dyn KeyProvider>, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { db, root, clock, ids }
    }

    fn aad(ws: Uuid, version: i32) -> String {
        format!("workspace_keys:{ws}:{version}")
    }

    async fn insert_version(&self, tx: &mut WsTx<'_>, version: i32) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        let kek: Zeroizing<[u8; 32]> = Zeroizing::new(random_bytes());
        let wrapped =
            seal(self.root.as_ref(), PURPOSE, &Self::aad(tx.ctx.workspace_id, version), kek.as_ref())?;
        sqlx::query("INSERT INTO workspace_keys (workspace_id, version, wrapped, state, created_at) VALUES ($1, $2, $3, 'active', $4)")
            .bind(tx.ctx.workspace_id)
            .bind(version)
            .bind(wrapped)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        Ok(kek)
    }

    /// The KEK of `version`.
    pub async fn kek(&self, tx: &mut WsTx<'_>, version: i32) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        let row: Option<Option<Vec<u8>>> =
            sqlx::query_scalar("SELECT wrapped FROM workspace_keys WHERE version = $1")
                .bind(version)
                .fetch_optional(tx.conn())
                .await?;
        let wrapped = row.ok_or(KeyError::Crypto(CryptoError::Open))?.ok_or(KeyError::Destroyed)?;
        let pt = open(self.root.as_ref(), PURPOSE, &Self::aad(tx.ctx.workspace_id, version), &wrapped)?;
        let mut k = Zeroizing::new([0u8; 32]);
        if pt.len() != 32 {
            return Err(KeyError::Crypto(CryptoError::Open));
        }
        k.copy_from_slice(&pt);
        Ok(k)
    }

    /// The active KEK (created as v1 on first use, unless the workspace was shredded).
    pub async fn active(&self, tx: &mut WsTx<'_>) -> Result<(i32, Zeroizing<[u8; 32]>), KeyError> {
        let v: Option<i32> = sqlx::query_scalar("SELECT version FROM workspace_keys WHERE state = 'active'")
            .fetch_optional(tx.conn())
            .await?;
        if let Some(v) = v {
            return Ok((v, self.kek(tx, v).await?));
        }
        let destroyed: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workspace_keys WHERE state = 'destroyed')")
                .fetch_one(tx.conn())
                .await?;
        if destroyed {
            return Err(KeyError::Destroyed);
        }
        let k = self.insert_version(tx, 1).await?;
        Ok((1, k))
    }

    /// Envelope-seals a value for the current workspace under its active KEK.
    pub async fn seal(
        &self,
        tx: &mut WsTx<'_>,
        context: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, KeyError> {
        let (v, k) = self.active(tx).await?;
        Ok(envelope_seal(&k, u32::try_from(v).unwrap_or(1), context, plaintext)?)
    }

    /// Opens an envelope sealed for the current workspace.
    pub async fn open(
        &self,
        tx: &mut WsTx<'_>,
        context: &str,
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, KeyError> {
        let v = envelope_version(sealed).ok_or(KeyError::Crypto(CryptoError::Open))?;
        let k = self.kek(tx, i32::try_from(v).unwrap_or(i32::MAX)).await?;
        Ok(envelope_open(|_| Ok(k), context, sealed)?)
    }

    /// Retires the active KEK and creates the next version. Values are rewrapped by [`Keys::rewrap_due`].
    /// `client` is the caller's (ip, user agent) for the audit record.
    pub async fn rotate(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<Uuid>,
        reason: Option<&str>,
        client: (Option<String>, Option<String>),
    ) -> Result<i32, KeyError> {
        let (current, _) = self.active(tx).await?;
        sqlx::query("UPDATE workspace_keys SET state = 'retired', retired_at = $2 WHERE version = $1")
            .bind(current)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?;
        let next = current + 1;
        self.insert_version(tx, next).await?;
        let mut ev = AuditEvent::new(
            actor,
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Config,
            "key.rotated",
            Outcome::Allowed,
        );
        ev.diff = Some(json!({"from_version": current, "to_version": next}));
        ev.reason = reason.map(str::to_owned);
        (ev.ip, ev.user_agent) = client;
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(next)
    }

    /// Versions with how many values each still wraps.
    pub async fn status(&self, tx: &mut WsTx<'_>) -> Result<Vec<KeyVersion>, KeyError> {
        let rows: Vec<(i32, String, OffsetDateTime, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT version, state, created_at, retired_at FROM workspace_keys ORDER BY version DESC",
        )
        .fetch_all(tx.conn())
        .await?;
        let mut counts = std::collections::HashMap::<i32, i64>::new();
        for (table, _id, col, _) in REGISTRY {
            let per: Vec<(i32, i64)> = sqlx::query_as(&format!(
                "SELECT get_byte({col}, 1) << 24 | get_byte({col}, 2) << 16 | get_byte({col}, 3) << 8 | get_byte({col}, 4), count(*)
                 FROM {table} WHERE length({col}) > 5 AND get_byte({col}, 0) = 2 GROUP BY 1"
            ))
            .fetch_all(tx.conn())
            .await?;
            for (v, n) in per {
                *counts.entry(v).or_default() += n;
            }
        }
        Ok(rows
            .into_iter()
            .map(|(version, state, created_at, retired_at)| KeyVersion {
                values: counts.get(&version).copied().unwrap_or(0),
                version,
                state,
                created_at,
                retired_at,
            })
            .collect())
    }

    /// Rewraps up to `batch` values per registered column from retired versions to the active one,
    /// in every workspace with retired keys; destroys retired versions no value references. Returns
    /// how many values were rewrapped.
    pub async fn rewrap_due(&self, batch: i64) -> Result<usize, KeyError> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.rewrap_pending_workspaces(1000)")
                .fetch_all(self.db.pool())
                .await?;
        let mut n = 0;
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await?;
            let (active, new_kek) = self.active(&mut tx).await?;
            for (table, id_col, col, _aad_prefix) in REGISTRY {
                let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(&format!(
                    "SELECT {id_col}, {col} FROM {table}
                     WHERE length({col}) > 5 AND get_byte({col}, 0) = 2
                       AND (get_byte({col}, 1) << 24 | get_byte({col}, 2) << 16 | get_byte({col}, 3) << 8 | get_byte({col}, 4)) <> $1
                     LIMIT $2 FOR UPDATE SKIP LOCKED"
                ))
                .bind(active)
                .bind(batch)
                .fetch_all(tx.conn())
                .await?;
                for (id, sealed) in rows {
                    let Some(v) = envelope_version(&sealed) else { continue };
                    let old = self.kek(&mut tx, i32::try_from(v).unwrap_or(i32::MAX)).await?;
                    let re = envelope_rewrap(&sealed, &old, &new_kek, u32::try_from(active).unwrap_or(1))?;
                    // The AAD binds the data, not the wrapping: the value still opens under `<prefix>:<id>`.
                    sqlx::query(&format!("UPDATE {table} SET {col} = $2 WHERE {id_col} = $1"))
                        .bind(id)
                        .bind(re)
                        .execute(tx.conn())
                        .await?;
                    n += 1;
                }
            }
            // Retired versions nothing references any more are destroyed.
            for k in self.status(&mut tx).await? {
                if k.state == "retired" && k.values == 0 {
                    sqlx::query(
                        "UPDATE workspace_keys SET state = 'destroyed', wrapped = NULL WHERE version = $1",
                    )
                    .bind(k.version)
                    .execute(tx.conn())
                    .await?;
                }
            }
            tx.commit().await?;
        }
        Ok(n)
    }

    /// Crypto-shreds the workspace: every KEK is destroyed, so no envelope of it can be opened again.
    pub async fn shred(&self, tx: &mut WsTx<'_>, actor: Option<Uuid>, reason: &str) -> Result<(), KeyError> {
        sqlx::query("UPDATE workspace_keys SET state = 'destroyed', wrapped = NULL, retired_at = coalesce(retired_at, $1)").bind(self.clock.now()).execute(tx.conn()).await?;
        let mut ev = AuditEvent::new(
            actor,
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Config,
            "key.shredded",
            Outcome::Allowed,
        );
        ev.reason = Some(reason.to_owned());
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }
}
