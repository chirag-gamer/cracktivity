//! llama-server sidecar (spec §6.2, D-003/D-030): spawns the bundled
//! `llama-server.exe` on an ephemeral loopback port with a per-run API key,
//! waits for `/health`, and kills the process on drop. One process, on
//! demand (D-032: the classify-tier residency is deferred; only the write
//! model ever runs).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

impl std::fmt::Debug for Sidecar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sidecar")
            .field("port", &self.port)
            .field("model_path", &self.model_path)
            .finish_non_exhaustive()
    }
}

use base64::Engine;

#[derive(Debug, thiserror::Error)]
#[error("sidecar: {0}")]
pub struct SidecarError(pub String);

pub struct Sidecar {
    pub port: u16,
    pub api_key: String,
    child: Option<Child>,
    /// Model path for restart-on-crash (spec §11: restart on next task).
    pub model_path: PathBuf,
    server_exe: PathBuf,
    threads: u32,
    ctx_size: u32,
}

impl Sidecar {
    /// Path of the bundled server binary inside the app resources dir.
    pub fn exe_path(resource_dir: &Path) -> PathBuf {
        resource_dir.join(crate::ai::models::LLAMA_SERVER_EXE)
    }

    /// Spawns the server and waits for readiness (503 → 200 on /health).
    pub fn spawn(
        server_exe: &Path,
        model_path: &Path,
        threads: u32,
        ctx_size: u32,
    ) -> Result<Sidecar, SidecarError> {
        let port = free_port()?;
        let api_key = random_key();
        let child = Command::new(server_exe)
            .arg("-m")
            .arg(model_path)
            .arg("-c")
            .arg(ctx_size.to_string())
            .arg("-t")
            .arg(threads.to_string())
            .arg("-tb")
            .arg(threads.to_string())
            .arg("-np")
            .arg("1")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--api-key")
            .arg(&api_key)
            // Pinned-build flags verified in D-030 (b12110).
            .arg("--cache-reuse")
            .arg("256")
            .arg("--load-mode")
            .arg("mmap")
            .arg("--no-webui")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                crate::platform::applog::record("error", format!("llama-server spawn failed: {e}"));
                SidecarError(format!("spawn failed: {e}"))
            })?;

        let mut sidecar = Sidecar {
            port,
            api_key,
            child: Some(child),
            model_path: model_path.to_path_buf(),
            server_exe: server_exe.to_path_buf(),
            threads,
            ctx_size,
        };
        sidecar.wait_healthy(Duration::from_secs(180))?;
        crate::platform::applog::record("info", format!("llama-server ready on port {port}"));
        Ok(sidecar)
    }

    /// Polls `GET /health` until 200 (public endpoint, no key) or timeout.
    fn wait_healthy(&mut self, timeout: Duration) -> Result<(), SidecarError> {
        let deadline = Instant::now() + timeout;
        let url = format!("http://127.0.0.1:{}/health", self.port);
        loop {
            if let Some(child) = self.child.as_mut() {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    crate::platform::applog::record(
                        "error",
                        "llama-server exited during startup (model file invalid?)",
                    );
                    return Err(SidecarError(
                        "llama-server exited during startup (model file invalid?)".into(),
                    ));
                }
            }
            if let Ok(response) = ureq::get(&url).call() {
                if response.status() == 200 {
                    return Ok(());
                }
            }
            if Instant::now() > deadline {
                self.kill();
                return Err(SidecarError(format!(
                    "llama-server did not become healthy within {} s",
                    timeout.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    /// Restarts the server (crash recovery, spec §11).
    pub fn restart(&mut self) -> Result<(), SidecarError> {
        self.kill();
        let fresh = Sidecar::spawn(
            &self.server_exe,
            &self.model_path,
            self.threads,
            self.ctx_size,
        )?;
        let (port, api_key, child) = fresh.into_parts();
        self.port = port;
        self.api_key = api_key;
        self.child = child;
        Ok(())
    }

    /// Splits the sidecar into its moveable parts (the subsequent Drop no-ops).
    fn into_parts(mut self) -> (u16, String, Option<Child>) {
        let child = self.child.take();
        (self.port, self.api_key.clone(), child)
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.kill();
    }
}

fn free_port() -> Result<u16, SidecarError> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .map(|l| l.local_addr().map(|a| a.port()))
        .map_err(|e| SidecarError(format!("no free port: {e}")))?
        .map_err(|e| SidecarError(format!("no free port: {e}")))
}

fn random_key() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("random api key");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_fails_loudly_without_the_binary() {
        let err = Sidecar::spawn(
            Path::new("Z:/definitely/not/here/llama-server.exe"),
            Path::new("Z:/no/model.gguf"),
            2,
            2048,
        )
        .unwrap_err();
        assert!(err.to_string().contains("spawn failed"), "{err:?}");
    }
}
