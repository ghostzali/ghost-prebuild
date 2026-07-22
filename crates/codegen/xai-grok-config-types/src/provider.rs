//! Multi-provider configuration for ghost-prebuild.
//!
//! Supports configuring multiple OpenAI-compatible API providers with
//! individual API keys and model catalogs. Users can switch providers
//! at runtime via CLI flag, environment variable, or config file.
//!
//! Also supports reading auth from existing Codex CLI installations
//! (`~/.codex/auth.json`) for ChatGPT subscription-based access.
//!
//! ## Codex token caveat
//!
//! Codex OAuth access tokens are short-lived (~1 hour). Ghost Prebuild reads
//! the token fresh from `auth.json` on each resolve (the Codex CLI background
//! process refreshes this file periodically). If the token is expired, run
//! `codex login` to refresh, then restart ghost.

use serde::{Deserialize, Serialize};

/// Auth mode for a provider — replaces the stringly-typed `auth_mode` field.
///
/// Serialized as snake_case in TOML/JSON: `"api_key"`, `"codex"`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthMode {
    /// Standard API key authentication (default if unset).
    ApiKey,
    /// Read OAuth access token from `~/.codex/auth.json` (ChatGPT subscription).
    Codex,
    /// OAuth 2.0 PKCE flow with authorization code grant.
    /// Provider must have `oauth` config with authorize_url, token_url, etc.
    OAuth,
}

/// OAuth 2.0 configuration for a provider.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OAuthConfig {
    /// Authorization endpoint URL.
    /// Example: "https://auth.openai.com/authorize"
    pub authorize_url: String,

    /// Token endpoint URL.
    /// Example: "https://auth.openai.com/oauth/token"
    pub token_url: String,

    /// OAuth client ID (public client, no secret for desktop PKCE flow).
    pub client_id: String,

    /// Space-separated OAuth scopes.
    /// Example: "openai.models.read openai.models.use"
    #[serde(default)]
    pub scopes: Option<String>,

    /// Redirect URI for the OAuth callback.
    /// Defaults to "http://localhost:PORT/callback" (loopback with random port).
    /// NOTE: Some providers require pre-registered redirect URIs and won't
    /// accept a random port. For those, use device-code flow instead.
    #[serde(default)]
    pub redirect_uri: Option<String>,

    /// Label shown during login, e.g. "OpenAI (ChatGPT Plus/Pro)".
    #[serde(default)]
    pub login_label: Option<String>,
}

/// A configured API provider with its own base URL, auth, and model list.
///
/// Does NOT implement `Default` — an empty-name provider is an invalid state.
/// Use `ProviderConfig::named("openai")` or construct with explicit fields.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Unique provider name — used with `--provider` CLI flag.
    /// Examples: "openai", "xai", "anthropic", "local-ollama", "codex"
    pub name: String,

    /// OpenAI-compatible API base URL.
    /// Example: "https://api.openai.com/v1"
    #[serde(default)]
    pub api_base: Option<String>,

    /// API key for this provider. Supports `${ENV_VAR}` substitution.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Environment variable to read the API key from (e.g. "OPENAI_API_KEY").
    /// When set, the key is read from that env var at runtime.
    #[serde(default)]
    pub env_key: Option<String>,

    /// Auth mode for this provider.
    /// - `None` or `ProviderAuthMode::ApiKey`: standard API key flow.
    /// - `ProviderAuthMode::Codex`: read OAuth token from `~/.codex/auth.json`.
    /// - `ProviderAuthMode::OAuth`: OAuth 2.0 PKCE flow using the `oauth` config.
    #[serde(default)]
    pub auth_mode: Option<ProviderAuthMode>,

    /// OAuth 2.0 configuration (required when `auth_mode = "oauth"`).
    #[serde(default)]
    pub oauth: Option<OAuthConfig>,

    /// List of model IDs available from this provider.
    /// When empty, all models from the provider's API are available.
    #[serde(default)]
    pub models: Vec<String>,

    /// Filter models by credential type.
    /// When true, OAuth-required models are only included when the
    /// provider has a valid OAuth credential (not API key or env var).
    #[serde(default)]
    pub filter_by_credential: bool,

    /// Override context window size for this provider's models (Phase 6.2).
    /// When set, all models from this provider use this context window
    /// instead of their individual defaults. Useful for proxies and
    /// rate-limited tiers.
    #[serde(default)]
    pub context_window_override: Option<u64>,

    /// Priority for failover ordering (Phase 6.3). Lower = higher priority.
    /// When a provider fails, the next highest-priority provider with the
    /// same model is tried. Default: 0 (no special priority).
    #[serde(default)]
    pub priority: u32,

    /// Optional: custom HTTP headers to add to all requests to this provider.
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
}

