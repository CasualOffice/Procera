//! Object storage port (FR-F03). Keys are bound to the workspace (`ws/<workspace>/<file>/<chunk>`).
//! The local filesystem backend serves single-node and development installs; an S3-compatible backend
//! plugs in behind the same calls.

use std::path::PathBuf;

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
