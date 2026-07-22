//! Model store trait + file-based persistence.
//!
//! Mirrors Pi's `ModelsStore` (`models-store.ts`):
//! - `read(provider)`: fetch stored model list for a provider
//! - `write(provider, entry)`: persist models + last-checked timestamp
//! - `delete(provider)`: remove stored models

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::io::Write;
use tracing;

/// A stored model entry for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredModels {
    /// Models fetched from the provider's API.
    pub models: Vec<String>,
    /// Unix timestamp (seconds) when the models were last checked.
    pub checked_at: u64,
}

/// Model store for persisting provider model lists.
pub trait ModelStore: Send + Sync {
    /// Read stored models for a provider.
    fn read(&self, provider_id: &str) -> Option<StoredModels>;

    /// Write (create or overwrite) stored models for a provider.
    fn write(&self, provider_id: &str, entry: StoredModels);

    /// Delete stored models for a provider.
    fn delete(&self, provider_id: &str);
}

// ── File-based implementation ──────────────────────────────────────────

/// File-backed model store at `~/.ghost/models-store.json`.
#[derive(Debug, Clone)]
pub struct FileModelStore {
    path: PathBuf,
}

impl FileModelStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        let home = std::env::var("GHOST_HOME")
            .ok()
            .or_else(|| std::env::var("GROK_HOME").ok())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                #[cfg(windows)]
                { std::env::var("USERPROFILE").ok().map(PathBuf::from).unwrap_or_default().join(".ghost") }
                #[cfg(not(windows))]
                { std::env::var("HOME").ok().map(PathBuf::from).unwrap_or_default().join(".ghost") }
            });
        home.join("models-store.json")
    }

    fn read_all(&self) -> serde_json::Map<String, serde_json::Value> {
        match std::fs::read_to_string(&self.path) {
            Ok(data) if data.trim().is_empty() => serde_json::Map::new(),
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => serde_json::Map::new(),
        }
    }

    fn write_all(&self, data: &serde_json::Map<String, serde_json::Value>) {
        if let Some(parent) = self.path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::error!("ModelStore: failed to create dir {:?}: {e}", parent);
            return;
        }
        let json = serde_json::to_string_pretty(data).unwrap_or_default();
        let tmp = self.path.with_extension("tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o644);
        }
        let mut f = match opts.open(&tmp) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("ModelStore: failed to open {:?}: {e}", tmp);
                return;
            }
        };
        if let Err(e) = f.write_all(json.as_bytes()) {
            tracing::error!("ModelStore: write_all failed for {:?}: {e}", tmp);
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        if let Err(e) = f.flush() {
            tracing::error!("ModelStore: flush failed for {:?}: {e}", tmp);
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            tracing::error!("ModelStore: rename {:?} → {:?} failed: {e}", tmp, self.path);
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

impl ModelStore for FileModelStore {
    fn read(&self, provider_id: &str) -> Option<StoredModels> {
        let all = self.read_all();
        all.get(provider_id)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    fn write(&self, provider_id: &str, entry: StoredModels) {
        let mut all = self.read_all();
        all.insert(
            provider_id.to_string(),
            serde_json::to_value(&entry).unwrap_or_default(),
        );
        self.write_all(&all);
    }

    fn delete(&self, provider_id: &str) {
        let mut all = self.read_all();
        all.remove(provider_id);
        self.write_all(&all);
    }
}
