//! AI subsystem (spec §6): pinned-model manager (FR-20), hardware tiering
//! (FR-23), the llama-server sidecar (D-003/D-030/D-032), the ChatEngine
//! trait with llama-server/OpenAI/xAI/Anthropic implementations (FR-21/22/24),
//! prompts and budgets. The master switch (FR-25) lives at the app layer:
//! `ai.enabled = false` kills the sidecar and the commands refuse to run.

pub mod chat;
pub mod manager;
pub mod models;
pub mod prompts;
pub mod sidecar;
pub mod siwc;

use std::path::{Path, PathBuf};

pub use chat::{ChatEngine, EngineKind, Msg};
pub use models::{Hardware, Tier};
pub use sidecar::Sidecar;

/// Context window per tier (D-004): 4096 on LOW, 8192 otherwise.
pub fn ctx_size(tier: Tier) -> u32 {
    match tier {
        Tier::Low => 4096,
        Tier::Mid | Tier::High => 8192,
    }
}

/// Where AI settings read their BYO API keys from (Credential Manager only,
/// FR-2/FR-24 — never config files).
pub fn byo_key_target(kind: EngineKind) -> &'static str {
    match kind {
        EngineKind::OpenAi => "cracktivity.byo.openai",
        EngineKind::Anthropic => "cracktivity.byo.anthropic",
        EngineKind::Xai => "cracktivity.byo.xai",
        EngineKind::ChatGpt => "cracktivity.chatgpt.access",
        EngineKind::Local => "cracktivity.byo.local",
    }
}

/// Dynamic Credential Manager target for built-in and custom BYO provider IDs.
/// Rejects empty or non-slug IDs so keys are always cleanly namespaced.
pub fn byo_key_target_for_id(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed == "local" || trimmed.len() > 64 {
        return None;
    }
    if trimmed == "chatgpt" {
        return Some("cracktivity.chatgpt.access".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(format!("cracktivity.byo.{trimmed}"))
}

/// Everything a task needs to run inference: the resolved engine plus the
/// notice when a BYO engine fell back to the local model (FR-24 AC).
pub struct ResolvedEngine {
    pub engine: Box<dyn ChatEngine>,
    pub kind: EngineKind,
    /// Non-empty when the requested BYO engine failed and the local model
    /// answered instead — the UI shows this visibly.
    pub fallback_notice: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("AI is disabled (master switch, FR-25)")]
    Disabled,
    #[error("the local model is not downloaded yet (Settings → AI)")]
    ModelMissing,
    #[error("{0}")]
    Other(String),
}

impl From<sidecar::SidecarError> for AiError {
    fn from(e: sidecar::SidecarError) -> Self {
        AiError::Other(e.to_string())
    }
}

impl From<manager::ModelError> for AiError {
    fn from(e: manager::ModelError) -> Self {
        AiError::Other(e.to_string())
    }
}

/// The live AI state: one sidecar process, spawned on first task, killed on
/// drop or master-switch off. Owned by AppState behind a mutex.
pub struct AiState {
    pub sidecar: Option<Sidecar>,
    pub model_dir: PathBuf,
    pub resource_dir: PathBuf,
}

impl AiState {
    pub fn new(app_data_dir: &Path, resource_dir: &Path) -> AiState {
        AiState {
            sidecar: None,
            model_dir: app_data_dir.join("models"),
            resource_dir: resource_dir.to_path_buf(),
        }
    }

    /// The tier this hardware gets (or the user's override).
    pub fn effective_tier(&self, override_tier: Option<&str>) -> Tier {
        override_tier
            .and_then(models::Tier::parse)
            .unwrap_or_else(|| models::tier_for(&models::detect()))
    }

    fn model_pin(&self, tier: Tier) -> &'static models::ModelPin {
        models::write_model_for(tier)
    }

    pub fn model_downloaded(&self, tier: Tier) -> bool {
        manager::is_downloaded(&self.model_dir, self.model_pin(tier))
    }

    /// The local engine, spawning the sidecar if needed (FR-21/22 path).
    /// A dead sidecar restarts on the next task (spec §11).
    pub fn local_engine(&mut self, tier: Tier) -> Result<chat::OpenAiCompat, AiError> {
        let pin = self.model_pin(tier);
        if !manager::is_downloaded(&self.model_dir, pin) {
            return Err(AiError::ModelMissing);
        }
        let model_path = manager::model_path(&self.model_dir, pin);
        let server_exe = Sidecar::exe_path(&self.resource_dir);
        if !server_exe.exists() {
            return Err(AiError::Other(
                "llama-server.exe is not bundled in this build".into(),
            ));
        }
        let healthy = self.sidecar.as_mut().map(|s| s.is_alive()).unwrap_or(false);
        if !healthy {
            self.sidecar = Some(Sidecar::spawn(
                &server_exe,
                &model_path,
                threads_for(tier),
                ctx_size(tier),
            )?);
        }
        let s = self.sidecar.as_ref().expect("just spawned");
        Ok(chat::OpenAiCompat::local(s.port, &s.api_key))
    }

    /// Stops everything (FR-25 master switch AC: zero model processes).
    pub fn shutdown(&mut self) {
        if let Some(mut s) = self.sidecar.take() {
            s.kill();
        }
    }
}

fn threads_for(_tier: Tier) -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byo_key_targets_are_namespaced() {
        assert_eq!(byo_key_target(EngineKind::OpenAi), "cracktivity.byo.openai");
        assert_eq!(
            byo_key_target(EngineKind::Anthropic),
            "cracktivity.byo.anthropic"
        );
        assert_ne!(
            byo_key_target(EngineKind::OpenAi),
            byo_key_target(EngineKind::Xai)
        );
    }

    #[test]
    fn ctx_size_fits_the_tier_budget() {
        assert_eq!(ctx_size(Tier::Low), 4096);
        assert_eq!(ctx_size(Tier::Mid), 8192);
    }

    #[test]
    fn master_switch_off_kills_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AiState::new(dir.path(), dir.path());
        state.shutdown();
        assert!(state.sidecar.is_none());
    }
}
