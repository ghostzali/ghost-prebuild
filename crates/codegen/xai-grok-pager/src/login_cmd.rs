//! `ghost login <provider>` — multi-provider login command.
//!
//! Supported auth modes:
//! - `ghost login openai --api-key sk-xxx` — store API key directly
//! - `ghost login openai --oauth` — OAuth PKCE browser flow
//! - `ghost login codex` — use existing Codex CLI auth (reads ~/.codex/auth.json)

use anyhow::{Context, Result};
use xai_grok_auth::credential_store::{Credential, CredentialStore, FileCredentialStore};
use xai_grok_auth::oauth::flow::{login_oauth, OAuthFlowConfig};
use xai_grok_config_types::{ProviderAuthMode};
use xai_grok_shell::agent::config::Config as AgentConfig;

pub async fn login_provider(
    agent_config: &AgentConfig,
    provider_name: &str,
    oauth: bool,
    api_key: Option<&str>,
) -> Result<()> {
    let provider = agent_config
        .providers
        .find(provider_name)
        .with_context(|| format!("Provider '{}' not found in configured providers", provider_name))?;

    let store = FileCredentialStore::new(FileCredentialStore::default_path());

    if let Some(key) = api_key {
        // Direct API key login
        let credential = Credential::ApiKey {
            key: key.to_string(),
        };
        store.write(provider_name, credential).await?;
        println!("✅ API key stored for provider '{}'.", provider_name);
        return Ok(());
    }

    if oauth || provider.auth_mode == Some(ProviderAuthMode::OAuth) {
        // OAuth PKCE flow
        let oauth_config = provider
            .oauth
            .as_ref()
            .with_context(|| format!("Provider '{}' does not have OAuth configured", provider_name))?;

        let flow_config = OAuthFlowConfig {
            provider_name: provider.name.clone(),
            provider_id: provider_name.to_string(),
            authorize_url: oauth_config.authorize_url.clone(),
            token_url: oauth_config.token_url.clone(),
            client_id: oauth_config.client_id.clone(),
            scopes: oauth_config.scopes.clone(),
            redirect_uri: oauth_config.redirect_uri.clone(),
        };

        login_oauth(flow_config, &store).await?;
        return Ok(());
    }

    if provider.auth_mode == Some(ProviderAuthMode::Codex) {
        // Codex CLI auth — already handled by resolve_codex_auth()
        // Just verify the auth file exists using the shared codex_home_path()
        let auth_path = xai_grok_config_types::codex_home_path().join("auth.json");
        if !auth_path.exists() {
            anyhow::bail!(
                "Codex auth file not found at {}. Run 'codex login' first.",
                auth_path.display()
            );
        }
        println!("✅ Codex auth found. Provider 'codex' is ready.");
        return Ok(());
    }

    // Default: try API key env var
    if let Some(env_key) = &provider.env_key {
        match std::env::var(env_key) {
            Ok(key) if !key.is_empty() => {
                let credential = Credential::ApiKey { key };
                store.write(provider_name, credential).await?;
                println!(
                    "✅ API key from ${} stored for provider '{}'.",
                    env_key, provider_name
                );
                return Ok(());
            }
            _ => {
                anyhow::bail!(
                    "Environment variable ${} is not set. Set it or use:\n  ghost login {} --api-key <key>\n  ghost login {} --oauth",
                    env_key, provider_name, provider_name
                );
            }
        }
    }

    anyhow::bail!(
        "Provider '{}' has no login method configured. Add api_key, env_key, or oauth config.",
        provider_name
    );
}

/// Log out from a provider by removing its stored credential.
pub async fn logout_provider(agent_config: &AgentConfig, provider_name: Option<&str>) -> Result<()> {
    let store = FileCredentialStore::new(FileCredentialStore::default_path());

    match provider_name {
        Some(name) => {
            // Verify provider exists
            agent_config
                .providers
                .find(name)
                .with_context(|| format!("Provider '{}' not found in configured providers", name))?;
            store.delete(name).await?;
            println!("✅ Logged out from '{}'.", name);
        }
        None => {
            // Logout all providers
            let count = agent_config.providers.providers.len();
            for p in &agent_config.providers.providers {
                store.delete(&p.name).await?;
            }
            println!("✅ Logged out from {} provider(s).", count);
        }
    }
    Ok(())
}

/// Show authentication status for all configured providers.
pub async fn auth_status(agent_config: &AgentConfig) -> Result<()> {
    let store = FileCredentialStore::new(FileCredentialStore::default_path());

    if agent_config.providers.providers.is_empty() {
        println!("No providers configured.");
        println!("Add providers to ~/.ghost/config.toml with [[providers]] sections.");
        return Ok(());
    }

    println!("Provider authentication status:");
    println!();
    println!("  {:<20} {:<12} {}", "PROVIDER", "AUTH", "DETAILS");
    println!("  {:-<20} {:-<12} {:-<40}", "", "", "");

    for p in &agent_config.providers.providers {
        let credential = store.read(&p.name).await?;
        let (status, details) = match credential {
            Some(Credential::ApiKey { ref key }) => {
                let masked = if key.len() > 12 {
                    format!("{}...{}", &key[..4], &key[key.len()-4..])
                } else if key.len() > 4 {
                    format!("{}...", &key[..4])
                } else {
                    "***".to_string()
                };
                ("✓ API key", masked)
            }
            Some(Credential::OAuth { expires_at, .. }) => {
                let expiry_str = expires_at.map_or("unknown".to_string(), |exp| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    if now < exp {
                        let remaining = (exp - now) / 3600;
                        format!("valid ({}h remaining)", remaining)
                    } else {
                        "expired".to_string()
                    }
                });
                ("✓ OAuth", expiry_str)
            }
            None => {
                // Check env var fallback
                if p.has_resolvable_key() {
                    ("✓ env var", format!("via {}", p.env_key.as_deref().unwrap_or("unknown")))
                } else {
                    ("✗ not set", String::new())
                }
            }
        };
        println!("  {:<20} {:<12} {}", p.name, status, details);
    }

    println!();
    println!("Use 'ghost login <provider> --api-key <key>' or '--oauth' to authenticate.");
    Ok(())
}
