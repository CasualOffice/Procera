//! Effective-settings cache (CFG-004, docs/20 §5, ADR-0019). Advisory only: a miss, an error or no
//! cache at all reads the database, and enforcement paths (sign-in, step-up, every write) always
//! read the database inside their transaction.
//!
//! Invalidation uses generations so it needs no key scans and cannot race a reader into keeping a
//! stale entry: a reader takes the workspace and floor generations *before* reading the database
//! and stores its snapshot under them; a writer installs a new generation *after* commit (and the
//! `settings.changed.v1` consumer installs another), so later readers never look at the old entry.
//! An absent generation reads as `0`; entries live far shorter than generations, so an entry
//! written under `0` has expired before a generation can lapse back to `0`.

use std::time::Duration;

use procera_cache::{Cache, CacheKey};
use procera_db::WsTx;
use uuid::Uuid;

use crate::{SettingsError, Snapshot};

pub use crate::CHANGED_TOPIC as TOPIC;

const SNAPSHOT_TTL: Duration = Duration::from_secs(60);
const GENERATION_TTL: Duration = Duration::from_secs(86_400);

/// How a read was served (reported to clients in `x-settings-cache`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Hit,
    Miss,
    /// No shared cache configured: read from the database.
    Off,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsCache {
    cache: Cache,
}

fn ws_generation(ws: Uuid) -> CacheKey {
    CacheKey::workspace(ws, "settings:gen")
}

fn floor_generation() -> CacheKey {
    CacheKey::system("settings:floors:gen")
}

impl SettingsCache {
    /// Caches on any backend; use [`SettingsCache::shared`] in the app.
    pub fn new(cache: Cache) -> Self {
        Self { cache }
    }

    /// Only a shared (Valkey) cache is used for settings: post-commit invalidation then reaches
    /// every node. With the in-process backend there is no settings cache (reads go to the database).
    pub fn shared(cache: &Cache) -> Option<Self> {
        matches!(cache, Cache::Remote(_)).then(|| Self::new(cache.clone()))
    }

    async fn generation(&self, key: &CacheKey) -> String {
        self.cache.get(key).await.and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| "0".into())
    }

    /// The transaction's workspace snapshot, from the cache when it is current.
    pub async fn snapshot(&self, tx: &mut WsTx<'_>) -> Result<(Snapshot, Outcome), SettingsError> {
        let ws = tx.ctx.workspace_id;
        let (g_ws, g_floor) =
            (self.generation(&ws_generation(ws)).await, self.generation(&floor_generation()).await);
        let key = CacheKey::workspace(ws, &format!("settings:snap:{g_ws}:{g_floor}"));
        if let Some(s) = self.cache.get(&key).await.and_then(|b| serde_json::from_slice(&b).ok()) {
            return Ok((s, Outcome::Hit));
        }
        let s = Snapshot::load(tx).await?;
        if let Ok(bytes) = serde_json::to_vec(&s) {
            self.cache.set(&key, &bytes, SNAPSHOT_TTL).await;
        }
        Ok((s, Outcome::Miss))
    }

    /// Call after the change commits (and from the outbox consumer).
    pub async fn invalidate_workspace(&self, ws: Uuid) {
        self.cache.set(&ws_generation(ws), Uuid::new_v4().to_string().as_bytes(), GENERATION_TTL).await;
    }

    /// A platform floor changed: every workspace's snapshot is stale.
    pub async fn invalidate_floors(&self) {
        self.cache.set(&floor_generation(), Uuid::new_v4().to_string().as_bytes(), GENERATION_TTL).await;
    }
}

/// Reads through `cache` when there is one, else from the database.
pub async fn snapshot(
    cache: Option<&SettingsCache>,
    tx: &mut WsTx<'_>,
) -> Result<(Snapshot, Outcome), SettingsError> {
    match cache {
        Some(c) => c.snapshot(tx).await,
        None => Ok((Snapshot::load(tx).await?, Outcome::Off)),
    }
}
