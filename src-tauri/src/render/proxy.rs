//! Loopback image-fetch proxy (spec §8.5, D-014): when the user allows
//! remote images (per message or per sender), the reader loads them from
//! this 127.0.0.1-only server, which fetches through the app's HTTP client
//! with no referer and no cookies. The user's IP still reaches the sender —
//! that caveat is stated in-product (NFR-10). Egress: the fetched host is
//! the mail-content host from the image URL (D-015 exception inherent to
//! "Show images"; visible in the Diagnostics host list).

use std::io::{Read, Write};
use std::sync::Arc;

use base64::Engine;

/// Max proxied image size (attachments-style cap; remote images are layout
/// assets, not documents).
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

pub struct ImageProxy {
    /// `http://127.0.0.1:<port>` — embedded into rendered documents.
    pub origin: String,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ImageProxy {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // A connect attempt wakes the accept loop; drop then terminates it.
        let _ = std::net::TcpStream::connect(self.origin.replacen("http://", "", 1));
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Builds the proxied URL embedded in place of a remote image src.
pub fn proxied_url(origin: &str, remote: &str) -> String {
    let target = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(remote.as_bytes());
    let token = proxy_token();
    format!("{origin}/img?t={token}&u={target}")
}

fn proxy_token() -> String {
    use std::sync::OnceLock;
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            let mut bytes = [0u8; 24];
            getrandom::fill(&mut bytes).expect("random token");
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        })
        .clone()
}

/// Starts the proxy on an ephemeral loopback port. Only requests carrying
/// the per-run token are served; everything else gets 403.
pub fn start() -> std::io::Result<ImageProxy> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_c = Arc::clone(&shutdown);
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown_c.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
            serve_one(&mut stream);
        }
    });
    Ok(ImageProxy {
        origin: format!("http://127.0.0.1:{port}"),
        shutdown,
        handle: Some(handle),
    })
}

fn serve_one(stream: &mut std::net::TcpStream) {
    let mut buf = [0u8; 8192];
    let mut request = Vec::new();
    // Read until the end of the request head.
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") || request.len() > 65536 {
                    break;
                }
            }
        }
    }
    let request = String::from_utf8_lossy(&request);
    let Some(request_line) = request.lines().next() else {
        return;
    };
    let status_line = handle_request(request_line);
    let _ = stream.write_all(status_line.as_bytes());
    let _ = stream.flush();
}

fn handle_request(request_line: &str) -> String {
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut token = None;
    let mut target = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "t" => token = Some(v.to_string()),
                "u" => target = Some(v.to_string()),
                _ => {}
            }
        }
    }
    if token.as_deref() != Some(proxy_token().as_str()) {
        return respond(403, "forbidden");
    }
    let Some(target) = target.and_then(|t| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(t)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    }) else {
        return respond(400, "bad target");
    };
    let Ok(url) = url::Url::parse(&target) else {
        return respond(400, "bad url");
    };
    if !matches!(url.scheme(), "http" | "https") {
        return respond(400, "unsupported scheme");
    }
    // Plain HTTP image hosts are allowed (newsletters do that); no referer,
    // no cookies, a hard timeout and size cap.
    let config = ureq::config::Config::builder()
        .max_redirects(4)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    crate::platform::netlog::record_url(&target);
    let response = match agent
        .get(&target)
        // No Referer header at all (an empty value would still be sent).
        .header("Accept", "image/*")
        .call()
    {
        Ok(r) => r,
        Err(_) => return respond(502, "fetch failed"),
    };
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let mut body = Vec::new();
    let mut reader = response
        .into_body()
        .into_reader()
        .take(MAX_IMAGE_BYTES as u64);
    if reader.read_to_end(&mut body).is_err() {
        return respond(502, "fetch failed");
    }
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    ) + &String::from_utf8_lossy(&body)
}

fn respond(code: u16, message: &str) -> String {
    format!(
        "HTTP/1.1 {code} {message}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
        message.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxied_urls_carry_token_and_base64_target() {
        let origin = "http://127.0.0.1:1234";
        let url = proxied_url(origin, "https://media.example/p.jpg");
        assert!(url.starts_with("http://127.0.0.1:1234/img?t="));
        let (t, u) = url
            .split_once("t=")
            .map(|(_, rest)| rest.split_once("&u=").unwrap())
            .unwrap();
        assert_eq!(t.len(), 32, "24 random bytes, url-safe base64");
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(u)
            .unwrap();
        assert_eq!(decoded, b"https://media.example/p.jpg");
    }

    /// The token gate: an unauthenticated request is refused; a valid token
    /// fetches through to the fixture origin (plain HTTP target allowed).
    #[test]
    fn proxy_serves_only_tokenized_requests() {
        let target_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let target_port = target_listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = target_listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\nPNG!",
            );
        });

        let proxy = start().unwrap();
        let client = |path: &str| {
            let mut s =
                std::net::TcpStream::connect(proxy.origin.replacen("http://", "", 1)).unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        };

        // No token → 403.
        let rejected = client("/img?t=bogus&u=abc");
        assert!(rejected.starts_with("HTTP/1.1 403"), "{rejected}");

        // Valid token → proxied body with the upstream content type.
        let good = proxied_url(
            &proxy.origin,
            &format!("http://127.0.0.1:{target_port}/p.png"),
        );
        let path = &good[good.find("/img").unwrap()..];
        let served = client(path);
        assert!(served.starts_with("HTTP/1.1 200"), "{served}");
        assert!(served.contains("Content-Type: image/png"), "{served}");
        assert!(served.ends_with("PNG!"), "{served}");
    }
}
