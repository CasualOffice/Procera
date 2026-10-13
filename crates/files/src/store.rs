//! Object storage port (FR-F03, BPMS-209). Keys are bound to the workspace
//! (`ws/<workspace>/<file>/<chunk>`). Two backends behind the same calls: the local filesystem for
//! single-node and development installs, and any S3-compatible object store (AWS S3, Ceph, SeaweedFS,
//! Cloudflare R2…) through the `object_store` crate. Chunks are sealed (encrypted) before they reach
//! either backend, so the store never sees plaintext.

use std::path::PathBuf;
use std::sync::Arc;

use futures::TryStreamExt;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, ObjectStoreExt};

/// Where file chunks live.
#[derive(Debug, Clone)]
pub enum Store {
    Local(LocalStore),
    /// An object store, with every key under `prefix` (may be empty).
    Object {
        store: Arc<dyn ObjectStore>,
        prefix: String,
        label: String,
    },
}

/// S3-compatible settings (PROCERA_S3_*).
#[derive(Debug, Clone, Default)]
pub struct S3Config {
    pub bucket: String,
    pub region: Option<String>,
    /// For non-AWS stores, e.g. `https://s3.example.com` or `http://seaweedfs:8333`.
    pub endpoint: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    /// Allow plain HTTP endpoints (development and private networks only).
    pub allow_http: bool,
    /// Path-style addressing (`endpoint/bucket/key`), which most non-AWS stores need.
    pub path_style: bool,
    pub prefix: String,
}

impl Store {
    pub fn local(root: impl Into<PathBuf>) -> Self {
        Self::Local(LocalStore::new(root))
    }

    /// An S3-compatible store. Credentials come from the config or, when absent, the usual AWS
    /// environment and instance metadata chain.
    pub fn s3(c: &S3Config) -> Result<Self, String> {
        if c.bucket.trim().is_empty() {
            return Err("S3 storage: a bucket name is required (PROCERA_S3_BUCKET)".into());
        }
        let mut b = object_store::aws::AmazonS3Builder::from_env().with_bucket_name(&c.bucket);
        if let Some(r) = &c.region {
            b = b.with_region(r);
        }
        if let Some(e) = &c.endpoint {
            b = b.with_endpoint(e);
        }
        if let (Some(k), Some(s)) = (&c.access_key_id, &c.secret_access_key) {
            b = b.with_access_key_id(k).with_secret_access_key(s);
        }
        b = b.with_allow_http(c.allow_http).with_virtual_hosted_style_request(!c.path_style);
        let store = b.build().map_err(|e| format!("S3 storage: {e}"))?;
        let label = format!("s3://{}/{}", c.bucket, c.prefix.trim_matches('/'));
        Ok(Self::Object { store: Arc::new(store), prefix: c.prefix.trim_matches('/').to_owned(), label })
    }

    /// Any `object_store` backend (tests use the in-memory one).
    pub fn object(store: Arc<dyn ObjectStore>, prefix: &str, label: &str) -> Self {
        Self::Object { store, prefix: prefix.trim_matches('/').to_owned(), label: label.to_owned() }
    }

    /// Where files are kept, for the platform health page.
    pub fn describe(&self) -> String {
        match self {
            Self::Local(l) => l.root.display().to_string(),
            Self::Object { label, .. } => label.clone(),
        }
    }

    fn checked(key: &str) -> Result<(), String> {
        if key.is_empty() || key.contains("..") || key.starts_with('/') {
            return Err("invalid storage key".into());
        }
        Ok(())
    }

    fn object_path(prefix: &str, key: &str, chunk: Option<i32>) -> ObjPath {
        let mut p = if prefix.is_empty() { key.to_owned() } else { format!("{prefix}/{key}") };
        if let Some(c) = chunk {
            p = format!("{p}/{c:08}.bin");
        }
        ObjPath::from(p)
    }

