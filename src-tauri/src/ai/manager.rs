//! Model manager (FR-20, spec §6.5): resumable download of pinned GGUF files
//! with SHA-256 verification. Progress streams through a callback so the UI
//! shows bytes received. Downloads happen ONLY after the user opts in
//! (D-005/D-015) and touch exactly the pinned HuggingFace URLs (D-031).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use ureq::ResponseExt as _;

use super::models::ModelPin;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("download failed: {0}")]
    Request(String),
    #[error("sha-256 mismatch after download (expected {expected})")]
    Checksum { expected: String },
    #[error("model file is not a GGUF (bad magic)")]
    NotGguf,
    #[error("no sha-256 resume support: partial file discarded, restart download")]
    ResumeUnsupported,
}

pub(crate) fn models_dir(base: &Path) -> PathBuf {
    base.join("models")
}

/// Path of a pinned model inside the models dir.
pub fn model_path(base: &Path, pin: &ModelPin) -> PathBuf {
    models_dir(base).join(pin.file)
}

pub fn is_downloaded(base: &Path, pin: &ModelPin) -> bool {
    let path = model_path(base, pin);
    path.exists() && std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) == pin.size_bytes
}

/// Downloads (or resumes) the pinned model into `base/models/`. `progress`
/// receives (received_bytes, total_bytes). Verification is not disableable
/// (spec §10.5): a checksum mismatch deletes the file (spec §11).
pub fn download(
    base: &Path,
    pin: &ModelPin,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    let dir = models_dir(base);
    std::fs::create_dir_all(&dir)?;
    let path = model_path(base, pin);
    download_to(
        &pin.download_url(),
        &path,
        pin.size_bytes,
        pin.sha256,
        progress,
    )
}

/// The wire seam: one pinned-URL download into `path` (a `.part` suffix is
/// managed internally). URL-injectable so tests drive it against a fixture
/// server instead of HuggingFace.
fn download_to(
    url: &str,
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension("gguf.part");

    let mut offset = 0u64;
    if partial.exists() {
        let size = std::fs::metadata(&partial)?.len();
        // A partial is only usable when the server honors Range resumes;
        // we probe with the request below. Cap the offset to a sane value.
        if size < expected_size {
            offset = size;
        } else {
            std::fs::remove_file(&partial)?;
        }
    }

    let config = ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(3600)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    crate::platform::netlog::record_url(url);
    let mut request = agent.get(url);
    if offset > 0 {
        request = request.header("Range", &format!("bytes={offset}-"));
    }
    let response = request
        .call()
        .map_err(|e| ModelError::Request(e.to_string()))?;
    // HF resolve URLs redirect to a CDN host; record what was actually hit.
    crate::platform::netlog::record(response.get_uri().host().unwrap_or(""));
    let status = response.status().as_u16();

    if offset > 0 && status == 200 {
        // Server ignored the Range header: discard the partial, start over.
        std::fs::remove_file(&partial)?;
        offset = 0;
    } else if offset > 0 && status == 206 {
        // Resuming (spec §6.5: resumable after interruption).
    } else if offset > 0 {
        return Err(ModelError::ResumeUnsupported);
    }

    let total: u64 = match response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
    {
        Some(v) => v.parse().unwrap_or(0) + offset,
        None => expected_size,
    };

    let mut file = if offset > 0 {
        std::fs::OpenOptions::new().append(true).open(&partial)?
    } else {
        std::fs::File::create(&partial)?
    };

    let mut reader = response.into_body().into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut received = offset;
    let mut hasher = Sha256::new();
    if offset > 0 {
        // Hash the already-downloaded prefix so final verification is total.
        let existing = std::fs::read(&partial)?;
        hasher.update(&existing);
    }
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                file.write_all(&buf[..n])?;
                hasher.update(&buf[..n]);
                received += n as u64;
                progress(received, total);
            }
            Err(e) => {
                // Keep the partial for resume (spec §11).
                let _ = file.flush();
                return Err(ModelError::Io(e));
            }
        }
    }
    file.flush()?;

    let digest = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if digest != expected_sha256 {
        let expected = expected_sha256.to_string();
        std::fs::remove_file(&partial).ok();
        return Err(ModelError::Checksum { expected });
    }
    // GGUF magic check: a checksum-valid file is already trusted, but this
    // catches a pinned URL pointing at the wrong asset.
    let mut magic = [0u8; 4];
    {
        let mut f = std::fs::File::open(&partial)?;
        f.read_exact(&mut magic)?;
    }
    if &magic != b"GGUF" {
        std::fs::remove_file(&partial).ok();
        return Err(ModelError::NotGguf);
    }
    std::fs::rename(&partial, path)?;
    Ok(path.to_path_buf())
}

