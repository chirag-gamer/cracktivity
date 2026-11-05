//! One-click unsubscribe (FR-14, RFC 8058): a POST happens **only** on the
//! user's explicit click — never automatically (AC: "no automatic
//! unsubscribe ever happens without a click"). The POST carries exactly the
//! RFC 8058 body to the HTTPS `List-Unsubscribe` URI; cookies/credentials are
//! never attached, and redirects are refused (§3.1: the sender must not
//! redirect). Without `List-Unsubscribe-Post` the mailto fallback is the only
//! path, and it is the user's own mail client, not this process.

/// Parsed `List-Unsubscribe` header: the first HTTPS URL (one-click POST
/// target) and the first mailto URI (fallback), in header order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnsubscribeLinks {
    pub https: Option<String>,
    pub mailto: Option<String>,
}

/// Extracts `<...>` URIs from the header's comma list (RFC 2369 §3.2 angle-
/// bracket form; some senders omit the brackets — accept bare tokens too).
pub fn parse_unsubscribe(header: &str) -> UnsubscribeLinks {
    let mut links = UnsubscribeLinks::default();
    for part in header.split(',') {
        let uri = part
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        let lower = uri.to_lowercase();
        if links.https.is_none() && lower.starts_with("https://") {
            links.https = Some(uri.to_string());
        } else if links.mailto.is_none() && lower.starts_with("mailto:") {
            links.mailto = Some(uri.to_string());
        }
    }
    links
}

#[derive(Debug, thiserror::Error)]
pub enum UnsubscribeError {
    #[error("this message has no unsubscribe POST endpoint (one-click not offered)")]
    NoPostEndpoint,
    #[error("only https unsubscribe endpoints are accepted (RFC 8058)")]
    InsecureEndpoint,
    #[error("unsubscribe request failed: {0}")]
    Request(String),
    #[error("unsubscription endpoint refused (status {0})")]
    Rejected(u16),
    #[error("endpoint tried to redirect — refusing to follow (RFC 8058 §3.1)")]
    Redirect,
}

/// Performs the RFC 8058 one-click POST. `header` is the message's raw
/// `List-Unsubscribe` value; `post_header` the raw `List-Unsubscribe-Post`
/// value. Returns the 2xx status on success.
pub fn one_click(header: &str, post_header: Option<&str>) -> Result<u16, UnsubscribeError> {
    one_click_with_roots(header, post_header, None)
}