impl ProviderConfig {
    /// Create a minimal provider config with just a name.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            api_base: None,
            api_key: None,
            env_key: None,
            auth_mode: None,
            oauth: None,
            models: Vec::new(),
            filter_by_credential: false,
            context_window_override: None,
            priority: 0,
            headers: Vec::new(),
        }
    }

    /// Resolve the effective API key:
    /// 1. Codex auth mode → read from `~/.codex/auth.json`
    /// 2. `api_key` field with `${ENV_VAR}` substitution
    /// 3. `env_key` field → `std::env::var()`
    pub fn resolve_api_key(&self) -> Option<String> {
        // Codex auth mode — delegate to codex auth file
        if self.auth_mode == Some(ProviderAuthMode::Codex) {
            return resolve_codex_auth();
        }

        // OAuth auth mode — read from credential store
        if self.auth_mode == Some(ProviderAuthMode::OAuth) {
            return resolve_oauth_credential(&self.name);
        }

        // Check the api_key field with ${ENV_VAR} substitution
        if let Some(ref key) = self.api_key {
            let resolved = resolve_env_vars_in_string(key);
            if resolved != *key {
                // Env var substitution occurred
                if resolved.is_empty() {
                    tracing::warn!(
                        "Provider '{}': env var referenced in api_key is unset or empty",
                        self.name
                    );
                    return None;
                }
                return Some(resolved);
            }
            // Plain key (no substitution pattern)
            if resolved.is_empty() {
                return None;
            }
            return Some(resolved);
        }

        // Check the env_key field
        if let Some(ref env_var) = self.env_key {
            match std::env::var(env_var) {
                Ok(val) if !val.is_empty() => return Some(val),
                Ok(_) => {
                    tracing::warn!(
                        "Provider '{}': env var '{}' is set but empty",
                        self.name,
                        env_var
                    );
                }
                Err(std::env::VarError::NotPresent) => {
                    tracing::warn!(
                        "Provider '{}': env var '{}' is not set",
                        self.name,
                        env_var
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Provider '{}': env var '{}' is invalid: {}",
                        self.name,
                        env_var,
                        e
                    );
                }
            }
        }

        None
    }

    /// Check whether a resolvable key exists — side-effect-free (no logging).
    /// Used by `ghost providers` listing so the key-status column doesn't
    /// emit `tracing::warn!` for expected absences.
    ///
    /// Mirrors [`resolve_api_key`]'s resolution order:
    /// codex auth file > api_key with \${ENV_VAR} substitution > env_key > None.
    pub fn has_resolvable_key(&self) -> bool {
        // Codex auth mode — check file existence
        if self.auth_mode == Some(ProviderAuthMode::Codex) {
            let auth_path = codex_home_path().join("auth.json");
            return auth_path.exists();
        }

        // OAuth auth mode — check credential store for valid token
        if self.auth_mode == Some(ProviderAuthMode::OAuth) {
            // Deferred to credential store: check ~/.ghost/credentials.json
            // for this provider's OAuth token. For now, OAuth providers
            // are considered "unresolved" until login is completed.
            return check_oauth_store(&self.name);
        }

        // Check api_key with env var substitution
        if let Some(ref key) = self.api_key {
            let resolved = resolve_env_vars_in_string(key);
            if resolved != *key {
                // Env var substitution occurred — check it resolved to non-empty
                return !resolved.is_empty();
            }
            // Plain key
            return !resolved.is_empty();
        }

        // Check env_key
        if let Some(ref env_var) = self.env_key {
            return std::env::var(env_var).is_ok_and(|v| !v.is_empty());
        }

        false
    }

    /// Resolve the effective API base URL with env var substitution.
    pub fn resolve_api_base(&self) -> Option<String> {
        // Codex auth mode always uses OpenAI API
        if self.auth_mode == Some(ProviderAuthMode::Codex) {
            return Some("https://api.openai.com/v1".to_string());
        }

        let resolved = self.api_base.as_ref().map(|url| resolve_env_vars_in_string(url));
        if resolved.is_none() {
            tracing::warn!(
                "Provider '{}': no api_base configured — requests to this provider will fail",
                self.name
            );
        }
        resolved
    }

    /// Check if this provider uses Codex subscription auth.
    pub fn is_codex(&self) -> bool {
        self.auth_mode == Some(ProviderAuthMode::Codex)
    }
}

