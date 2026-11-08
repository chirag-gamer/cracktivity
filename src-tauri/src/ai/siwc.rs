//! Sign in with ChatGPT (FR-24 extension, D-047): lets a user connect their
//! ChatGPT plan to power AI features without an API key. Implements the
//! documented open-source client contract (developers.openai.com,
//! /siwc/token-sharing-open-source, verified 2026-11-08):
//!
//! - Dynamic client registration: first sign-in sends
//!   `client_id=dynamic_agent_client` plus `agent_name_hint` and the
//!   persisted `ext_agent_host_id`; the callback returns the issued
//!   `oaiapp_...` client id which is stored and reused (never
//!   `dynamic_agent_client`).
//! - Loopback redirect on `127.0.0.1:<ephemeral>` with path `/auth/callback`
//!   (RFC 8252; `localhost` is not allowed by the contract).
//! - Scopes: `openid profile email offline_access resource.invoke
//!   chatgpt.tokens.use.direct`, resource `https://api.openai.com/v1`.
//! - Token exchange at `https://auth.openai.com/api/accounts/oauth/token`,
//!   form-encoded, no client secret.
//! - Inference: `POST https://api.openai.com/v1/responses` with the OAuth
//!   access token, `store: false`, `stream: true`.
//!
//! Only `auth.openai.com` and `api.openai.com` are contacted here (D-015;
//! both are in the netlog inventory). Tokens live in Credential Manager
//! (FR-2) under `cracktivity.chatgpt.*` targets, never in settings.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use base64::Engine;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use url::Url;

/// Authorize + token endpoints (production OIDC discovery values,
/// developers.openai.com/siwc/website).
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
/// The resource indicator for plan-backed Responses API calls.
pub const RESOURCE: &str = "https://api.openai.com/v1";
pub const MODELS_URL: &str = "https://api.openai.com/v1/models";
/// First-sign-in registration entrypoint (not a usable client id itself).
pub const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
pub const APP_NAME_HINT: &str = "cracktivity";
pub const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
pub const CALLBACK_PATH: &str = "/auth/callback";

pub const TARGET_CLIENT_ID: &str = "cracktivity.chatgpt.client_id";
pub const TARGET_ACCESS: &str = "cracktivity.chatgpt.access";
pub const TARGET_REFRESH: &str = "cracktivity.chatgpt.refresh";
pub const TARGET_MODEL: &str = "cracktivity.chatgpt.model";
pub const TARGET_EMAIL: &str = "cracktivity.chatgpt.email";
pub const TARGET_EXPIRES: &str = "cracktivity.chatgpt.expires";

#[derive(Debug, thiserror::Error)]
pub enum SiwcError {
    #[error("sign-in was cancelled or timed out")]
    FlowCancelled,
    #[error("state mismatch: the callback does not match this attempt")]
    StateMismatch,
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("randomness failure: {0}")]
    Random(#[from] getrandom::Error),
    #[error("{0}")]
    Other(String),
}

/// What one successful sign-in leaves behind.
pub struct Connected {
    pub email: String,
    pub model: String,
}

/// PKCE pair: verifier + S256 challenge (RFC 7636 section 4.2, unpadded
/// base64url of the SHA-256 digest).
struct Pkce {
    verifier: String,
    challenge: String,
}

fn pkce() -> Result<Pkce, SiwcError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    Ok(Pkce {
        verifier,
        challenge,
    })
}

fn random_hex(n: usize) -> Result<String, SiwcError> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn http_post_form(url: &str, body: &str) -> Result<serde_json::Value, SiwcError> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    crate::platform::netlog::record_url(url);
    let response = agent
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body)
        .map_err(|e| SiwcError::Exchange(e.to_string()))?;
    let status = response.status().as_u16();
    let mut text = String::new();
    response
        .into_body()
        .into_reader()
        .read_to_string(&mut text)
        .map_err(SiwcError::Io)?;
    if status != 200 {
        return Err(SiwcError::Exchange(format!("HTTP {status}: {text}")));
    }
    serde_json::from_str(&text).map_err(|e| SiwcError::Exchange(format!("bad JSON: {e}")))
}

fn open_browser(url: &str) -> Result<(), SiwcError> {
    // Windows-only app (README): `start` with an empty title keeps URLs with
    // `&` intact. No shell plugin dep needed for one call.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map(|_| ())
            .map_err(SiwcError::Io)
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err(SiwcError::Other("browser sign-in is Windows-only".into()))
    }
}

