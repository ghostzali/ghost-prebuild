//! Unified env var accessor — Ghost ecosystem env vars with GROK_* fallback.
//!
//! Mirrors Pi's approach: every env var reader goes through a single point,
//! with deprecation warnings when GROK_* vars are used.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Ghost env var accessor. Reads GHOST_* first, falls back to GROK_*.
pub struct GhostEnv;

/// Track which env vars have already warned (rate-limit per var per session).
static WARNED_VARS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

fn warn_once(var_name: &str, ghost_name: &str) {
    let mut warned = WARNED_VARS.lock().unwrap_or_else(|e| e.into_inner());
    if warned.insert(var_name.to_string()) {
        tracing::warn!(
            "Env var {} is deprecated. Use {} instead.",
            var_name, ghost_name
        );
    }
}

impl GhostEnv {
    /// Read an env var with GHOST_* → GROK_* fallback.
    /// Logs a deprecation warning when falling back to the GROK var.
    pub fn var(name: &str) -> Option<String> {
    let ghost_name = format!("GHOST_{}", name);
    if let Ok(val) = std::env::var(&ghost_name)
        && !val.is_empty()
    {
        return Some(val);
    }

    let grok_name = format!("GROK_{}", name);
    if let Ok(val) = std::env::var(&grok_name)
        && !val.is_empty()
    {
        warn_once(&grok_name, &ghost_name);
        return Some(val);
    }

    let xai_name = format!("XAI_{}", name);
    if let Ok(val) = std::env::var(&xai_name)
        && !val.is_empty()
    {
        warn_once(&xai_name, &ghost_name);
        return Some(val);
    }

    None
}

    /// Resolve the ghost home directory.
    /// Order: GHOST_HOME → GROK_HOME → ~/.ghost → ~/.grok
    pub fn home() -> PathBuf {
        if let Some(home) = Self::var("HOME") {
            return PathBuf::from(home);
        }
        // Fall back to default paths
        let default_ghost = Self::home_dir().join(".ghost");
        if default_ghost.exists() {
            return default_ghost;
        }
        let default_grok = Self::home_dir().join(".grok");
        if default_grok.exists() {
            tracing::warn!("Using legacy ~/.grok directory. Migrate to ~/.ghost.");
            return default_grok;
        }
        default_ghost
    }

    /// Read the API key from the environment.
    /// Order: GHOST_API_KEY → XAI_API_KEY → GROK_API_KEY
    pub fn api_key() -> Option<String> {
        Self::var("API_KEY")
    }

    /// Read the deployment key.
    /// Order: GHOST_DEPLOYMENT_KEY → GROK_DEPLOYMENT_KEY
    pub fn deployment_key() -> Option<String> {
        Self::var("DEPLOYMENT_KEY")
    }

    /// Cross-platform home directory.
    fn home_dir() -> PathBuf {
        #[cfg(windows)]
        { std::env::var("USERPROFILE").ok().map(PathBuf::from).unwrap_or_default() }
        #[cfg(not(windows))]
        { std::env::var("HOME").ok().map(PathBuf::from).unwrap_or_default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn ghost_env_prefers_ghost_over_grok() {
        unsafe {
            std::env::set_var("GHOST_TEST_KEY", "ghost-value");
            std::env::set_var("GROK_TEST_KEY", "grok-value");
        }
        assert_eq!(GhostEnv::var("TEST_KEY").as_deref(), Some("ghost-value"));
        unsafe {
            std::env::remove_var("GHOST_TEST_KEY");
            std::env::remove_var("GROK_TEST_KEY");
        }
    }

    #[test]
    #[serial_test::serial]
    fn ghost_env_falls_back_to_grok() {
        unsafe {
            std::env::set_var("GROK_TEST_KEY", "grok-value");
        }
        assert_eq!(GhostEnv::var("TEST_KEY").as_deref(), Some("grok-value"));
        unsafe {
            std::env::remove_var("GROK_TEST_KEY");
        }
    }

    #[test]
    #[serial_test::serial]
    fn ghost_env_returns_none_when_neither_set() {
        assert_eq!(GhostEnv::var("NONEXISTENT_TEST_KEY_XYZ"), None);
    }
}
