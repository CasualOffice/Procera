//! Integration tests against a real Valkey/Redis-protocol server.
//! Skipped unless PROCERA_TEST_CACHE_URL is set (CI provides a Valkey service).

use std::time::Duration;

use procera_cache::{Cache, CacheKey, Health};
use uuid::Uuid;

async fn cache() -> Option<Cache> {
    let url = std::env::var("PROCERA_TEST_CACHE_URL").ok()?;
    Some(Cache::from_url(Some(&url)).await.unwrap_or_else(|e| panic!("connect {url}: {e}")))
}

#[tokio::test]
async fn round_trip_ttl_delete_and_namespacing() {
    let Some(c) = cache().await else { return };
    assert_eq!(c.backend(), "valkey");
    assert_eq!(c.health().await, Health::Ok);
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    let k = CacheKey::workspace(a, "effective-settings");
    c.set(&k, b"{\"x\":1}", Duration::from_millis(300)).await;
    assert_eq!(c.get(&k).await.as_deref(), Some(&b"{\"x\":1}"[..]));
    assert_eq!(c.get(&CacheKey::workspace(b, "effective-settings")).await, None, "other workspace must miss");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(c.get(&k).await, None, "expired by TTL");
    c.set(&k, b"y", Duration::from_secs(60)).await;
    c.delete(&k).await;
    assert_eq!(c.get(&k).await, None);
}

#[tokio::test]
async fn rate_limit_is_shared_across_clients() {
    let Some(c1) = cache().await else { return };
    let Some(c2) = cache().await else { return };
    let k = CacheKey::system(&format!("login:{}", Uuid::now_v7()));
    let mut allowed = 0;
    for i in 0..10 {
        let c = if i % 2 == 0 { &c1 } else { &c2 }; // two API replicas
        if c.rate_limit(&k, 6, Duration::from_secs(60)).await.allowed {
            allowed += 1;
        }
    }
    assert_eq!(allowed, 6, "limit enforced across replicas");
}

#[tokio::test]
async fn unreachable_server_degrades_without_failing() {
    // Port 1 is never a Redis server. Connecting fails fast; callers fall back to memory.
    let r = Cache::from_url(Some("redis://127.0.0.1:1")).await;
    assert!(r.is_err(), "connect must report the outage so startup can fall back");
}
