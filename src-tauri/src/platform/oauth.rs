//! Outlook personal-account OAuth2 (spec §5, FR-3): system-browser
//! authorization-code flow with PKCE against a loopback redirect, plus the
//! refresh flow. Only `login.microsoftonline.com` is ever contacted here
//! (D-015 allowlist).
//!
//! Facts below are verified against Microsoft's Entra documentation
//! (2026-11-04):
//! - Endpoints: `login.microsoftonline.com/consumers/oauth2/v2.0/{authorize,token}`
//!   — the `consumers` tenant is personal accounts only.
//! - Scopes: `offline_access https://outlook.office.com/IMAP.AccessAsUser.All
//!   https://outlook.office.com/SMTP.Send`. The `outlook.office365.com`
//!   scope variant is a documented common failure; do not use it.
//! - Loopback redirect: `http://127.0.0.1/<port>` — the port is ignored when
//!   the Entra app matches loopback reply URLs, so one registration covers
//!   every ephemeral port. Microsoft prefers 127.0.0.1 over localhost; the
//!   URI must be registered through the app manifest (`replyUrlsWithType`)
//!   because the portal's text box rejects `http://` + IP.
//! - Refresh tokens rotate: every refresh returns a new `refresh_token`
//!   which replaces the stored one; the old one is not server-revoked but
//!   is discarded.
//! - PKCE is recommended (not mandated) for native clients; we always send it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use base64::Engine;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("Outlook OAuth is not configured: set OUTLOOK_CLIENT_ID in src/platform/oauth.rs to the project's Entra app registration id")]
    ClientIdMissing,
    #[error("authorization was rejected or timed out")]
    FlowCancelled,
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("randomness failure: {0}")]
    Random(#[from] getrandom::Error),
}

/// The Entra app registration's client id, owned by the cracktivity project
/// (spec §5). A client id is public data, not a secret; it must still exist
/// for the flow to run. Until the registration is created and its id pasted
/// here, every Outlook OAuth attempt fails with a clear, actionable error.
pub const OUTLOOK_CLIENT_ID: &str = "";

/// Auth scope set for IMAP + SMTP + offline refresh (verified, see module
/// docs). One string, space-separated, exactly as the authorize URL wants.
pub const SCOPES: &str =
    "offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send";

const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

#[derive(Debug)]
pub struct OAuthTokens {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    /// Seconds until `access_token` expires (server-reported).
    pub expires_in: u64,
}

/// The loopback server for one authorization attempt. Binds
/// `127.0.0.1:<ephemeral>`; the port goes into the authorize URL's
/// `redirect_uri`, which Entra matches ignoring the port.
pub struct Loopback {
    listener: TcpListener,
    pub port: u16,
}

impl Loopback {
    pub fn bind() -> Result<Self, OAuthError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    /// The redirect_uri for this attempt: `http://127.0.0.1:<port>` with the
    /// port in the PORT component (D-017). Entra matches loopback reply URLs
    /// ignoring only the port, so the path must be empty.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// Blocks up to `timeout` for the browser's redirect GET and extracts the
    /// `code` parameter, requiring `state` to equal `expected_state` (RFC
    /// 8252: state binds the redirect to this attempt; PKCE binds the code to
    /// this client). Answers the browser with a minimal completion page
    /// either way.
    pub fn wait_for_code(
        &self,
        timeout: Duration,
        expected_state: &str,
    ) -> Result<String, OAuthError> {
        // Non-blocking accept polled against the deadline: TcpListener has
        // no accept timeout, and a 50 ms poll costs nothing.
        self.listener.set_nonblocking(true)?;
        let deadline = std::time::Instant::now() + timeout;
        let (mut stream, _) = loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(OAuthError::FlowCancelled);
            }
            match self.listener.accept() {
                Ok(pair) => break pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e.into()),
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        // One request is all a redirect produces; read until the header
        // block ends (browsers close or the body is empty for GET).
        let mut buf = vec![0u8; 8192];
        let mut raw = String::new();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            raw.push_str(&String::from_utf8_lossy(&buf[..n]));
            if raw.contains("\r\n\r\n") || raw.len() > 16 * 1024 {
                break;
            }
        }

        let result = extract_query_param(&raw, "code")
            .filter(|c| !c.is_empty())
            .filter(|_| {
                // State mismatch (or absence) means this redirect does not
                // belong to the attempt that generated the authorize URL.
                extract_query_param(&raw, "state")
                    .is_some_and(|s| constant_time_eq(&s, expected_state))
            })
            .ok_or(OAuthError::FlowCancelled);
        let page = match &result {
            Ok(_) => "<html><body><p>cracktivity is signed in. You can close this tab.</p></body></html>",
            Err(_) => "<html><body><p>cracktivity sign-in failed or was cancelled. You can close this tab.</p></body></html>",
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            page.len(),
            page
        );
        let _ = stream.write_all(response.as_bytes());
        result
    }
}

