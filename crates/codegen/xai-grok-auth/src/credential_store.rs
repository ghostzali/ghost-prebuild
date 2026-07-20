//! Credential store trait + file-based persistence.
//!
//! Mirrors Pi's `CredentialStore` (`auth/credential-store.ts`):
//! - `read()`: fetch stored credential
//! - `write()`: persist credential (Result)
//! - `modify()`: atomic read-modify-write with per-provider locking
//! - `delete()`: remove credential
//!
//! Security: credential file is stored with `0600` permissions on Unix.
//! Writes use temp-file + rename for atomicity.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing;

/// A stored credential for a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Credential {
    /// API key stored after `ghost login <provider> --api-key`.
    #[serde(rename = "api_key")]
    ApiKey {
        /// The API key value.
        key: String,
    },
    /// OAuth 2.0 token stored after `ghost login <provider> --oauth`.
    #[serde(rename = "oauth")]
    OAuth {
        /// Bearer access token.
        access_token: String,
        /// Refresh token (may be rotated by provider).
        #[serde(default)]
        refresh_token: Option<String>,
        /// Unix timestamp (seconds) when the access token expires.
        /// `None` = expiry unknown — treated conservatively (needs validation).
        #[serde(default)]
        expires_at: Option<u64>,
    },
}

impl Credential {
    /// Whether this credential is (likely) still valid.
    /// OAuth credentials without a recorded expiry are treated as "needs validation"
    /// (returns `false`) to avoid using potentially expired tokens.
    pub fn is_valid(&self) -> bool {
        match self {
            Credential::ApiKey { .. } => true,
            Credential::OAuth { expires_at, .. } => match expires_at {
                Some(exp) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    now < *exp
                }
                // No expiry recorded — conservative: treat as invalid
                None => false,
            },
        }
    }

    /// Whether this is an OAuth credential that needs refresh (expired or near expiry).
    /// Returns true if expires_at is within 5 minutes of now, or if no expiry is recorded.
    pub fn needs_refresh(&self) -> bool {
        match self {
            Credential::OAuth { expires_at, .. } => match expires_at {
                Some(exp) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    // Refresh if expiring within 5 minutes
                    now + 300 >= *exp
                }
                // No expiry recorded — refresh to be safe
                None => true,
            },
            _ => false,
        }
    }

    pub fn access_token(&self) -> Option<&str> {
        match self {
            Credential::ApiKey { key } => Some(key.as_str()),
            Credential::OAuth { access_token, .. } => Some(access_token.as_str()),
        }
    }
}

/// Error type for credential store operations.
#[derive(Debug)]
pub enum CredentialStoreError {
    /// I/O error reading or writing the credential file.
    Io(std::io::Error),
    /// JSON serialization/deserialization error.
    Json(serde_json::Error),
}

impl std::fmt::Display for CredentialStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialStoreError::Io(e) => write!(f, "credential store I/O error: {e}"),
            CredentialStoreError::Json(e) => write!(f, "credential store JSON error: {e}"),
        }
    }
}

impl std::error::Error for CredentialStoreError {}

impl From<std::io::Error> for CredentialStoreError {
    fn from(e: std::io::Error) -> Self { CredentialStoreError::Io(e) }
}

impl From<serde_json::Error> for CredentialStoreError {
    fn from(e: serde_json::Error) -> Self { CredentialStoreError::Json(e) }
}

/// Asynchronous credential storage.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    /// Read the stored credential for a provider.
    async fn read(&self, provider_id: &str) -> Result<Option<Credential>, CredentialStoreError>;

    /// Write (create or overwrite) a credential for a provider.
    async fn write(&self, provider_id: &str, credential: Credential) -> Result<(), CredentialStoreError>;

    /// Atomic read-modify-write with per-provider serialization.
    ///
    /// `f` receives the current credential (if any, by value) and returns the new
    /// credential to store, or `None` to leave the existing credential
    /// **unchanged** (matching Pi's `undefined` → unchanged semantics).
    ///
    /// Calls to `modify()` for the same `provider_id` are serialized via
    /// an internal per-key lock. Concurrent calls for different providers
    /// proceed independently.
    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> Result<Option<Credential>, CredentialStoreError> + Send>,
    ) -> Result<(), CredentialStoreError>;

    /// Delete the stored credential for a provider.
    async fn delete(&self, provider_id: &str) -> Result<(), CredentialStoreError>;
}