/// One authorization attempt on a pre-bound loopback listener. The returned
/// URL is the full callback (query includes `code`, `state`, and, on first
/// registration, the issued `client_id`).
fn wait_callback(listener: TcpListener, expected_state: &str) -> Result<Url, SiwcError> {
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    // 5-minute deadline (sign-in can require MFA); the socket closes on drop.
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    let (mut conn, _) = loop {
        if std::time::Instant::now() > deadline {
            return Err(SiwcError::FlowCancelled);
        }
        match listener.accept() {
            Ok(c) => break c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(SiwcError::Io(e)),
        }
    };
    listener.set_nonblocking(false).ok();
    conn.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buf = [0u8; 8192];
    let n = conn.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]).to_string();
    let target = request
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string();
    let mut respond = |body: &str| {
        let _ = conn.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .as_bytes(),
        );
    };
    if target.starts_with(CALLBACK_PATH) {
        let url = Url::parse(&format!("http://127.0.0.1:{port}{target}"))
            .map_err(|e| SiwcError::Other(e.to_string()))?;
        if url.query_pairs().any(|(k, _)| k == "error") {
            respond(
                "<html><body><h3>Sign-in failed. You can close this window.</h3></body></html>",
            );
            return Err(SiwcError::FlowCancelled);
        }
        let state_ok = url
            .query_pairs()
            .any(|(k, v)| k == "state" && v == expected_state);
        if !state_ok {
            respond(
                "<html><body><h3>Sign-in failed. You can close this window.</h3></body></html>",
            );
            return Err(SiwcError::StateMismatch);
        }
        respond("<html><body><h3>Signed in. Return to cracktivity.</h3></body></html>");
        return Ok(url);
    }
    respond("<html><body>Waiting for the sign-in redirect...</body></html>");
    Err(SiwcError::FlowCancelled)
}
pub struct Stored {
    pub client_id: String,
    pub access: SecretString,
    pub model: String,
    pub email: String,
    pub expires_at: i64,
}

pub fn load_stored() -> Option<Stored> {
    let client_id = crate::platform::providers::get_account_secret(TARGET_CLIENT_ID).ok()?;
    let access = crate::platform::providers::get_account_secret(TARGET_ACCESS).ok()?;
    let model = crate::platform::providers::get_account_secret(TARGET_MODEL).ok()?;
    let email = crate::platform::providers::get_account_secret(TARGET_EMAIL).ok()?;
    let expires_at = crate::platform::providers::get_account_secret(TARGET_EXPIRES)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    Some(Stored {
        client_id,
        access: SecretString::from(access),
        model,
        email,
        expires_at,
    })
}

pub fn disconnect() {
    for t in [
        TARGET_CLIENT_ID,
        TARGET_ACCESS,
        TARGET_REFRESH,
        TARGET_MODEL,
        TARGET_EMAIL,
        TARGET_EXPIRES,
    ] {
        let _ = crate::platform::providers::delete_account_secret(t);
    }
}

/// True when the stored access token is missing or expires within 60s.
pub fn needs_refresh(stored: &Stored) -> bool {
    let now = chrono::Utc::now().timestamp();
    stored.expires_at - 60 <= now
}

/// Refresh-token grant. Rotated refresh tokens replace the stored one.
pub fn refresh(stored: &Stored) -> Result<(), SiwcError> {
    let refresh_token = crate::platform::providers::get_account_secret(TARGET_REFRESH)
        .map_err(|_| SiwcError::Other("no refresh token stored".into()))?;
    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}&scope={}",
        urlencoding(&stored.client_id),
        urlencoding(&refresh_token),
        urlencoding(SCOPES),
    );
    let v = http_post_form(TOKEN_URL, &body)?;
    store_tokens(&v).map_err(SiwcError::Other)
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("v", s)
        .finish()
        .trim_start_matches("v=")
        .to_string()
}

