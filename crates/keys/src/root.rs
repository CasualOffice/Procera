//! Root key rotation (BPMS-030; docs/06 §4): `procera rotate-master-key`.
//!
//! Values sealed directly under the root key (with [`procera_crypto::seal`]) are re-sealed under a
//! new root key in **one** transaction, and each new ciphertext is opened with the new key before
//! the transaction commits. Envelopes (version byte 2) are wrapped by a workspace KEK, not by the
//! root key, so they stay as they are: re-sealing the KEK rows is what keeps them readable.
//!
//! | Table | Column | Purpose | AAD |
//! |---|---|---|---|
//! | `workspace_keys` | `wrapped` | `workspace-kek` | `workspace_keys:<ws>:<version>` |
//! | `user_credentials` (TOTP) | `secret` | `secrets` | `user_credentials:<id>:totp` |
//! | `invitations` | `token_sealed` | `secrets` | `invitations:<id>` |
//! | `email_messages` (pre-envelope bodies) | `body_sealed` | `secrets` | `email_messages:<id>` |
//!
//! Keys *derived* from the root key are not stored, so they simply change: access-token signing
//! (`jwt-access-v1`) and signed file links (`file-download`, `file-preview`).

use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_crypto::{KeyProvider, is_envelope, open, seal};
use procera_db::{Db, SYSTEM_WORKSPACE, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::{KeyError, PURPOSE};

const SECRETS: &str = "secrets";

/// What a root key rotation re-sealed.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct RootRotation {
    /// Workspaces (including the system plane) whose values were visited.
    pub workspaces: usize,
    /// Workspace key-encryption keys (every live version).
    pub workspace_keys: usize,
    /// Authenticator-app (TOTP) secrets.
    pub totp_secrets: usize,
    /// Invitation tokens kept for the mailer.
    pub invitation_tokens: usize,
    /// Email bodies stored before workspace envelopes existed.
    pub legacy_email_bodies: usize,
}

impl RootRotation {
    pub fn total(&self) -> usize {
        self.workspace_keys + self.totp_secrets + self.invitation_tokens + self.legacy_email_bodies
    }
}

/// Opens `sealed` with `current`, seals it again with `new`, and proves the new value opens with
/// `new` to the same plaintext. Errors name the table and row, never key material.
fn reseal(
    current: &dyn KeyProvider,
    new: &dyn KeyProvider,
    purpose: &str,
    aad: &str,
    sealed: &[u8],
    table: &'static str,
) -> Result<Vec<u8>, KeyError> {
    let pt = open(current, purpose, aad, sealed)
        .map_err(|_| KeyError::RootMismatch { table, row: aad.to_owned() })?;
    let re = seal(new, purpose, aad, &pt)?;
    let back =
        open(new, purpose, aad, &re).map_err(|_| KeyError::RootVerify { table, row: aad.to_owned() })?;
    if back.as_slice() != pt.as_slice() {
        return Err(KeyError::RootVerify { table, row: aad.to_owned() });
    }
    Ok(re)
}

/// `true` when both providers derive the same keys (the "new" key is the current one).
fn same_root(a: &dyn KeyProvider, b: &dyn KeyProvider) -> Result<bool, KeyError> {
    let probe = "procera:rotation-check";
    Ok(a.key(probe)?.as_ref() == b.key(probe)?.as_ref())
}

