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
    // Header: 1-char marker column + 20-wide name + 10-wide auth + 12-wide key
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
        println!(
            "{:>2} {:<18} {:<10} {:<12} {}",
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
                "key_configured": has_key_quiet(p),
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

/// Check whether a provider has a resolvable key without side effects.
/// Unlike `resolve_api_key()`, this does NOT emit `tracing::warn!` logs
/// for expected absences (missing codex auth file, unset env vars, etc.).
fn has_key_quiet(p: &ProviderConfig) -> bool {
    // Direct api_key field
    if p.api_key.as_ref().is_some_and(|k| !k.trim().is_empty()) {
        return true;
    }
    // Env var key
    if p.env_key.as_ref().is_some_and(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty())) {
        return true;
    }
    // Codex auth: check file existence without calling resolve_codex_auth()
    if p.auth_mode == Some(ProviderAuthMode::Codex) {
        let auth_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".codex")
            .join("auth.json");
        if auth_path.exists() {
            // File exists — assume it has a valid token.
            // resolve_codex_auth() would also validate JSON shape,
            // but for listing purposes file presence is enough.
            return true;
        }
    }
    false
}

fn key_status_label(p: &ProviderConfig) -> &str {
    if has_key_quiet(p) {
        "✓ resolved"
    } else {
        "✗ unset"
    }
}
