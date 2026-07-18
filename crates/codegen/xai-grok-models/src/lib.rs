//! Default model IDs loaded from `default_models.json` at runtime.
//! Edit that JSON file to change them.
//!
//! At runtime each model is resolved via:
//!   CLI flag > ENV var > config.toml > remote settings > these defaults
//!
//! Also exposes the embedded provider catalog for multi-provider setup.

use std::sync::LazyLock;

/// The raw JSON, embedded at compile time. Re-exported through the
/// `xai_grok_shell::models` facade and consumed by `agent::config`, so it must
/// be `pub` (was `pub(crate)` when this lived inside the shell crate).
pub const DEFAULT_MODELS_JSON: &str = include_str!("../default_models.json");

#[derive(serde::Deserialize)]
struct DefaultModels {
    default: String,
    /// Falls back to `default` if not specified in JSON.
    web_search: Option<String>,
    /// Falls back to `default` if not specified in JSON.
    image_description: Option<String>,
    /// Falls back to `default` if not specified in JSON.
    session_summary: Option<String>,
    #[serde(default)]
    providers: Vec<EmbeddedProvider>,
    models: Vec<DefaultModelEntry>,
}

#[derive(serde::Deserialize)]
struct DefaultModelEntry {
    model: String,
}

/// Embedded provider catalog entry from default_models.json.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EmbeddedProvider {
    pub name: String,
    pub api_base: Option<String>,
    pub env_key: Option<String>,
    pub auth_mode: Option<String>,
    pub models: Vec<EmbeddedModelEntry>,
}

/// Embedded model entry within a provider.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EmbeddedModelEntry {
    pub model: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub context_window: Option<u64>,
}

static DEFAULTS: LazyLock<DefaultModels> = LazyLock::new(|| {
    let defaults: DefaultModels = serde_json::from_str(DEFAULT_MODELS_JSON)
        .expect("default_models.json: invalid JSON or missing 'default' field");

    // Baked-in JSON — a mismatch here is a developer error, not a runtime condition.
    let model_ids: Vec<&str> = defaults.models.iter().map(|m| m.model.as_str()).collect();
    assert!(
        model_ids.contains(&defaults.default.as_str()),
        "default_models.json: 'default' is '{}' but 'models' array only has {model_ids:?}",
        defaults.default,
    );

    defaults
});

/// Primary model for coding tasks and general fallback.
pub fn default_model() -> &'static str {
    &DEFAULTS.default
}

/// Model for web search tool synthesis. Falls back to default model.
pub fn default_web_search_model() -> &'static str {
    DEFAULTS.web_search.as_deref().unwrap_or(&DEFAULTS.default)
}

/// Model for image describe. Falls back to default model.
pub fn default_image_description_model() -> &'static str {
    DEFAULTS
        .image_description
        .as_deref()
        .unwrap_or(&DEFAULTS.default)
}

/// Model for session title generation. Falls back to default model.
pub fn default_session_summary_model() -> &'static str {
    DEFAULTS
        .session_summary
        .as_deref()
        .unwrap_or(&DEFAULTS.default)
}

/// All embedded providers from default_models.json.
pub fn embedded_providers() -> &'static [EmbeddedProvider] {
    &DEFAULTS.providers
}

/// Find an embedded provider by name.
pub fn find_embedded_provider(name: &str) -> Option<&EmbeddedProvider> {
    DEFAULTS.providers.iter().find(|p| p.name == name)
}