/// The default models we register for the codex provider.
/// Single-sourced — both `auto_register_codex()` and `default_models.json`
/// should stay in sync with this list.
const CODEX_DEFAULT_MODELS: &[&str] = &["gpt-5.6-sol", "gpt-5.6-luna", "gpt-4.1"];

/// Resolve auth from an existing Codex CLI installation.
///
/// Reads `~/.codex/auth.json` on each call to pick up token refreshes
/// performed by the Codex CLI background process. The Codex CLI refreshes
/// OAuth tokens periodically and re-writes `auth.json`; ghost relies on
/// that rather than implementing its own refresh flow.
///
/// If the token is expired and Codex hasn't refreshed it yet, run
/// `codex login` to force a refresh, then restart ghost.
fn resolve_codex_auth() -> Option<String> {
    let codex_home = codex_home_path();
    let auth_path = codex_home.join("auth.json");

    match std::fs::read_to_string(&auth_path) {
        Ok(contents) => match serde_json::from_str::<serde_json::Value>(&contents) {
            Ok(auth) => {
                // Try access_token from tokens.access_token (OAuth mode)
                if let Some(token) = auth
                    .get("tokens")
                    .and_then(|t| t.get("access_token"))
                    .and_then(|v| v.as_str())
                    && !token.is_empty()
                {
                    tracing::debug!(
                        "Resolved codex auth from {}/auth.json",
                        codex_home.display()
                    );
                    return Some(token.to_string());
                }
                // Try direct OPENAI_API_KEY field (API key mode)
                if let Some(key) = auth.get("OPENAI_API_KEY").and_then(|v| v.as_str())
                    && !key.is_empty()
                {
                    return Some(key.to_string());
                }
                tracing::warn!(
                    "Codex auth.json found but no valid tokens — run `codex login` first"
                );
                None
            }
            Err(e) => {
                tracing::warn!("Failed to parse codex auth.json: {}", e);
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                "Codex auth.json not found at {} — install and login to Codex CLI first",
                auth_path.display()
            );
            None
        }
        Err(e) => {
            tracing::warn!("Failed to read codex auth.json: {}", e);
            None
        }
    }
}

