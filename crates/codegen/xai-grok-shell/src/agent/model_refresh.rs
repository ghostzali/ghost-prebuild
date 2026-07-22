//! Provider model refresh pipeline.
//!
//! When a provider has a configured API key, models can be fetched from
//! its `/v1/models` endpoint and merged into a dynamic overlay.
//! Mirrors Pi's `Provider::refreshModels()` pattern.

use std::collections::HashMap;
use tracing;

/// Result of a provider model refresh.
#[derive(Debug)]
pub struct RefreshResult {
    /// Models fetched (or cached) for this provider.
    pub models: Vec<String>,
    /// Whether this was a cache hit (no network request).
    pub from_cache: bool,
    /// Any error that occurred during fetch.
    pub error: Option<String>,
}

/// Refresh models for a set of providers.
///
/// Each provider with a resolvable API key gets its `/v1/models` endpoint
/// queried. Results are persisted to the model store for offline use.
///
/// Cache: if the store has records newer than `cache_ttl_secs`, use them
/// without a network request.
pub async fn refresh_provider_models(
    providers: &[(&str, &str, &str)], // (id, base_url, api_key)
    store: &dyn xai_grok_auth::model_store::ModelStore,
    cache_ttl_secs: u64,
) -> HashMap<String, RefreshResult> {
    let mut results = HashMap::new();

    for (id, base_url, api_key) in providers {
        // Check cache first
        if let Some(cached) = store.read(id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now.saturating_sub(cached.checked_at) < cache_ttl_secs {
                tracing::debug!(
                    provider = %id,
                    "Using cached models ({} models, {}s old)",
                    cached.models.len(),
                    now - cached.checked_at
                );
                results.insert(
                    id.to_string(),
                    RefreshResult {
                        models: cached.models,
                        from_cache: true,
                        error: None,
                    },
                );
                continue;
            }
        }

        // Fetch from provider API
        match xai_grok_auth::models_api::fetch_models(base_url, api_key).await {
            Ok(raw_models) => {
                let models: Vec<String> = raw_models.into_iter().map(|m| m.id).collect();
                tracing::info!(
                    provider = %id,
                    count = models.len(),
                    "Fetched models from provider"
                );

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                store.write(
                    id,
                    xai_grok_auth::model_store::StoredModels {
                        models: models.clone(),
                        checked_at: now,
                    },
                );

                results.insert(
                    id.to_string(),
                    RefreshResult {
                        models,
                        from_cache: false,
                        error: None,
                    },
                );
            }
            Err(e) => {
                tracing::warn!(
                    provider = %id,
                    error = %e,
                    "Failed to refresh models; using cached if available"
                );
                let cached = store.read(id);
                let from_cache = cached.is_some();
                results.insert(
                    id.to_string(),
                    RefreshResult {
                        models: cached.map(|c| c.models).unwrap_or_default(),
                        from_cache,
                        error: Some(e.to_string()),
                    },
                );
            }
        }
    }

    results
}