/// Deletes a model file, reclaiming disk (FR-20 AC).
pub fn delete(base: &Path, pin: &ModelPin) -> Result<bool, ModelError> {
    let path = model_path(base, pin);
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Disk usage of the models dir.
pub fn disk_usage(base: &Path) -> u64 {
    let dir = models_dir(base);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake GGUF body (4 KiB, real magic) shared by the fixture tests.
    fn fake_gguf() -> (Vec<u8>, String) {
        let mut v = vec![b'G', b'G', b'U', b'F'];
        v.extend((0..4092).map(|i| (i % 251) as u8));
        let mut h = Sha256::new();
        h.update(&v);
        (
            v,
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        )
    }

    /// Serves one request: honors `Range` suffixes with 206, else 200.
    fn spawn_fixture(body: &'static [u8]) -> (std::sync::mpsc::Receiver<String>, u16) {
        let (req_tx, req_rx) = std::sync::mpsc::channel();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = req_tx.send(req.clone());
            let range = req
                .lines()
                .find(|l| l.to_lowercase().starts_with("range:"))
                .and_then(|l| l.split_once('=').map(|(_, v)| v.to_string()));
            match range {
                Some(from) => {
                    let start: usize = from.trim_end_matches('-').parse().unwrap();
                    let rest = &body[start..];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\r\n",
                        rest.len()
                    );
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(rest);
                }
                None => {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(body);
                }
            }
        });
        (req_rx, port)
    }

    /// Full download: progress events, checksum pass, GGUF rename (FR-20).
    #[test]
    fn download_verifies_and_renames() {
        let (body, digest) = fake_gguf();
        let leaked: &'static [u8] = Box::leak(body.clone().into_boxed_slice());
        let (req_rx, port) = spawn_fixture(leaked);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models").join("fake.gguf");
        let mut progress_calls = 0;
        let final_progress = std::cell::RefCell::new((0u64, 0u64));
        download_to(
            &format!("http://127.0.0.1:{port}/f.gguf"),
            &path,
            body.len() as u64,
            &digest,
            &mut |received, total| {
                progress_calls += 1;
                *final_progress.borrow_mut() = (received, total);
            },
        )
        .unwrap();
        assert!(req_rx.recv().unwrap().starts_with("GET /f.gguf"));
        assert!(progress_calls > 0, "progress streamed");
        assert_eq!(final_progress.borrow().0, body.len() as u64);
        assert!(path.exists());
        assert!(!path.with_extension("gguf.part").exists());
        // Disk-layer helpers agree.
        let pin = ModelPin {
            tier: super::super::models::Tier::Low,
            repo: "t/f",
            revision: "0",
            file: "fake.gguf",
            size_bytes: body.len() as u64,
            sha256: Box::leak(digest.into_boxed_str()),
        };
        assert!(is_downloaded(dir.path(), &pin));
        assert_eq!(disk_usage(dir.path()), body.len() as u64);
        assert!(delete(dir.path(), &pin).unwrap());
    }

    /// Resume: an existing partial is continued with a Range request and the
    /// hash covers the whole file (spec §6.5/§11).
    #[test]
    fn download_resumes_from_partial() {
        let (body, digest) = fake_gguf();
        let leaked: &'static [u8] = Box::leak(body.clone().into_boxed_slice());
        let (req_rx, port) = spawn_fixture(leaked);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models").join("fake.gguf");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Half a download already on disk.
        std::fs::write(path.with_extension("gguf.part"), &body[..2048]).unwrap();
        download_to(
            &format!("http://127.0.0.1:{port}/f.gguf"),
            &path,
            body.len() as u64,
            &digest,
            &mut |_, _| {},
        )
        .unwrap();
        let req = req_rx.recv().unwrap();
        assert!(
            req.contains("range: bytes=2048-") || req.contains("Range: bytes=2048-"),
            "{req}"
        );
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), body, "resumed byte-exact");
    }

    /// A checksum mismatch deletes the partial and errors (never installs).
    #[test]
    fn checksum_mismatch_deletes_and_errors() {
        let (body, _) = fake_gguf();
        let leaked: &'static [u8] = Box::leak(body.into_boxed_slice());
        let (_req_rx, port) = spawn_fixture(leaked);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models").join("fake.gguf");
        let err = download_to(
            &format!("http://127.0.0.1:{port}/f.gguf"),
            &path,
            4096,
            &"0".repeat(64),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, ModelError::Checksum { .. }), "{err:?}");
        assert!(!path.exists());
        assert!(!path.with_extension("gguf.part").exists());
    }
}