/// Resolve the Codex home directory.
pub fn codex_home_path() -> std::path::PathBuf {
    // CODEX_HOME env var, or ~/.codex
    if let Ok(home) = std::env::var("CODEX_HOME") {
        return std::path::PathBuf::from(home);
    }
    #[allow(deprecated)]
    let home = std::env::home_dir()
        .or_else(|| std::env::var("HOME").ok().map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join(".codex")
}

/// Check if an OAuth credential exists for this provider.
/// Reads `~/.ghost/credentials.json` and looks for an OAuth entry.
///
/// NOTE: Called per-provider on every `ghost providers` invocation.
/// Perf: acceptable for expected handful of providers; if the list grows,
/// batch reads into a single file-load to avoid repeated I/O.
fn check_oauth_store(provider_name: &str) -> bool {
    resolve_oauth_credential_inner(provider_name).is_some()
}

/// Resolve an OAuth access token from the credential store.
fn resolve_oauth_credential(provider_name: &str) -> Option<String> {
    resolve_oauth_credential_inner(provider_name)
}

fn resolve_oauth_credential_inner(provider_name: &str) -> Option<String> {
    let ghost_home = std::env::var("GHOST_HOME")
        .ok()
        .or_else(|| std::env::var("GROK_HOME").ok())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            // Cross-platform home directory fallback
            #[cfg(windows)]
            { std::env::var("USERPROFILE").ok().map(std::path::PathBuf::from).unwrap_or_default().join(".ghost") }
            #[cfg(not(windows))]
            { std::env::var("HOME").ok().map(std::path::PathBuf::from).unwrap_or_default().join(".ghost") }
        });
    let creds_path = ghost_home.join("credentials.json");
    if let Ok(data) = std::fs::read_to_string(&creds_path)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&data)
        && let Some(providers) = json.as_object()
        && let Some(entry) = providers.get(provider_name)
        && let Some(auth_type) = entry.get("type").and_then(|v| v.as_str())
    {
        if auth_type == "oauth" {
            // Check expiry
            if let Some(expires) = entry.get("expires_at").and_then(|v| v.as_u64()) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if now >= expires {
                    tracing::warn!(
                        "Provider '{}': OAuth token expired. Run 'ghost login {} --oauth' to refresh.",
                        provider_name, provider_name
                    );
                    return None;
                }
            }
            // Return access token
            return entry.get("access_token").and_then(|v| v.as_str().map(|s| s.to_string()));
        }
        if auth_type == "api_key" {
            return entry.get("key").and_then(|v| v.as_str().map(|s| s.to_string()));
        }
    }
    None
}

/// A key-value pair for custom HTTP headers.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

