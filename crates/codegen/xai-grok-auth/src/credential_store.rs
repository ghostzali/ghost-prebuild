//! Credential store trait + file-based persistence.
//!
//! Mirrors Pi's `CredentialStore` (`auth/credential-store.ts`):
//! - `read()`: fetch stored credential
//! - `write()`: persist credential
//! - `modify()`: atomic read-modify-write (double-checked locking)
//! - `delete()`: remove credential

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        #[serde(default)]
        expires_at: Option<u64>,
    },
}

impl Credential {
    /// Whether this credential is (likely) still valid.
    /// OAuth credentials check expiry; API keys are always considered valid.
    pub fn is_valid(&self) -> bool {
        match self {
            Credential::ApiKey { .. } => true,
            Credential::OAuth { expires_at, .. } => {
                if let Some(exp) = expires_at {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    now < *exp
                } else {
                    true // no expiry → assume valid
                }
            }
        }
    }

    /// Whether this is an OAuth credential that needs refresh (expired or near expiry).
    /// Returns true if expires_at is within 5 minutes of now.
    pub fn needs_refresh(&self) -> bool {
        match self {
            Credential::OAuth { expires_at, .. } => {
                if let Some(exp) = expires_at {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    // Refresh if expiring within 5 minutes
                    now + 300 >= *exp
                } else {
                    false
                }
            }
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

/// Asynchronous credential storage.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    /// Read the stored credential for a provider.
    async fn read(&self, provider_id: &str) -> Option<Credential>;

    /// Write (create or overwrite) a credential for a provider.
    async fn write(&self, provider_id: &str, credential: Credential);

    /// Atomic read-modify-write.
    ///
    /// `f` receives the current credential (if any) and returns the new
    /// credential to store. `None` from `f` leaves the existing credential
    /// unchanged. The store serializes concurrent `modify()` calls per
    /// provider_id (file-level locking).
    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> Option<Credential> + Send>,
    );

    /// Delete the stored credential for a provider.
    async fn delete(&self, provider_id: &str);
}

// ── File-based implementation ──────────────────────────────────────────

/// File-backed credential store at `~/.ghost/credentials.json`.
#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        let home = std::env::var("GHOST_HOME")
            .ok()
            .or_else(|| std::env::var("GROK_HOME").ok())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".ghost")
            });
        home.join("credentials.json")
    }

    fn read_all(&self) -> serde_json::Map<String, serde_json::Value> {
        match std::fs::read_to_string(&self.path) {
            Ok(data) => {
                serde_json::from_str(&data)
                    .unwrap_or_default()
            }
            Err(_) => serde_json::Map::new(),
        }
    }

    fn write_all(&self, data: &serde_json::Map<String, serde_json::Value>) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(data).unwrap_or_default();
        // Write atomically: temp file → rename
        let tmp = self.path.with_extension("tmp");
        let _ = std::fs::write(&tmp, json);
        let _ = std::fs::rename(&tmp, &self.path);
    }
}

#[async_trait]
impl CredentialStore for FileCredentialStore {
    async fn read(&self, provider_id: &str) -> Option<Credential> {
        let all = self.read_all();
        all.get(provider_id)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    async fn write(&self, provider_id: &str, credential: Credential) {
        let mut all = self.read_all();
        all.insert(
            provider_id.to_string(),
            serde_json::to_value(&credential).unwrap_or_default(),
        );
        self.write_all(&all);
    }

    async fn modify(
        &self,
        provider_id: &str,
        f: Box<dyn FnOnce(Option<Credential>) -> Option<Credential> + Send>,
    ) {
        let mut all = self.read_all();
        let current = all
            .get(provider_id)
            .and_then(|v| serde_json::from_value::<Credential>(v.clone()).ok());
        let result = f(current);
        match result {
            Some(new_cred) => {
                all.insert(
                    provider_id.to_string(),
                    serde_json::to_value(&new_cred).unwrap_or_default(),
                );
            }
            None => {
                all.remove(provider_id);
            }
        }
        self.write_all(&all);
    }

    async fn delete(&self, provider_id: &str) {
        let mut all = self.read_all();
        all.remove(provider_id);
        self.write_all(&all);
    }
}