fn store_tokens(v: &serde_json::Value) -> Result<(), String> {
    let access = v["access_token"].as_str().unwrap_or_default();
    let refresh = v["refresh_token"].as_str().unwrap_or_default();
    let expires = v["expires_in"].as_u64().unwrap_or(3600);
    if access.is_empty() {
        return Err("token response had no access_token".into());
    }
    crate::platform::providers::set_account_secret(TARGET_ACCESS, access)
        .map_err(|e| e.to_string())?;
    if !refresh.is_empty() {
        // Refresh tokens rotate (Outlook lesson, oauth.rs): replace when the
        // server returns one; keep the old otherwise.
        crate::platform::providers::set_account_secret(TARGET_REFRESH, refresh)
            .map_err(|e| e.to_string())?;
    }
    let expires_at = chrono::Utc::now().timestamp() + expires as i64;
    crate::platform::providers::set_account_secret(TARGET_EXPIRES, &expires_at.to_string())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Runs the full browser sign-in: dynamic registration on first use, code
/// exchange, default-model discovery. Returns the connected account email.
/// Blocking (multi-minute, user-paced); call from a worker thread.
pub fn connect() -> Result<Connected, SiwcError> {
    let saved_client = crate::platform::providers::get_account_secret(TARGET_CLIENT_ID).ok();
    // Host id: one random id per install, persisted with the registration
    // (the docs require the same id for the same host on later sign-ins).
    let host_id = crate::platform::providers::get_account_secret("cracktivity.chatgpt.host_id")
        .unwrap_or_else(|_| random_hex(16).expect("host id entropy"));
    let _ = crate::platform::providers::set_account_secret("cracktivity.chatgpt.host_id", &host_id);

    let pkce = pkce()?;
    let state = random_hex(16)?;
    let nonce = random_hex(16)?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();

    let client_id_for_url = saved_client
        .clone()
        .unwrap_or_else(|| DYNAMIC_CLIENT.into());
    let mut url = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}&resource={}&state={}&nonce={}&code_challenge_method=S256&code_challenge={}",
        AUTHORIZE_URL,
        urlencoding(&client_id_for_url),
        urlencoding(&format!("http://127.0.0.1:{port}{CALLBACK_PATH}")),
        urlencoding(SCOPES),
        urlencoding(RESOURCE),
        urlencoding(&state),
        urlencoding(&nonce),
        urlencoding(&pkce.challenge),
    );
    if saved_client.is_none() {
        // Registration-only params (docs: initial dynamic requests only).
        url.push_str(&format!(
            "&agent_name_hint={}&ext_agent_host_id={}",
            urlencoding(APP_NAME_HINT),
            urlencoding(&host_id),
        ));
    }
    open_browser(&url)?;

    let callback = wait_callback(listener, &state)?;
    let code = callback
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .ok_or(SiwcError::FlowCancelled)?;
    // New registration returns the issued id; keep it and never save
    // `dynamic_agent_client` itself.
    let issued = callback
        .query_pairs()
        .find(|(k, _)| k == "client_id")
        .map(|(_, v)| v.to_string())
        .or(saved_client)
        .ok_or(SiwcError::Other(
            "registration callback missing the issued client id".into(),
        ))?;
    if issued == DYNAMIC_CLIENT {
        return Err(SiwcError::Other(
            "registration callback returned the dynamic entrypoint as client id".into(),
        ));
    }
    crate::platform::providers::set_account_secret(TARGET_CLIENT_ID, &issued)
        .map_err(|e| SiwcError::Other(e.to_string()))?;

    let body = format!(
        "grant_type=authorization_code&client_id={}&code={}&code_verifier={}&redirect_uri={}&resource={}",
        urlencoding(&issued),
        urlencoding(&code),
        urlencoding(&pkce.verifier),
        urlencoding(&format!("http://127.0.0.1:{port}{CALLBACK_PATH}")),
        urlencoding(RESOURCE),
    );
    let v = http_post_form(TOKEN_URL, &body)?;
    store_tokens(&v).map_err(SiwcError::Other)?;

    let email = id_token_email(&v).unwrap_or_default();
    if !email.is_empty() {
        crate::platform::providers::set_account_secret(TARGET_EMAIL, &email)
            .map_err(|e| SiwcError::Other(e.to_string()))?;
    }

    let model = pick_model(v["access_token"].as_str().unwrap_or_default())?;
    crate::platform::providers::set_account_secret(TARGET_MODEL, &model)
        .map_err(|e| SiwcError::Other(e.to_string()))?;

    Ok(Connected { email, model })
}

/// Email claim from the id_token payload. The JWT arrives directly over TLS
/// from the documented token endpoint; full JWKS signature validation is a
/// future hardening task, noted in decisions.md (D-047).
fn id_token_email(v: &serde_json::Value) -> Option<String> {
    let jwt = v["id_token"].as_str()?;
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims["email"].as_str().map(|s| s.to_string())
}

/// Default model: the docs' list endpoint, first list-visible slug. The
/// choice is stored and reused; users re-run sign-in to re-pick.
fn pick_model(access: &str) -> Result<String, SiwcError> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    crate::platform::netlog::record_url(MODELS_URL);
    let response = agent
        .get(MODELS_URL)
        .header("Authorization", &format!("Bearer {access}"))
        .call()
        .map_err(|e| SiwcError::Other(format!("model list failed: {e}")))?;
    let mut text = String::new();
    response
        .into_body()
        .into_reader()
        .read_to_string(&mut text)
        .map_err(SiwcError::Io)?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| SiwcError::Other(format!("bad JSON: {e}")))?;
    for m in v["data"].as_array().into_iter().flatten() {
        if m["visibility"].as_str() == Some("list") {
            if let Some(slug) = m["slug"].as_str() {
                return Ok(slug.to_string());
            }
        }
    }
    Err(SiwcError::Other(
        "no list-visible model on this ChatGPT plan".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_openai_owned() {
        assert_eq!(
            Url::parse(AUTHORIZE_URL).unwrap().host_str(),
            Some("auth.openai.com")
        );
        assert_eq!(
            Url::parse(TOKEN_URL).unwrap().host_str(),
            Some("auth.openai.com")
        );
        assert_eq!(
            Url::parse(RESOURCE).unwrap().host_str(),
            Some("api.openai.com")
        );
    }

    #[test]
    fn scopes_request_plan_usage() {
        assert!(SCOPES.contains("chatgpt.tokens.use.direct"));
        assert!(SCOPES.contains("offline_access"));
    }

    #[test]
    fn callback_path_is_loopback_relative() {
        assert!(CALLBACK_PATH.starts_with('/'));
        assert!(!CALLBACK_PATH.contains("localhost"));
    }
}