/// Pulls one query parameter out of the request line of a raw HTTP request.
fn extract_query_param(raw_request: &str, name: &str) -> Option<String> {
    let request_line = raw_request.lines().next()?;
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == name {
            // Form-urlencoded rules: `+` is a space, %XX is a raw byte.
            return percent_decode(v);
        }
    }
    None
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// 128 bits of entropy as a 43-character base64url verifier (RFC 7636 §4.1
/// minimum), then S256 challenge.
pub fn pkce_pair() -> Result<(String, String), OAuthError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let challenge = pkce_challenge(&verifier);
    Ok((verifier, challenge))
}

/// S256 challenge of a verifier: BASE64URL(SHA256(verifier)).
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// One-time `state` token binding the browser redirect to this attempt
/// (RFC 8252 §4.4.1): 128 bits, base64url, 22 characters.
pub fn state_token() -> Result<String, OAuthError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Byte-wise equality in constant time for short tokens.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The authorize URL to open in the system browser.
pub fn authorize_url(
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> Result<String, OAuthError> {
    if OUTLOOK_CLIENT_ID.is_empty() {
        return Err(OAuthError::ClientIdMissing);
    }
    build_authorize_url(OUTLOOK_CLIENT_ID, redirect_uri, code_challenge, state)
}

/// Pure URL construction, split from the client-id guard so tests can pin
/// the exact query parameters without depending on the registration.
fn build_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> Result<String, OAuthError> {
    let mut url = Url::parse(AUTHORIZE_URL).expect("authorize endpoint is a valid URL");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_mode", "query")
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

/// Exchanges the authorization code for the initial token pair.
pub fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokens, OAuthError> {
    let form = [
        ("client_id", OUTLOOK_CLIENT_ID.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("code_verifier", verifier.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
    ];
    parse_token_response(post_token_endpoint(&form)?)
}

/// Refreshes an access token. Returns the NEW refresh token (rotation:
/// Microsoft replaces the token on every use; the caller must store the
/// replacement, discarding the old one).
pub fn refresh(refresh_token: &SecretString) -> Result<OAuthTokens, OAuthError> {
    if OUTLOOK_CLIENT_ID.is_empty() {
        return Err(OAuthError::ClientIdMissing);
    }
    let form = [
        ("client_id", OUTLOOK_CLIENT_ID.to_string()),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.expose_secret().to_string()),
        ("scope", SCOPES.to_string()),
    ];
    parse_token_response(post_token_endpoint(&form)?)
}

/// POSTs the form to the token endpoint and returns the raw JSON body.
/// Split out from parsing so tests can cover parsing without network.
fn post_token_endpoint(form: &[(&str, String)]) -> Result<String, OAuthError> {
    if OUTLOOK_CLIENT_ID.is_empty() {
        return Err(OAuthError::ClientIdMissing);
    }
    crate::platform::netlog::record_url(TOKEN_URL);
    let response = ureq::post(TOKEN_URL)
        .send_form(form.iter().map(|(k, v)| (*k, v.as_str())))
        .map_err(|e| OAuthError::Exchange(e.to_string()))?;
    let mut response = response;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| OAuthError::Exchange(e.to_string()))?;
    Ok(body)
}

/// Minimal JSON field extraction for the flat token response (no serde
/// struct needed; the response is a flat JSON object with string/number
/// values, no nesting). Avoids pulling serde_json into the platform layer —
/// it is already a workspace dependency though, so use it for correctness.
fn parse_token_response(body: String) -> Result<OAuthTokens, OAuthError> {
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| OAuthError::Exchange(format!("bad JSON: {e}")))?;
    let get = |key: &str| value.get(key).and_then(|v| v.as_str()).map(String::from);
    let access = get("access_token").ok_or_else(|| {
        // Error responses carry `error` + `error_description`.
        OAuthError::Exchange(
            value
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("response missing access_token")
                .to_string(),
        )
    })?;
    let refresh = get("refresh_token").ok_or_else(|| {
        OAuthError::Exchange("response missing refresh_token (offline_access not granted?)".into())
    })?;
    let expires_in = value
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);
    Ok(OAuthTokens {
        access_token: SecretString::from(access),
        refresh_token: SecretString::from(refresh),
        expires_in,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// PKCE math per RFC 7636 appendix B's test vector.
    #[test]
    fn pkce_challenge_matches_rfc7636_appendix_b() {
        // Verifier from the RFC appendix; expected S256 challenge.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn pkce_pair_shapes() {
        let (verifier, challenge) = pkce_pair().unwrap();
        assert_eq!(verifier.len(), 43, "32 bytes → 43 base64url chars");
        assert!(!verifier.contains('+') && !verifier.contains('/') && !verifier.contains('='));
        assert_eq!(challenge.len(), 43);
        assert_ne!(verifier, challenge);
        // Deterministic: same verifier → same challenge.
        assert_eq!(pkce_challenge(&verifier), challenge);
    }

    #[test]
    fn authorize_url_carries_verified_parameters() {
        let url = build_authorize_url(
            "test-client-id",
            "http://127.0.0.1/52341",
            "test-challenge",
            "st8",
        )
        .unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.domain(), Some("login.microsoftonline.com"));
        assert!(parsed.path().contains("/consumers/oauth2/v2.0/authorize"));
        // V-5.2 audit pin: the contacted host stays tied to the egress
        // inventory (netlog::STATIC_EGRESS_HOSTS).
        assert!(AUTHORIZE_URL.contains(crate::platform::netlog::HOST_MS_OAUTH));
        assert!(TOKEN_URL.contains(crate::platform::netlog::HOST_MS_OAUTH));
        let query: BTreeMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(query.get("response_type").unwrap(), "code");
        assert_eq!(query.get("redirect_uri").unwrap(), "http://127.0.0.1/52341");
        assert_eq!(query.get("state").unwrap(), "st8");
        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        // Scope must use outlook.office.com, not the office365.com variant
        // (documented common failure).
        assert!(query
            .get("scope")
            .unwrap()
            .contains("outlook.office.com/IMAP.AccessAsUser.All"));
        assert!(query.get("scope").unwrap().contains("offline_access"));
        assert!(!query.get("scope").unwrap().contains("office365.com"));
    }

    #[test]
    fn query_param_extraction_handles_percent_encoding() {
        let raw = "GET /?code=abc%20def+ghi&state=x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(extract_query_param(raw, "code").unwrap(), "abc def ghi");
        assert_eq!(extract_query_param(raw, "state").unwrap(), "x");
        assert_eq!(extract_query_param(raw, "missing"), None);
    }

    /// The loopback listener answers one redirect with a completion page and
    /// the code, but only when `state` matches (F2 of the review: the state
    /// binds the redirect to this attempt).
    #[test]
    fn loopback_serves_redirect_and_extracts_code() {
        let loopback = Loopback::bind().unwrap();
        let port = loopback.port;
        // D-017 requires the port in the PORT component, path empty.
        let uri = loopback.redirect_uri();
        let parsed = Url::parse(&uri).unwrap();
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(port));
        assert_eq!(parsed.path(), "/", "loopback path must be empty");

        let handle = std::thread::spawn(move || {
            let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = conn.write_all(
                b"GET /?code=the%2Bcode&state=st8 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            );
            let mut resp = String::new();
            let _ = conn.read_to_string(&mut resp);
            resp
        });
        let code = loopback
            .wait_for_code(Duration::from_secs(5), "st8")
            .unwrap();
        assert_eq!(code, "the+code", "redirect query arrives percent-decoded");
        let page = handle.join().unwrap();
        assert!(page.contains("signed in"));
    }

    /// A redirect carrying a different (or absent) `state` is a cancellation,
    /// never an accepted code.
    #[test]
    fn loopback_rejects_wrong_state() {
        let loopback = Loopback::bind().unwrap();
        let port = loopback.port;
        let handle = std::thread::spawn(move || {
            let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = conn.write_all(
                b"GET /?code=evil&state=stolen HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            );
            let mut resp = String::new();
            let _ = conn.read_to_string(&mut resp);
            resp
        });
        let err = loopback
            .wait_for_code(Duration::from_secs(5), "expected")
            .expect_err("state mismatch must not yield a code");
        assert!(matches!(err, OAuthError::FlowCancelled));
        let page = handle.join().unwrap();
        assert!(page.contains("failed"));
    }

    #[test]
    fn state_token_is_url_safe_and_unique() {
        let a = state_token().unwrap();
        let b = state_token().unwrap();
        assert_eq!(a.len(), 22, "16 bytes → 22 base64url chars");
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/') && !a.contains('='));
    }

    #[test]
    fn constant_time_eq_matches_only_equal() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn loopback_times_out_without_a_browser() {
        let loopback = Loopback::bind().unwrap();
        let err = loopback
            .wait_for_code(Duration::from_millis(200), "any")
            .expect_err("no request should time out");
        assert!(matches!(err, OAuthError::FlowCancelled));
    }

    /// Token response parsing, both success and the documented error shape.
    #[test]
    fn token_response_parsing() {
        let ok = parse_token_response(
            r#"{"token_type":"Bearer","scope":"offline_access IMAP.AccessAsUser.All",
                "expires_in":3599,"access_token":"AT","refresh_token":"RT"}"#
                .to_string(),
        )
        .unwrap();
        assert_eq!(ok.expires_in, 3599);
        assert_eq!(ok.access_token.expose_secret(), "AT");
        assert_eq!(ok.refresh_token.expose_secret(), "RT");

        let err = parse_token_response(
            r#"{"error":"invalid_grant","error_description":"AADSTS9002313: Invalid request"}"#
                .to_string(),
        )
        .expect_err("error body must surface");
        assert!(err.to_string().contains("AADSTS9002313"));
    }
}
