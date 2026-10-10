//! Cache port (ADR-0019, docs/04 §1 `infra/cache`).
//!
//! Rules enforced by this API:
//! - every key is namespaced: `ws:{workspace_id}:…` or `sys:…` (no cross-workspace collisions);
//! - the cache is **advisory**: `get` failures degrade to a miss, writes are best-effort;
//! - rate limiting falls back to an in-process limiter if the server is unreachable, so a cache
//!   outage costs precision, never availability or correctness.

mod memory;
mod remote;

use std::time::Duration;

pub use memory::MemoryCache;
pub use remote::RemoteCache;
use uuid::Uuid;

/// A namespaced cache key. Construct only via [`CacheKey::workspace`] or [`CacheKey::system`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    pub fn workspace(workspace_id: Uuid, name: &str) -> Self {
        Self(format!("ws:{workspace_id}:{name}"))
    }
    pub fn system(name: &str) -> Self {
        Self(format!("sys:{name}"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache backend error: {0}")]
    Backend(String),
}

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateDecision {
    pub allowed: bool,
    pub remaining: u32,
    /// Time until the current window resets.
    pub reset_after: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    /// Backend unreachable; operating on in-process fallbacks (docs/03 §8 degraded modes).
    Degraded,
}

/// The cache, selected at startup. Enum dispatch keeps it object-safe without boxing.
#[derive(Debug, Clone)]
pub enum Cache {
    Memory(MemoryCache),
    Remote(RemoteCache),
}

impl Cache {
    /// `None`, empty or `memory` → in-process cache; otherwise a `redis://` / `rediss://` URL (Valkey or Redis).
    pub async fn from_url(url: Option<&str>) -> Result<Self, CacheError> {
        match url.map(str::trim) {
            None | Some("") | Some("memory") => Ok(Self::Memory(MemoryCache::default())),
            Some(u) => Ok(Self::Remote(RemoteCache::connect(u).await?)),
        }
    }

    /// Best-effort read: backend errors are logged and treated as a miss.
    pub async fn get(&self, key: &CacheKey) -> Option<Vec<u8>> {
        match self {
            Self::Memory(c) => c.get(key),
            Self::Remote(c) => c.get(key).await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "cache get failed; treating as miss");
                None
            }),
        }
    }

    /// Best-effort write with a mandatory TTL (nothing is cached forever).
    pub async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) {
        match self {
            Self::Memory(c) => c.set(key, value, ttl),
            Self::Remote(c) => {
                if let Err(e) = c.set(key, value, ttl).await {
                    tracing::warn!(error = %e, "cache set failed; ignored");
                }
            }
        }
    }

    pub async fn delete(&self, key: &CacheKey) {
        match self {
            Self::Memory(c) => c.delete(key),
            Self::Remote(c) => {
                if let Err(e) = c.delete(key).await {
                    tracing::warn!(error = %e, "cache delete failed; entry will expire by TTL");
                }
            }
        }
    }

    /// Fixed-window rate limit: at most `limit` hits per `window` for `key`.
    pub async fn rate_limit(&self, key: &CacheKey, limit: u32, window: Duration) -> RateDecision {
        match self {
            Self::Memory(c) => c.rate_limit(key, limit, window),
            Self::Remote(c) => c.rate_limit(key, limit, window).await,
        }
    }

    pub async fn health(&self) -> Health {
        match self {
            Self::Memory(_) => Health::Ok,
            Self::Remote(c) => c.health().await,
        }
    }

    pub fn backend(&self) -> &'static str {
        match self {
            Self::Memory(_) => "memory",
            Self::Remote(_) => "valkey",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_namespaced() {
        let ws = Uuid::nil();
        assert_eq!(
            CacheKey::workspace(ws, "policy").as_str(),
            "ws:00000000-0000-0000-0000-000000000000:policy"
        );
        assert_eq!(CacheKey::system("health").as_str(), "sys:health");
    }
}