    pub async fn put_chunk(&self, key: &str, chunk: i32, bytes: &[u8]) -> Result<(), String> {
        match self {
            Self::Local(l) => l.put_chunk(key, chunk, bytes).await,
            Self::Object { store, prefix, .. } => {
                Self::checked(key)?;
                store
                    .put(
                        &Self::object_path(prefix, key, Some(chunk)),
                        bytes::Bytes::copy_from_slice(bytes).into(),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }
    }

    pub async fn get_chunk(&self, key: &str, chunk: i32) -> Result<Vec<u8>, String> {
        match self {
            Self::Local(l) => l.get_chunk(key, chunk).await,
            Self::Object { store, prefix, .. } => {
                Self::checked(key)?;
                let r = store
                    .get(&Self::object_path(prefix, key, Some(chunk)))
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(r.bytes().await.map_err(|e| e.to_string())?.to_vec())
            }
        }
    }

    /// Deletes every chunk under `key` (missing is fine).
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        match self {
            Self::Local(l) => l.delete(key).await,
            Self::Object { store, prefix, .. } => {
                Self::checked(key)?;
                let under = Self::object_path(prefix, key, None);
                let paths: Vec<ObjPath> = store
                    .list(Some(&under))
                    .map_ok(|m| m.location)
                    .try_collect()
                    .await
                    .map_err(|e| e.to_string())?;
                for p in paths {
                    match store.delete(&p).await {
                        Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                        Err(e) => return Err(e.to_string()),
                    }
                }
                Ok(())
            }
        }
    }
}

impl From<LocalStore> for Store {
    fn from(l: LocalStore) -> Self {
        Self::Local(l)
    }
}

#[derive(Debug, Clone)]
pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, key: &str, chunk: i32) -> Result<PathBuf, String> {
        if key.contains("..") || key.starts_with('/') {
            return Err("invalid storage key".into());
        }
        Ok(self.root.join(key).join(format!("{chunk:08}.bin")))
    }

    pub async fn put_chunk(&self, key: &str, chunk: i32, bytes: &[u8]) -> Result<(), String> {
        let p = self.path(key, chunk)?;
        if let Some(dir) = p.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
        }
        tokio::fs::write(&p, bytes).await.map_err(|e| e.to_string())
    }

    pub async fn get_chunk(&self, key: &str, chunk: i32) -> Result<Vec<u8>, String> {
        tokio::fs::read(self.path(key, chunk)?).await.map_err(|e| e.to_string())
    }

    pub async fn delete(&self, key: &str) -> Result<(), String> {
        if key.contains("..") || key.starts_with('/') {
            return Err("invalid storage key".into());
        }
        match tokio::fs::remove_dir_all(self.root.join(key)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use object_store::memory::InMemory;

    /// The same contract for every backend: chunks round-trip, keys stay apart, delete removes a
    /// file's chunks (and is fine when nothing is there), and unsafe keys are refused.
    async fn contract(s: &Store) {
        s.put_chunk("ws/a/f1", 0, b"first").await.unwrap();
        s.put_chunk("ws/a/f1", 1, b"second").await.unwrap();
        s.put_chunk("ws/a/f2", 0, b"other").await.unwrap();
        assert_eq!(s.get_chunk("ws/a/f1", 1).await.unwrap(), b"second");
        assert_eq!(s.get_chunk("ws/a/f2", 0).await.unwrap(), b"other");
        s.delete("ws/a/f1").await.unwrap();
        assert!(s.get_chunk("ws/a/f1", 0).await.is_err(), "deleted");
        assert_eq!(s.get_chunk("ws/a/f2", 0).await.unwrap(), b"other", "a neighbour survives");
        s.delete("ws/a/nothing-here").await.unwrap();
        for bad in ["../x", "/etc", "ws/../../x"] {
            assert!(s.put_chunk(bad, 0, b"x").await.is_err(), "{bad}");
            assert!(s.delete(bad).await.is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn local_disk_keeps_the_contract() {
        let dir = std::env::temp_dir().join(format!("procera-store-{}", uuid::Uuid::now_v7()));
        let s = Store::local(&dir);
        contract(&s).await;
        assert_eq!(s.describe(), dir.display().to_string());
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn an_object_store_keeps_the_contract_under_its_prefix() {
        let mem = Arc::new(InMemory::new());
        let s = Store::object(mem.clone(), "/procera/prod/", "memory://procera/prod");
        contract(&s).await;
        // Everything lives under the prefix.
        let all: Vec<ObjPath> = mem.list(None).map_ok(|m| m.location).try_collect().await.unwrap();
        assert!(
            !all.is_empty() && all.iter().all(|p| p.as_ref().starts_with("procera/prod/ws/a/")),
            "{all:?}"
        );
        // Another prefix on the same bucket sees nothing of it.
        let other = Store::object(mem, "staging", "memory://staging");
        assert!(other.get_chunk("ws/a/f2", 0).await.is_err());
    }

    #[test]
    fn s3_config_builds_without_network_and_needs_a_bucket() {
        let c = S3Config {
            bucket: "procera-files".into(),
            region: Some("eu-west-1".into()),
            endpoint: Some("http://seaweedfs:8333".into()),
            access_key_id: Some("k".into()),
            secret_access_key: Some("s".into()),
            allow_http: true,
            path_style: true,
            prefix: "prod".into(),
        };
        let s = Store::s3(&c).expect("builds");
        assert_eq!(s.describe(), "s3://procera-files/prod");
        assert!(Store::s3(&S3Config { bucket: String::new(), ..c }).is_err(), "a bucket is required");
    }
}
