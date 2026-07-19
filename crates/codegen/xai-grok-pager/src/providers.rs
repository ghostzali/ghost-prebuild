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
    println!(
        "  {:<20} {:<10} {:<12} {}",
        "NAME", "AUTH", "KEY STATUS", "API BASE"
    );
    println!("  {:-<20} {:-<10} {:-<12} {:-<40}", "", "", "", "");

    let default_name = registry
        .default()
        .map(|p| p.name.as_str());

    for p in &registry.providers {
        let marker = if Some(p.name.as_str()) == default_name {
            "*"
        } else {
            " "
        };
        let auth_mode = auth_label(&p.auth_mode);
        let key_status = key_status_label(p);
        let base = p.api_base.as_deref().unwrap_or("(not set)");
        println!(
            "{} {:<19} {:<10} {:<12} {}",
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
            let key_resolved = p.resolve_api_key().is_some();
            serde_json::json!({
                "name": p.name,
                "api_base": p.api_base,
                "auth_mode": p.auth_mode.as_ref().map(|m| match m {
                    ProviderAuthMode::ApiKey => "api_key",
                    ProviderAuthMode::Codex => "codex",
                }),
                "key_configured": key_resolved,
                "env_key": p.env_key,
                "models": p.models,
            })
        }).collect::<Vec<_>>(),
        "default_provider": registry.default().map(|p| &p.name),
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|e| format!("JSON error: {e}")));
}

fn auth_label(mode: &Option<ProviderAuthMode>) -> &str {
    match mode {
        None => "API key",
        Some(ProviderAuthMode::ApiKey) => "API key",
        Some(ProviderAuthMode::Codex) => "Codex",
    }
}

fn key_status_label(p: &ProviderConfig) -> &str {
    if p.resolve_api_key().is_some() {
        "✓ resolved"
    } else {
        "✗ unset"
    }
}
