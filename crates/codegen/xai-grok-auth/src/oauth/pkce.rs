//! OAuth 2.0 PKCE (Proof Key for Code Exchange) — RFC 7636.
//!
//! Used for desktop OAuth flows where a client secret can't be securely stored.
//! Generates a `code_verifier` (random 43-128 char string) and derives a
//! `code_challenge` (SHA-256 hash, base64url-encoded).

use base64::Engine;
use rand::Rng;

/// A PKCE pair: the secret verifier and its derived challenge.
pub struct PkcePair {
    /// Sent in the token exchange request (`code_verifier`).
    pub code_verifier: String,
    /// Sent in the authorization request (`code_challenge`).
    pub code_challenge: String,
}

/// Generate a PKCE code_verifier + code_challenge pair.
pub fn generate_pkce() -> PkcePair {
    // 32 random bytes → 43 base64url chars (within 43-128 spec range)
    let mut bytes = [0u8; 32];
    rand::rng().try_fill_bytes(&mut bytes).expect("RNG failure");
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    // SHA-256(code_verifier) → base64url
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let digest = hasher.finalize();
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    PkcePair {
        code_verifier,
        code_challenge,
    }
}

/// Build the authorization URL for the OAuth PKCE flow.
pub fn build_authorize_url(
    authorize_url: &str,
    client_id: &str,
    redirect_uri: &str,
    scopes: Option<&str>,
    code_challenge: &str,
    state: &str,
) -> String {
    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        authorize_url, client_id,
        urlencoding(redirect_uri),
        code_challenge,
        state,
    );
    if let Some(scope) = scopes {
        url.push_str("&scope=");
        url.push_str(&urlencoding(scope));
    }
    url
}

/// URL-encode a value for query parameters (only the characters that need escaping).
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}