/// Re-seals every root-sealed value from `current` to `new` in one transaction (see the module
/// table), verifies each one with `new`, appends `key.root_rotated` to the system audit chain and
/// commits. Nothing changes if any value fails to open with `current` or to verify with `new`.
pub async fn rotate_root(
    db: &Db,
    current: &dyn KeyProvider,
    new: &dyn KeyProvider,
    clock: &dyn Clock,
    ids: &dyn IdGen,
) -> Result<RootRotation, KeyError> {
    if same_root(current, new)? {
        return Err(KeyError::SameRootKey);
    }
    let mut out = RootRotation::default();
    let mut tx = db.begin_system().await?;
    let mut workspaces: Vec<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM discovery.root_sealed_workspaces()")
            .fetch_all(tx.conn())
            .await?;
    if !workspaces.contains(&SYSTEM_WORKSPACE) {
        workspaces.push(SYSTEM_WORKSPACE);
    }
    for ws in &workspaces {
        tx.rescope(WsCtx { workspace_id: *ws }).await?;
        reseal_workspace(&mut tx, current, new, &mut out).await?;
    }
    out.workspaces = workspaces.len();

    // Users are global (no workspace_id, no RLS): their TOTP secrets are visited once.
    let rows: Vec<(Uuid, Vec<u8>)> =
        sqlx::query_as("SELECT id, secret FROM user_credentials WHERE kind = 'totp' ORDER BY id FOR UPDATE")
            .fetch_all(tx.conn())
            .await?;
    for (id, sealed) in rows {
        let re = reseal(
            current,
            new,
            SECRETS,
            &format!("user_credentials:{id}:totp"),
            &sealed,
            "user_credentials",
        )?;
        sqlx::query("UPDATE user_credentials SET secret = $2 WHERE id = $1")
            .bind(id)
            .bind(re)
            .execute(tx.conn())
            .await?;
        out.totp_secrets += 1;
    }

    tx.rescope(WsCtx { workspace_id: SYSTEM_WORKSPACE }).await?;
    let mut ev =
        AuditEvent::new(None, ActorKind::System, Category::Config, "key.root_rotated", Outcome::Allowed);
    ev.diff = Some(json!(out));
    append(&mut tx, clock, ids, &ev).await?;
    tx.commit().await?;
    Ok(out)
}

async fn reseal_workspace(
    tx: &mut WsTx<'_>,
    current: &dyn KeyProvider,
    new: &dyn KeyProvider,
    out: &mut RootRotation,
) -> Result<(), KeyError> {
    let ws = tx.ctx.workspace_id;
    let keks: Vec<(i32, Vec<u8>)> = sqlx::query_as(
        "SELECT version, wrapped FROM workspace_keys WHERE wrapped IS NOT NULL ORDER BY version FOR UPDATE",
    )
    .fetch_all(tx.conn())
    .await?;
    for (version, sealed) in keks {
        let re = reseal(
            current,
            new,
            PURPOSE,
            &format!("workspace_keys:{ws}:{version}"),
            &sealed,
            "workspace_keys",
        )?;
        sqlx::query("UPDATE workspace_keys SET wrapped = $2 WHERE version = $1")
            .bind(version)
            .bind(re)
            .execute(tx.conn())
            .await?;
        out.workspace_keys += 1;
    }

    let invitations: Vec<(Uuid, Vec<u8>)> =
        sqlx::query_as("SELECT id, token_sealed FROM invitations ORDER BY id FOR UPDATE")
            .fetch_all(tx.conn())
            .await?;
    for (id, sealed) in invitations.into_iter().filter(|(_, s)| !is_envelope(s)) {
        let re = reseal(current, new, SECRETS, &format!("invitations:{id}"), &sealed, "invitations")?;
        sqlx::query("UPDATE invitations SET token_sealed = $2 WHERE id = $1")
            .bind(id)
            .bind(re)
            .execute(tx.conn())
            .await?;
        out.invitation_tokens += 1;
    }

    // Only bodies from before workspace envelopes (BPMS-030) are root-sealed.
    let bodies: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT id, body_sealed FROM email_messages WHERE length(body_sealed) > 0 AND get_byte(body_sealed, 0) <> 2 ORDER BY id FOR UPDATE",
    )
    .fetch_all(tx.conn())
    .await?;
    for (id, sealed) in bodies {
        let re = reseal(current, new, SECRETS, &format!("email_messages:{id}"), &sealed, "email_messages")?;
        sqlx::query("UPDATE email_messages SET body_sealed = $2 WHERE id = $1")
            .bind(id)
            .bind(re)
            .execute(tx.conn())
            .await?;
        out.legacy_email_bodies += 1;
    }
    Ok(())
}
