//! Valkey / Redis-protocol backend with an in-process fallback for rate limiting.

use std::time::Duration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::{AsyncCommands, Script};

use crate::{CacheError, CacheKey, Health, MemoryCache, RateDecision};

/// INCR + set expiry on first hit, atomically; returns {count, pttl}.
const RATE_LIMIT_LUA: &str = r"
local c = redis.call('INCR', KEYS[1])
if c == 1 then redis.call('PEXPIRE', KEYS[1], ARGV[1]) end
local ttl = redis.call('PTTL', KEYS[1])
return {c, ttl}
";

#[derive(Clone)]
pub struct RemoteCache {
    conn: ConnectionManager,
    fallback: MemoryCache,
    script: Script,
}

impl std::fmt::Debug for RemoteCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteCache").finish_non_exhaustive()
    }
}

fn err(e: redis::RedisError) -> CacheError {
    CacheError::Backend(e.to_string())
}

impl RemoteCache {
    pub async fn connect(url: &str) -> Result<Self, CacheError> {
        let client = redis::Client::open(url).map_err(err)?;
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(2)))
            .set_response_timeout(Some(Duration::from_secs(1)))
            .set_number_of_retries(1);
        let conn = ConnectionManager::new_with_config(client, config).await.map_err(err)?;
        Ok(Self { conn, fallback: MemoryCache::default(), script: Script::new(RATE_LIMIT_LUA) })
    }

    pub async fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>, CacheError> {
        self.conn.clone().get(key.as_str()).await.map_err(err)
    }

    pub async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) -> Result<(), CacheError> {
        let ms = u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX).max(1);
        self.conn.clone().pset_ex::<_, _, ()>(key.as_str(), value, ms).await.map_err(err)
    }

    pub async fn delete(&self, key: &CacheKey) -> Result<(), CacheError> {
        self.conn.clone().del::<_, ()>(key.as_str()).await.map_err(err)
    }

    pub async fn rate_limit(&self, key: &CacheKey, limit: u32, window: Duration) -> RateDecision {
        let ms = u64::try_from(window.as_millis()).unwrap_or(u64::MAX).max(1);
        let rl_key = format!("{}:rl", key.as_str());
        let res: Result<(i64, i64), _> =
            self.script.key(&rl_key).arg(ms).invoke_async(&mut self.conn.clone()).await;
        match res {
            Ok((count, pttl)) => {
                let count = u32::try_from(count.max(0)).unwrap_or(u32::MAX);
                RateDecision {
                    allowed: count <= limit,
                    remaining: limit.saturating_sub(count),
                    reset_after: Duration::from_millis(u64::try_from(pttl.max(0)).unwrap_or(0)),
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "rate limit backend unavailable; using in-process limiter");
                self.fallback.rate_limit(key, limit, window)
            }
        }
    }

    pub async fn health(&self) -> Health {
        let pong: Result<String, _> = redis::cmd("PING").query_async(&mut self.conn.clone()).await;
        if pong.is_ok() { Health::Ok } else { Health::Degraded }
    }
}
