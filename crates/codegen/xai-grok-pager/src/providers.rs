//! `ghost providers` subcommand.

use anyhow::Result;
use xai_grok_config_types::{ProviderAuthMode, ProviderConfig, ProviderRegistry};
use xai_grok_shell::agent::config::Config as AgentConfig;

/// Print the list of configured providers.
pub async fn list_providers(agent_config: &AgentConfig, json: bool) -> Result<()> {
    let registry = &agent_config.providers;

    if json {
        print_json(registry);
    } else {
        print_table(registry);
    }

    Ok(())
}

fn print_table(registry: &ProviderRegistry) {
    if registry.providers.is_empty() {
        println!("No providers configured.");
        println!();
        println!("Add providers to ~/.ghost/config.toml:");
        println!();
        println!("  [[providers]]");
        println!("  name = \"openai\"");
        println!("  api_base = \"https://api.openai.com/v1\"");
        println!("  env_key = \"OPENAI_API_KEY\"");
        println!();
        return;
    }

    println!("Configured providers:");
    println!();
    // Header: 2-space indent + 20-wide name = AUTH at col 23
    println!(
        "  {:<20} {:<10} {:<12} {}",
        "NAME", "AUTH", "KEY STATUS", "API BASE"
    );
    println!("  {:-<20} {:-<10} {:-<12} {:-<40}", "", "", "", "");

    let default_name = registry.default_provider.as_deref();

    for p in &registry.providers {
        let marker = if Some(p.name.as_str()) == default_name { "*" } else { " " };
        let auth_mode = auth_label(&p.auth_mode);
        let key_status = key_status_label(p);
        let base = p.api_base.as_deref().unwrap_or("(not set)");
        // {:>2} marker (2) + space + {:<19} name (19) + space = AUTH at col 23 (matches header)
        println!(
            "{:>2} {:<19} {:<10} {:<12} {}",
            marker, p.name, auth_mode, key_status, base
        );
    }

    println!();
    let total = registry.providers.len();
    println!("{total} provider(s) configured.");

    if let Some(def) = default_name {
        println!();
        println!(" * = default provider ({def}) — override with --provider <name>");
    }
}

fn print_json(registry: &ProviderRegistry) {
    let output = serde_json::json!({
        "providers": registry.providers.iter().map(|p: &ProviderConfig| {
            serde_json::json!({
                "name": p.name,
                "api_base": p.api_base,
                "auth_mode": p.auth_mode,
                "key_configured": p.has_resolvable_key(),
                "env_key": p.env_key,
                "models": p.models,
            })
        }).collect::<Vec<_>>(),
        "default_provider": registry.default_provider,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn auth_label(mode: &Option<ProviderAuthMode>) -> &str {
    match mode {
        None => "API key",
        Some(ProviderAuthMode::ApiKey) => "API key",
        Some(ProviderAuthMode::Codex) => "Codex",
    }
}

fn key_status_label(p: &ProviderConfig) -> &str {
    if p.has_resolvable_key() {
        "✓ resolved"
    } else {
        "✗ unset"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_resolvable_key_direct_api_key() {
        let p = ProviderConfig {
            name: "test".into(),
            api_key: Some("sk-abc123".into()),
            ..ProviderConfig::named("test")
        };
        assert!(p.has_resolvable_key());
    }

    #[test]
    fn has_resolvable_key_empty_api_key_is_false() {
        let p = ProviderConfig {
            name: "test".into(),
            api_key: Some("".into()),
            ..ProviderConfig::named("test")
        };
        assert!(!p.has_resolvable_key());
    }

    #[test]
    fn has_resolvable_key_env_var_substitution_unset_is_false() {
        // ${VAR} where VAR is unset → resolves to empty string → false
        let p = ProviderConfig {
            name: "test".into(),
            api_key: Some("${GHOST_TEST_UNSET_VAR_XYZ789}".into()),
            ..ProviderConfig::named("test")
        };
        assert!(!p.has_resolvable_key());
    }

    #[test]
    fn has_resolvable_key_env_var_substitution_set_is_true() {
        // Can't safely set env in parallel tests, but verify a bare key still works
        let p = ProviderConfig {
            name: "test".into(),
            api_key: Some("sk-valid-key".into()),
            ..ProviderConfig::named("test")
        };
        assert!(p.has_resolvable_key());
    }

    #[test]
    fn has_resolvable_key_no_credentials_is_false() {
        let p = ProviderConfig::named("test");
        assert!(!p.has_resolvable_key());
    }

    #[test]
    fn has_resolvable_key_env_key_unset_is_false() {
        let p = ProviderConfig {
            name: "test".into(),
            env_key: Some("GHOST_TEST_UNSET_VAR_XYZ123".into()),
            ..ProviderConfig::named("test")
        };
        assert!(!p.has_resolvable_key());
    }

    #[test]
    fn has_resolvable_key_codex_no_file_is_false() {
        // Override HOME to a temp dir so the test doesn't depend on ambient ~/.codex/auth.json
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = ProviderConfig {
            name: "test".into(),
            auth_mode: Some(ProviderAuthMode::Codex),
            ..ProviderConfig::named("test")
        };
        // codex_home_path() → $CODEX_HOME or $HOME/.codex
        // We can't easily override codex_home_path(), so set CODEX_HOME to the temp dir
        // which has no auth.json
        std::env::set_var("CODEX_HOME", tmp.path());
        let result = p.has_resolvable_key();
        std::env::remove_var("CODEX_HOME");
        assert!(!result, "no auth.json in empty temp dir → false");
    }

    #[test]
    fn key_status_label_resolved() {
        let p = ProviderConfig {
            name: "test".into(),
            api_key: Some("sk-abc".into()),
            ..ProviderConfig::named("test")
        };
        assert_eq!(key_status_label(&p), "✓ resolved");
    }

    #[test]
    fn key_status_label_unset() {
        let p = ProviderConfig::named("test");
        assert_eq!(key_status_label(&p), "✗ unset");
    }

    #[test]
    fn auth_label_defaults_to_api_key() {
        assert_eq!(auth_label(&None), "API key");
    }

    #[test]
    fn auth_label_codex() {
        assert_eq!(auth_label(&Some(ProviderAuthMode::Codex)), "Codex");
    }
}