/// Resolve `${ENV_VAR}` patterns in a string from environment variables.
fn resolve_env_vars_in_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // skip '{'
            let mut var_name = String::new();
            while let Some(&next) = chars.peek() {
                if next == '}' {
                    chars.next(); // skip '}'
                    break;
                }
                var_name.push(next);
                chars.next();
            }
            if let Ok(val) = std::env::var(&var_name) {
                result.push_str(&val);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Collection of providers loaded from config and defaults.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderRegistry {
    /// All configured providers, keyed by name.
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,

    /// The default provider name to use when none is specified.
    #[serde(default)]
    pub default_provider: Option<String>,
}

impl ProviderRegistry {
    /// Find a provider by name.
    pub fn find(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// Get the default provider, if any.
    pub fn default(&self) -> Option<&ProviderConfig> {
        self.default_provider
            .as_deref()
            .and_then(|name| self.find(name))
            .or_else(|| self.providers.first())
    }

    /// Find failover providers that support a given model (Phase 6.3).
    /// Returns providers sorted by priority (lower = higher priority),
    /// excluding the given provider. Used when a provider fails to
    /// try the next best alternative.
    pub fn failover_for(&self, model_id: &str, exclude_provider: &str) -> Vec<&ProviderConfig> {
        let mut candidates: Vec<&ProviderConfig> = self
            .providers
            .iter()
            .filter(|p| p.name != exclude_provider)
            .filter(|p| p.models.is_empty() || p.models.iter().any(|m| m == model_id))
            .collect();
        candidates.sort_by_key(|p| p.priority);
        candidates
    }

    /// Auto-register a codex provider if codex auth exists.
    /// Returns true if a codex provider was added.
    ///
    /// Does a single I/O read of `auth.json` (cached in `auth_token`)
    /// to avoid the double-I/O pattern: checks existence AND extracts
    /// the token in one pass.
    pub fn auto_register_codex(&mut self) -> bool {
        if self.find("codex").is_some() {
            return false; // Already registered
        }
        let auth_path = codex_home_path().join("auth.json");
        if !auth_path.exists() {
            return false;
        }
        // Probe auth.json once to decide whether to register. The token is
        // re-read fresh on each resolve_api_key() to pick up Codex CLI
        // background refreshes — we only cache the existence check here.
        let auth_token = resolve_codex_auth();
        if auth_token.is_some() {
            self.providers.push(ProviderConfig {
                name: "codex".to_string(),
                api_base: Some("https://api.openai.com/v1".to_string()),
                api_key: None,
                env_key: None,
                auth_mode: Some(ProviderAuthMode::Codex),
                oauth: None,
                models: CODEX_DEFAULT_MODELS.iter().map(|s| s.to_string()).collect(),
                filter_by_credential: false,
                context_window_override: None,
                priority: 0,
                headers: Vec::new(),
            });
            tracing::info!("Auto-registered 'codex' provider from existing Codex CLI auth");
            true
        } else {
            false
        }
    }
}

/// Get the effective provider name: CLI flag > env var > config default.
pub fn resolve_provider_name(
    cli_provider: Option<&str>,
    config: Option<&ProviderRegistry>,
) -> Option<String> {
    if let Some(name) = cli_provider {
        return Some(name.to_string());
    }
    if let Ok(name) = std::env::var("GHOST_DEFAULT_PROVIDER") {
        return Some(name);
    }
    if let Some(reg) = config {
        return reg.default_provider.clone();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[serial_test::serial]
    #[test]
    fn test_resolve_env_vars_basic() {
        // SAFETY: test is serialized via #[serial_test::serial]; no other thread mutates env.
        unsafe { std::env::set_var("GHOST_TEST_KEY", "sk-test-123") };
        let result = resolve_env_vars_in_string("Bearer ${GHOST_TEST_KEY}");
        assert_eq!(result, "Bearer sk-test-123");
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("GHOST_TEST_KEY") };
    }

    #[test]
    fn test_resolve_env_vars_unset() {
        let result = resolve_env_vars_in_string("Bearer ${NONEXISTENT_VAR}");
        assert_eq!(result, "Bearer ");
    }

    #[serial_test::serial]
    #[test]
    fn test_resolve_api_key_from_env_key() {
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::set_var("TEST_API_KEY", "sk-env-456") };
        let provider = ProviderConfig {
            name: "test".into(),
            env_key: Some("TEST_API_KEY".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(provider.resolve_api_key(), Some("sk-env-456".into()));
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("TEST_API_KEY") };
    }

    #[test]
    fn test_resolve_api_key_from_direct_value() {
        let provider = ProviderConfig {
            name: "test".into(),
            api_key: Some("sk-direct-789".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(provider.resolve_api_key(), Some("sk-direct-789".into()));
    }

    #[serial_test::serial]
    #[test]
    fn test_resolve_api_key_env_var_unset_returns_none() {
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("NONEXISTENT_VAR") };
        let provider = ProviderConfig {
            name: "test".into(),
            api_key: Some("${NONEXISTENT_VAR}".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(provider.resolve_api_key(), None);
    }

    #[serial_test::serial]
    #[test]
    fn test_resolve_api_key_env_var_empty_returns_none() {
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::set_var("EMPTY_VAR", "") };
        let provider = ProviderConfig {
            name: "test".into(),
            api_key: Some("${EMPTY_VAR}".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(provider.resolve_api_key(), None);
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("EMPTY_VAR") };
    }

    #[test]
    fn test_resolve_api_key_plain_key_without_substitution() {
        let provider = ProviderConfig {
            name: "test".into(),
            api_key: Some("sk-plain-key-no-dollars".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(
            provider.resolve_api_key(),
            Some("sk-plain-key-no-dollars".into())
        );
    }

    #[test]
    fn test_resolve_api_base_warns_when_none() {
        let provider = ProviderConfig::named("no-base");
        assert_eq!(provider.resolve_api_base(), None);
    }

    #[serial_test::serial]
    #[test]
    fn test_resolve_api_key_from_env_key_not_set() {
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("NONEXISTENT_VAR") };
        let provider = ProviderConfig {
            name: "test".into(),
            env_key: Some("NONEXISTENT_VAR".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(provider.resolve_api_key(), None);
    }

    #[test]
    fn test_find_provider() {
        let registry = ProviderRegistry {
            providers: vec![ProviderConfig::named("openai"), ProviderConfig::named("xai")],
            default_provider: Some("openai".into()),
        };
        assert!(registry.find("openai").is_some());
        assert!(registry.find("xai").is_some());
        assert!(registry.find("nonexistent").is_none());
        assert_eq!(registry.default().unwrap().name, "openai");
    }

    #[serial_test::serial]
    #[test]
    fn test_resolve_provider_name_free_function() {
        assert_eq!(
            resolve_provider_name(Some("openai"), None),
            Some("openai".to_string())
        );

        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::set_var("GHOST_DEFAULT_PROVIDER", "env-provider") };
        assert_eq!(
            resolve_provider_name(None, None),
            Some("env-provider".to_string())
        );
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("GHOST_DEFAULT_PROVIDER") };

        let registry = ProviderRegistry {
            providers: vec![],
            default_provider: Some("config-default".into()),
        };
        assert_eq!(
            resolve_provider_name(None, Some(&registry)),
            Some("config-default".to_string())
        );

        assert_eq!(resolve_provider_name(None, None), None);
    }

    #[serial_test::serial]
    #[test]
    fn test_codex_home_path_from_env() {
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::set_var("CODEX_HOME", "/custom/codex/home") };
        let path = codex_home_path();
        assert_eq!(path, std::path::PathBuf::from("/custom/codex/home"));
        // SAFETY: test is serialized; no concurrent env mutation.
        unsafe { std::env::remove_var("CODEX_HOME") };
    }

    #[test]
    fn test_provider_config_named() {
        let p = ProviderConfig::named("test-provider");
        assert_eq!(p.name, "test-provider");
        assert_eq!(p.api_base, None);
        assert_eq!(p.api_key, None);
        assert_eq!(p.models, Vec::<String>::new());
    }

    #[test]
    fn test_provider_auth_mode_serde() {
        // Round-trip the enum through JSON
        let codex_mode: ProviderAuthMode =
            serde_json::from_str("\"codex\"").expect("codex");
        assert_eq!(codex_mode, ProviderAuthMode::Codex);

        let api_key_mode: ProviderAuthMode =
            serde_json::from_str("\"api_key\"").expect("api_key");
        assert_eq!(api_key_mode, ProviderAuthMode::ApiKey);

        // Unknown variant should fail (no silent fallback)
        let err: Result<ProviderAuthMode, _> = serde_json::from_str("\"codeex\"");
        assert!(err.is_err());
    }

    #[test]
    fn test_provider_with_codex_auth_mode() {
        let provider = ProviderConfig {
            name: "codex".into(),
            api_base: Some("https://api.openai.com/v1".into()),
            auth_mode: Some(ProviderAuthMode::Codex),
            ..ProviderConfig::named("codex")
        };
        assert!(provider.is_codex());
        assert_eq!(
            provider.resolve_api_base(),
            Some("https://api.openai.com/v1".into())
        );
    }

    /// Integration test: multiple providers with different API keys.
    /// Uses DeepSeek and Z.AI — validates each resolves its own key independently.
    #[test]
    #[serial_test::serial]
    fn test_multi_provider_key_resolution() {
        // SAFETY: serialized test; no concurrent env mutation.
        unsafe {
            std::env::set_var("DEEPSEEK_API_KEY", "sk-deepseek-test-key-123");
            std::env::set_var("ZAI_API_KEY", "zai-test-token-456");
        }

        let deepseek = ProviderConfig {
            name: "deepseek".into(),
            api_base: Some("https://api.deepseek.com/v1".into()),
            env_key: Some("DEEPSEEK_API_KEY".into()),
            ..ProviderConfig::named("deepseek")
        };
        let zai = ProviderConfig {
            name: "zai".into(),
            api_base: Some("https://api.z.ai/v1".into()),
            env_key: Some("ZAI_API_KEY".into()),
            ..ProviderConfig::named("zai")
        };

        // Each provider resolves its own key
        assert!(deepseek.has_resolvable_key());
        assert_eq!(
            deepseek.resolve_api_key(),
            Some("sk-deepseek-test-key-123".into())
        );
        assert!(zai.has_resolvable_key());
        assert_eq!(zai.resolve_api_key(), Some("zai-test-token-456".into()));

        // Cleanup
        unsafe {
            std::env::remove_var("DEEPSEEK_API_KEY");
            std::env::remove_var("ZAI_API_KEY");
        }
    }
}