// ── File-based implementation ──────────────────────────────────────────

/// File-backed credential store at `~/.ghost/credentials.json`.
///
/// - **Permissions**: creates files with `0600` on Unix.
/// - **Atomic writes**: temp file + rename.
/// - **Per-key locking**: `modify()` serializes concurrent calls per provider_id
///   via an internal `HashMap<provider_id, Mutex<()>>`.
pub struct FileCredentialStore {
    path: PathBuf,
    locks: Mutex<HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
}

impl FileCredentialStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            locks: Mutex::new(HashMap::new()),
        }
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
        home.join("credentials.json")
    }

    /// Get (or create) a per-provider mutex. Uses a global lock to safely
    /// access the map, then drops it before returning the per-key lock.
    fn per_key_lock(&self, provider_id: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let mut map = self.locks.lock().unwrap();
        map.entry(provider_id.to_string())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn read_all(&self) -> Result<serde_json::Map<String, serde_json::Value>, CredentialStoreError> {
        match std::fs::read_to_string(&self.path) {
            Ok(data) if data.trim().is_empty() => Ok(serde_json::Map::new()),
            Ok(data) => Ok(serde_json::from_str(&data).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
            Err(e) => Err(CredentialStoreError::Io(e)),
        }
    }

    fn write_all(&self, data: &serde_json::Map<String, serde_json::Value>) -> Result<(), CredentialStoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Use 0600 permissions on Unix
        let tmp = self.path.with_extension("tmp");
        let mut file = {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            opts.open(&tmp)?
        };

        let json = serde_json::to_string_pretty(data).unwrap_or_default();
        file.write_all(json.as_bytes())?;
        file.flush()?;

        // Rename to final path (atomic on same filesystem)
        std::fs::rename(&tmp, &self.path)?;

        // Set 0600 on the final file too (rename preserves mode of dest, not src)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&self.path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&self.path, perms);
            }
        }

        // Clean up tmp on failure to rename (best-effort)
        #[cfg(windows)]
        { let _ = std::fs::remove_file(&tmp); }

        tracing::debug!("Wrote credentials to {}", self.path.display());
        Ok(())
    }
}

#[async_trait]
impl CredentialStore for FileCredentialStore {
    async fn read(&self, provider_id: &str) -> Result<Option<Credential>, CredentialStoreError> {
        let all = self.read_all()?;
        Ok(all
            .get(provider_id)
            .and_then(|v| serde_json::from_value(v.clone()).ok()))
    }

    async fn write(&self, provider_id: &str, credential: Credential) -> Result<(), CredentialStoreError> {
        let mut all = self.read_all()?;
        all.insert(
            provider_id.to_string(),
            serde_json::to_value(&credential).map_err(CredentialStoreError::Json)?,
        );
        self.write_all(&all)
    }

    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> Result<Option<Credential>, CredentialStoreError> + Send>,
    ) -> Result<(), CredentialStoreError> {
        let lock = self.per_key_lock(provider_id);
        let _guard = lock.lock().await;

        let mut all = self.read_all()?;
        let current = all
            .get(provider_id)
            .and_then(|v| serde_json::from_value::<Credential>(v.clone()).ok());
        let result = f(current)?;

        match result {
            Some(new_cred) => {
                all.insert(
                    provider_id.to_string(),
                    serde_json::to_value(&new_cred).map_err(CredentialStoreError::Json)?,
                );
                self.write_all(&all)?;
            }
            None => {
                // None means "leave unchanged" — matching Pi's contract
                return Ok(());
            }
        }
        Ok(())
    }

    async fn delete(&self, provider_id: &str) -> Result<(), CredentialStoreError> {
        let mut all = self.read_all()?;
        all.remove(provider_id);
        self.write_all(&all)
    }
}
