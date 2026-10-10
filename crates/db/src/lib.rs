//! PostgreSQL access layer (BPMS-025).
//!
//! The only way to touch workspace data is through a [`WsTx`]: a transaction whose first statement
//! sets `app.workspace_id` transaction-locally, so forced RLS scopes every query that follows
//! (docs/03 §7, docs/24 §3.2). Session-level settings are never used (SP-09 finding A-2).

pub mod idempotency;
pub mod outbox;
#[cfg(feature = "test-support")]
pub mod test_support;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// The system plane's own audit chain and settings live under the nil workspace id.
pub const SYSTEM_WORKSPACE: Uuid = Uuid::nil();

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("row-level security guard failed: workspace tables without forced RLS: {0:?}")]
    RlsGuard(Vec<String>),
}

/// Runs pending migrations. Must be called with the **migrator** role (never the app role).
pub async fn migrate(migrator_url: &str) -> Result<(), DbError> {
    let pool = PgPoolOptions::new().max_connections(1).connect(migrator_url).await?;
    MIGRATOR.run(&pool).await?;
    pool.close().await;
    Ok(())
}

/// Workspace context carried by every repository call (docs/04 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WsCtx {
    pub workspace_id: Uuid,
}

/// Runtime database handle (connects as `procera_app`).
#[derive(Debug, Clone)]
pub struct Db {
    pool: PgPool,
}

/// A transaction scoped to one workspace by RLS.
#[derive(Debug)]
pub struct WsTx<'c> {
    pub ctx: WsCtx,
    tx: Transaction<'c, Postgres>,
}

impl Db {
    pub async fn connect(app_url: &str, max_connections: u32) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new().max_connections(max_connections).connect(app_url).await?;
        Ok(Self { pool })
    }

    /// Fails if any workspace-scoped table lacks forced RLS + the isolation policy. Run at startup.
    pub async fn assert_rls_guard(&self) -> Result<(), DbError> {
        let bad: Vec<String> =
            sqlx::query_scalar("SELECT table_name FROM procera_rls_violations").fetch_all(&self.pool).await?;
        if bad.is_empty() { Ok(()) } else { Err(DbError::RlsGuard(bad)) }
    }

    /// Begins a transaction scoped to `ctx.workspace_id`.
    pub async fn begin_ws(&self, ctx: WsCtx) -> Result<WsTx<'static>, DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.workspace_id', $1, true)")
            .bind(ctx.workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(WsTx { ctx, tx })
    }

    /// Begins a system-plane transaction (the nil workspace).
    pub async fn begin_system(&self) -> Result<WsTx<'static>, DbError> {
        self.begin_ws(WsCtx { workspace_id: SYSTEM_WORKSPACE }).await
    }

    /// Workspaces with unpublished outbox rows (ids only; SECURITY DEFINER discovery function).
    pub async fn outbox_pending_workspaces(&self, max: i32) -> Result<Vec<Uuid>, DbError> {
        Ok(sqlx::query_scalar("SELECT workspace_id FROM discovery.outbox_pending_workspaces($1)")
            .bind(max)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Ensures audit partitions exist for the coming months (idempotent).
    pub async fn ensure_audit_partitions(&self, months_ahead: i32) -> Result<i32, DbError> {
        Ok(sqlx::query_scalar("SELECT procera_ensure_audit_partitions($1)")
            .bind(months_ahead)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Liveness probe for /readyz.
    pub async fn ping(&self) -> bool {
        sqlx::query("SELECT 1").execute(&self.pool).await.is_ok()
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl<'c> WsTx<'c> {
    /// The underlying connection, for repository queries.
    pub fn conn(&mut self) -> &mut sqlx::PgConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> Result<(), DbError> {
        Ok(self.tx.commit().await?)
    }

    pub async fn rollback(self) -> Result<(), DbError> {
        Ok(self.tx.rollback().await?)
    }
}
