//! `grok models` subcommand.

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use xai_grok_shell::agent::config::Config as AgentConfig;
use xai_grok_shell::cli_models::{AuthStatus, list_models};

use crate::client_identity::{PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION};

pub async fn list_available_models(
    agent_config: &AgentConfig,
    provider_filter: Option<&str>,
) -> Result<()> {
    let has_provider_config = !agent_config.providers.providers.is_empty();
    match AuthStatus::resolve(agent_config) {
        AuthStatus::ApiKey => println!("You are using XAI_API_KEY."),
        AuthStatus::LoggedIn(host) => println!("You are logged in with {host}."),
        AuthStatus::ModelCredentials(model) => {
            println!("Model '{model}' is using its own API key.");
        }
        AuthStatus::DeploymentKey => println!("You are authenticated via deployment key."),
        AuthStatus::NotAuthenticated => println!("You are not authenticated."),
    }
    println!();

    // Display configured providers if present
    if has_provider_config {
        if let Some(filter) = provider_filter {
            let provider = agent_config.providers.find(filter);
            if provider.is_none() {
                eprintln!(
                    "Provider '{}' not found in configured providers. Available: {}",
                    filter,
                    agent_config
                        .providers
                        .providers
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Ok(());
            }
            println!("Models for provider '{}':", filter);
        } else {
            println!("Configured providers:");
            for p in &agent_config.providers.providers {
                println!("  {} — {}", p.name, p.api_base.as_deref().unwrap_or("(no api_base)"));
            }
            println!();
        }
    }

    let cancel = CancellationToken::new();
    let spawned = crate::acp::spawn::spawn_grok_shell(agent_config.clone(), &cancel, None).await?;

    let state = list_models(&spawned.channel.tx, PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION).await?;

    println!("Default model: {}", state.current_model_id.0);
    println!();
    println!("Available models:");
    for m in state.available_models {
        // Apply provider filter if specified
        if let Some(filter) = provider_filter {
            // Check if this model belongs to the filtered provider
            let model_belongs = agent_config
                .providers
                .find(filter)
                .map(|p| p.models.iter().any(|s| s.as_str() == m.model_id.0.as_ref()))
                .unwrap_or(false);
            if !model_belongs {
                continue;
            }
        }
        if m.model_id == state.current_model_id {
            println!("  * {} (default)", m.model_id.0);
        } else {
            println!("  - {}", m.model_id.0);
        }
    }

    cancel.cancel();
    Ok(())
}
