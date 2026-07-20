//! OpenAI-compatible `/v1/models` API client.
//!
//! Used by dynamic providers to fetch their model list at runtime.

use serde::Deserialize;

/// Raw model info from a provider's `/v1/models` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct RawModel {
    /// Model identifier (e.g. "gpt-4", "claude-3-opus").
    pub id: String,
    /// Object type (always "model" for OpenAI-compatible).
    #[serde(default)]
    pub object: Option<String>,
    /// Owned by provider (e.g. "openai").
    #[serde(default)]
    pub owned_by: Option<String>,
}

/// Response from `/v1/models`.
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    pub data: Vec<RawModel>,
}

/// Fetch models from an OpenAI-compatible API.
pub async fn fetch_models(
    base_url: &str,
    api_key: &str,
) -> Result<Vec<RawModel>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let url = if base_url.ends_with('/') {
        format!("{}v1/models", base_url)
    } else {
        format!("{}/v1/models", base_url)
    };

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch models ({}): {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }

    let body: ModelsResponse = response.json().await?;
    Ok(body.data)
}
