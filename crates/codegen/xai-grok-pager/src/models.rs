//! `ghost models` subcommand.

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use xai_grok_shell::agent::config::Config as AgentConfig;
use xai_grok_shell::cli_models::{AuthStatus, list_models};

use crate::client_identity::{PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION};

pub async fn list_available_models(
    agent_config: &AgentConfig,
    provider_filter: Option<&str>,
) -> Result<()> {
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

    // Resolve provider filter once
    let provider = provider_filter
        .and_then(|filter| {
            let p = agent_config.providers.find(filter);
            if p.is_none() {
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
            }
            p
        });

    if let Some(p) = provider {
        println!("Models for provider '{}':", p.name);
        if p.api_base.is_none() {
            println!("  (no api_base configured)");
        }
        println!();
    }

    let cancel = CancellationToken::new();
    let spawned = crate::acp::spawn::spawn_grok_shell(agent_config.clone(), &cancel, None).await?;

    let state = list_models(&spawned.channel.tx, PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION).await?;

    println!("Default model: {}", state.current_model_id.0);
    println!();
    println!("Available models:");
    for m in state.available_models {
        // Apply provider filter if specified
        if let Some(p) = provider {
            let model_belongs = p.models.iter().any(|s| s.as_str() == m.model_id.0.as_ref());
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
