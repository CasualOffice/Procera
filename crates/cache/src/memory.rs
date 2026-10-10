//! In-process cache: the Trial-topology default and the fallback when the server is down.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use crate::{CacheKey, RateDecision};

const MAX_ENTRIES: usize = 50_000;

#[derive(Debug, Default, Clone)]
pub struct MemoryCache {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    values: HashMap<String, (Vec<u8>, Instant)>,
    windows: HashMap<String, (u32, Instant)>,
}

impl MemoryCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self, key: &CacheKey) -> Option<Vec<u8>> {
        let mut g = self.lock();
        match g.values.get(key.as_str()) {
            Some((v, exp)) if *exp > Instant::now() => Some(v.clone()),
            Some(_) => {
                g.values.remove(key.as_str());
                None
            }
            None => None,
        }
    }

    pub fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) {
        let mut g = self.lock();
        let now = Instant::now();
        if g.values.len() >= MAX_ENTRIES {
            g.values.retain(|_, (_, exp)| *exp > now); // drop expired first
            if g.values.len() >= MAX_ENTRIES {
                g.values.clear(); // bounded memory beats perfect hit rate
            }
        }
        g.values.insert(key.as_str().to_owned(), (value.to_vec(), now + ttl));
    }

    pub fn delete(&self, key: &CacheKey) {
        self.lock().values.remove(key.as_str());
    }

    pub fn rate_limit(&self, key: &CacheKey, limit: u32, window: Duration) -> RateDecision {
        let mut g = self.lock();
        let now = Instant::now();
        let entry = g.windows.entry(key.as_str().to_owned()).or_insert((0, now + window));
        if entry.1 <= now {
            *entry = (0, now + window);
        }
        entry.0 = entry.0.saturating_add(1);
        RateDecision {
            allowed: entry.0 <= limit,
            remaining: limit.saturating_sub(entry.0),
            reset_after: entry.1.saturating_duration_since(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test(start_paused = true)]
    async fn values_expire_after_ttl() {
        let c = MemoryCache::default();
        let k = CacheKey::workspace(Uuid::nil(), "a");
        c.set(&k, b"1", Duration::from_secs(10));
        assert_eq!(c.get(&k).as_deref(), Some(&b"1"[..]));
        tokio::time::advance(Duration::from_secs(11)).await;
        assert_eq!(c.get(&k), None);
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_resets_per_window() {
        let c = MemoryCache::default();
        let k = CacheKey::system("login:1.2.3.4");
        for i in 1..=5 {
            assert!(c.rate_limit(&k, 5, Duration::from_secs(60)).allowed, "hit {i}");
        }
        let d = c.rate_limit(&k, 5, Duration::from_secs(60));
        assert!(!d.allowed);
        assert_eq!(d.remaining, 0);
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(c.rate_limit(&k, 5, Duration::from_secs(60)).allowed);
    }

    #[test]
    fn workspaces_do_not_collide() {
        let c = MemoryCache::default();
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        c.set(&CacheKey::workspace(a, "settings"), b"A", Duration::from_secs(60));
        assert_eq!(c.get(&CacheKey::workspace(b, "settings")), None);
    }
}