/// Root-cert injection exists for the V-4.3 fixture server; production
/// callers pass `None` (the standard web PKI applies).
fn one_click_with_roots(
    header: &str,
    post_header: Option<&str>,
    tls: Option<ureq::tls::TlsConfig>,
) -> Result<u16, UnsubscribeError> {
    // One-click is offered only when the message advertises
    // `List-Unsubscribe-Post: List-Unsubscribe=One-Click` (RFC 8058 §5).
    if !post_header
        .map(|v| v.to_lowercase().contains("one-click"))
        .unwrap_or(false)
    {
        return Err(UnsubscribeError::NoPostEndpoint);
    }
    let url = parse_unsubscribe(header)
        .https
        .ok_or(UnsubscribeError::NoPostEndpoint)?;
    if !url.to_lowercase().starts_with("https://") {
        return Err(UnsubscribeError::InsecureEndpoint);
    }

    // No cookies, no auth, no redirects (RFC 8058 §3.1): `max_redirects(0)`
    // makes a 3xx come back as an ordinary status (ureq's status-as-error
    // covers only 4xx/5xx — verified against run.rs), and the match below
    // refuses it as Redirect. A caller-
    // supplied TLS config (fixture tests only) replaces the default.
    let mut config = ureq::config::Config::builder().max_redirects(0);
    if let Some(tls) = tls {
        config = config.tls_config(tls);
    }
    let agent = ureq::Agent::new_with_config(config.build());
    crate::platform::netlog::record_url(&url);
    let response = agent
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send("List-Unsubscribe=One-Click")
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => UnsubscribeError::Rejected(code),
            other => UnsubscribeError::Request(other.to_string()),
        })?;
    let status = response.status().as_u16();
    match status {
        200..=299 => Ok(status),
        300..=399 => Err(UnsubscribeError::Redirect),
        _ => Err(UnsubscribeError::Rejected(status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_parsing_picks_https_and_mailto() {
        let l = parse_unsubscribe(
            "<https://lists.example/unsub?id=1>, <mailto:leave@lists.example?subject=bye>",
        );
        assert_eq!(l.https.as_deref(), Some("https://lists.example/unsub?id=1"));
        assert_eq!(
            l.mailto.as_deref(),
            Some("mailto:leave@lists.example?subject=bye")
        );

        // Bare form without angle brackets, http:// never counted as one-click.
        let l2 = parse_unsubscribe("http://insecure.example/u");
        assert_eq!(l2.https, None);

        // Only a mailto link: fallback survives, no POST endpoint.
        let l3 = parse_unsubscribe("<mailto:bye@x.example>");
        assert_eq!(l3.https, None);
        assert_eq!(l3.mailto.as_deref(), Some("mailto:bye@x.example"));
    }

    #[test]
    fn one_click_requires_the_post_advertised() {
        let err = one_click("<https://x.example/u>", None).unwrap_err();
        assert!(matches!(err, UnsubscribeError::NoPostEndpoint));
        let err =
            one_click("<https://x.example/u>", Some("List-Unsubscribe=Web-Action")).unwrap_err();
        assert!(matches!(err, UnsubscribeError::NoPostEndpoint));
    }

    #[test]
    fn one_click_refuses_insecure_endpoints() {
        // A plaintext-http-only header offers no valid POST endpoint; the
        // https guard in one_click stays as defense-in-depth.
        let err =
            one_click("<http://x.example/u>", Some("List-Unsubscribe=One-Click")).unwrap_err();
        assert!(matches!(err, UnsubscribeError::NoPostEndpoint));
    }

    /// The wire contract against a real TLS fixture server (V-4.3): exact
    /// method, path, content type, body; no credentials/cookies leak; 2xx
    /// success; 4xx → Rejected. RFC 8058 requires HTTPS, so the fixture is a
    /// one-shot rustls server with a self-signed cert the client trusts.
    #[test]
    fn one_click_wire_contract_against_fixture_server() {
        use std::io::{Read, Write};
        use std::sync::Arc;

        // Self-signed server cert for 127.0.0.1 (the client's fixture TLS
        // config is verification-disabled; the cert only needs to handshake).
        let server = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let cert_der: Vec<u8> = server.cert.der().to_vec();
        let key_der: Vec<u8> = server.key_pair.serialize_der();
        let spawn_fixture = |status_line: &'static str| {
            let (port_tx, port_rx) = std::sync::mpsc::channel();
            let (req_tx, req_rx) = std::sync::mpsc::channel();
            let cert_der = cert_der.clone();
            let key_der = key_der.clone();
            std::thread::spawn(move || {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                port_tx.send(listener.local_addr().unwrap().port()).unwrap();
                let config = rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![rustls_pki_types::CertificateDer::from(cert_der)],
                        rustls_pki_types::PrivateKeyDer::Pkcs8(
                            rustls_pki_types::PrivatePkcs8KeyDer::from(key_der),
                        ),
                    )
                    .unwrap();
                let (mut sock, _) = listener.accept().unwrap();
                let mut conn = rustls::ServerConnection::new(Arc::new(config)).unwrap();
                let mut tls = rustls::Stream::new(&mut conn, &mut sock);
                // Accumulate until the request is complete: rustls may split
                // headers and body across records.
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match tls.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if req.ends_with(b"List-Unsubscribe=One-Click") {
                                break;
                            }
                        }
                    }
                }
                req_tx
                    .send(String::from_utf8_lossy(&req).to_string())
                    .unwrap();
                tls.write_all(format!("{status_line}\r\nContent-Length: 0\r\n\r\n").as_bytes())
                    .unwrap();
                tls.flush().unwrap();
            });
            (port_rx, req_rx)
        };

        // Success path: assert the exact request contract.
        let (port_rx, req_rx) = spawn_fixture("HTTP/1.1 200 OK");
        let port = port_rx.recv().unwrap();
        // Loopback fixture: the TLS config is connection-only (verification
        // disabled) — the RFC's HTTPS requirement is enforced by the URL
        // scheme guard, and the fixture pins the wire contract itself.
        let fixture_tls = ureq::tls::TlsConfig::builder()
            .disable_verification(true)
            .build();
        let status = one_click_with_roots(
            &format!("<http://ignored.example/u>, <https://127.0.0.1:{port}/unsub?t=xyz>"),
            Some("List-Unsubscribe=One-Click"),
            Some(fixture_tls.clone()),
        )
        .unwrap();
        assert_eq!(status, 200);
        let req = req_rx.recv().unwrap();
        assert!(req.starts_with("POST /unsub?t=xyz HTTP/1.1"), "{req}");
        assert!(
            req.contains("content-type: application/x-www-form-urlencoded"),
            "{req}"
        );
        assert!(req.ends_with("List-Unsubscribe=One-Click"), "{req}");
        assert!(
            !req.to_lowercase().contains("authorization"),
            "no creds: {req}"
        );
        assert!(!req.to_lowercase().contains("cookie"), "no cookies: {req}");

        // Rejection path: 404 → Rejected(404).
        let (port_rx2, _req_rx2) = spawn_fixture("HTTP/1.1 404 Not Found");
        let port2 = port_rx2.recv().unwrap();
        let err = one_click_with_roots(
            &format!("<https://127.0.0.1:{port2}/u>"),
            Some("List-Unsubscribe=One-Click"),
            Some(fixture_tls.clone()),
        )
        .unwrap_err();
        assert!(matches!(err, UnsubscribeError::Rejected(404)), "{err:?}");

        // Redirect path: 302 → Redirect (RFC 8058 §3.1: never followed).
        let (port_rx3, _req_rx3) = spawn_fixture("HTTP/1.1 302 Found");
        let port3 = port_rx3.recv().unwrap();
        let err = one_click_with_roots(
            &format!("<https://127.0.0.1:{port3}/u>"),
            Some("List-Unsubscribe=One-Click"),
            Some(fixture_tls),
        )
        .unwrap_err();
        assert!(matches!(err, UnsubscribeError::Redirect), "{err:?}");
    }
}
