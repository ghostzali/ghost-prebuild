//! Provider health checks (Phase 6.1 stub).
//!
//! When fully implemented, pings each provider's API endpoint on startup
//! and marks unreachable providers. Currently a placeholder for the
//! architecture described in ROADMAP.md.

/// Health status of a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderHealth {
    /// Provider is reachable and responding.
    Healthy,
    /// Provider was unreachable or timed out.
    Unhealthy { reason: String },
    /// Not yet checked.
    Unknown,
}

/// Check a provider's health by pinging its base URL.
/// Returns `Unknown` (not yet implemented — stub for Phase 6.1).
pub async fn check_provider_health(_base_url: &str, _api_key: &str) -> ProviderHealth {
    // TODO(phase6): Implement actual health check with timeout
    // - GET {base_url}/v1/models with 5s timeout
    // - Mark Healthy on 200, Unhealthy on error/timeout
    ProviderHealth::Unknown
}
