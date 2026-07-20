//! OAuth 2.0 authorization code flow with PKCE.
//!
//! Desktop OAuth for CLI apps:
//! 1. Generate PKCE pair + state nonce
//! 2. Open browser to provider's authorize URL
//! 3. Start local HTTP server on a random port to catch the redirect
//! 4. Exchange authorization code for access + refresh tokens
//! 5. Store in credential store

use crate::credential_store::{Credential, CredentialStore};
use crate::oauth::pkce::{self};
use anyhow::{Context, Result};
use base64::Engine;
use serde::Deserialize;
use std::net::TcpListener;
use tracing;
use urlencoding;

/// Configuration for an OAuth PKCE flow.
pub struct OAuthFlowConfig {
    pub provider_name: String,
    pub provider_id: String,
    pub authorize_url: String,
    pub token_url: String,
    pub client_id: String,
    pub scopes: Option<String>,
    pub redirect_uri: Option<String>,
}

/// Response from the token endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    token_type: Option<String>,
}

/// Run the OAuth PKCE login flow and store the resulting credential.
///
/// Returns the stored credential on success.
pub async fn login_oauth<Store: CredentialStore + ?Sized>(
    config: OAuthFlowConfig,
    store: &Store,
) -> Result<Credential> {
    // 1. Generate PKCE pair and state nonce
    let pkce = pkce::generate_pkce();
    let state = generate_state();

    // 2. Find a free port and set up redirect URI
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("Failed to bind local port for OAuth callback")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = config
        .redirect_uri
        .unwrap_or_else(|| format!("http://localhost:{}/callback", port));

    // 3. Build and open authorization URL
    let auth_url = pkce::build_authorize_url(
        &config.authorize_url,
        &config.client_id,
        &redirect_uri,
        config.scopes.as_deref(),
        &pkce.code_challenge,
        &state,
    );

    tracing::info!(
        provider = %config.provider_id,
        "Opening browser for OAuth login: {}",
        config.provider_name
    );
    open_browser(&auth_url);

    println!();
    println!("🔑 Logging in to {}...", config.provider_name);
    println!("   If your browser doesn't open, visit:");
    println!("   {}", auth_url);
    println!();

    // 4. Wait for the callback (with 5-minute timeout)
    let listener = listener;
    let code = receive_callback(listener, &state, &redirect_uri).await?;

    // 5. Exchange code for tokens
    tracing::info!(provider = %config.provider_id, "Exchanging authorization code for tokens");
    let token = exchange_code(
        &config.token_url,
        &config.client_id,
        &code,
        &pkce.code_verifier,
        &redirect_uri,
    )
    .await?;

    // 6. Build credential and store
    let expires_at = token.expires_in.map(|secs| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + secs
    });

    let credential = Credential::OAuth {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at,
    };

    store.write(&config.provider_id, credential.clone()).await?;

    println!("✅ Successfully logged in to {}!", config.provider_name);
    Ok(credential)
}

/// Generate a random state nonce for CSRF protection.
fn generate_state() -> String {
    let bytes: [u8; 16] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Open the authorization URL in the user's default browser.
fn open_browser(url: &str) {
    match webbrowser::open(url) {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("Failed to open browser: {e}");
        }
    }
}

/// Wait for the OAuth callback on the bound TCP listener.
/// Loops accepting connections until a state-matching callback arrives
/// (handles Chrome pre-connect probes) or the 5-minute deadline fires.
async fn receive_callback(listener: TcpListener, expected_state: &str, _redirect_uri: &str) -> Result<String> {
    let (code_sender, code_receiver) = tokio::sync::oneshot::channel::<String>();
    let state = expected_state.to_string();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);

    let listener = std::sync::Arc::new(listener);
    tokio::spawn(async move {
        loop {
            let remaining = deadline
                .checked_duration_since(tokio::time::Instant::now())
                .unwrap_or_default();
            if remaining.is_zero() {
                return;
            }

            let l = listener.clone();
            let accept_result = tokio::time::timeout(remaining, async {
                tokio::task::spawn_blocking(move || l.accept()).await
            }).await;

            let mut stream = match accept_result {
                Ok(Ok(Ok((s, _)))) => s,
                Ok(Ok(Err(e))) => {
                    tracing::warn!("OAuth callback accept error: {e}");
                    continue;
                }
                _ => return, // timeout or join error
            };

            use std::io::{BufRead, BufReader, Write};
            let mut reader = BufReader::new(&mut stream);
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }

            // Parse the GET request: "GET /callback?code=...&state=... HTTP/1.1"
            let query = match request_line.find("?") {
                Some(qs) => {
                    let qe = request_line[qs..].find(" HTTP").unwrap_or(request_line.len() - qs);
                    &request_line[qs + 1..qs + qe]
                }
                None => {
                    let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
                        <html><body><h1>❌ Bad request</h1></body></html>";
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }
            };

            let params: Vec<(String, String)> = query
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    Some((
                        parts.next()?.to_string(),
                        parts.next().unwrap_or("").to_string(),
                    ))
                })
                .collect();

            let received_state = params
                .iter()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| urlencoding::decode(v).unwrap_or_default().into_owned());
            let code = params
                .iter()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| urlencoding::decode(v).unwrap_or_default().into_owned());

            if received_state.as_deref() == Some(&state)
                && let Some(c) = code
            {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                    <html><body><h1>✅ Login successful!</h1>\
                    <p>You may close this window and return to the terminal.</p>\
                    </body></html>";
                let _ = stream.write_all(response.as_bytes());
                let _ = code_sender.send(c);
                return;
            }

            // State mismatch or missing — send error and continue looping
            let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
                <html><body><h1>❌ Login failed</h1>\
                <p>Invalid callback. Please try again.</p>\
                </body></html>";
            let _ = stream.write_all(response.as_bytes());
        }
    });

    match tokio::time::timeout(std::time::Duration::from_secs(300), code_receiver).await {
        Ok(Ok(code)) => Ok(code),
        Ok(Err(_)) => anyhow::bail!("OAuth callback channel closed unexpectedly"),
        Err(_) => anyhow::bail!("OAuth login timed out — no response within 5 minutes"),
    }
}

/// Exchange an authorization code for access + refresh tokens.
async fn exchange_code(
    token_url: &str,
    client_id: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", redirect_uri),
    ];

    let response = client
        .post(token_url)
        .form(&params)
        .header("Accept", "application/json")
        .send()
        .await
        .context("Failed to send token exchange request")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Token exchange failed ({}): {}",
            status,
            body.chars().take(500).collect::<String>()
        );
    }

    let token: TokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    Ok(token)
}
