//! Model catalog + hardware tiering (spec §6.1, D-004, D-031): pinned
//! GGUF files with frozen revisions and SHA-256 digests (verified live via
//! the HuggingFace API, 2026-11-05 — see decisions.md D-031).

/// Hardware tier (D-004): RAM + cores decide the write-tier model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Low,
    Mid,
    High,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Low => "low",
            Tier::Mid => "mid",
            Tier::High => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Tier::Low),
            "mid" => Some(Tier::Mid),
            "high" => Some(Tier::High),
            _ => None,
        }
    }
}

/// One downloadable model file (D-031 pin).
#[derive(Debug, Clone, Copy)]
pub struct ModelPin {
    pub tier: Tier,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
}

impl ModelPin {
    /// `https://huggingface.co/<repo>/resolve/<rev>/<file>` (D-015 allowlist:
    /// huggingface.co + its CDN, reachable only during opt-in downloads).
    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, self.file
        )
    }
}

/// The write-tier model per hardware class (D-031 pins; classify-tier
/// residency is deferred per D-032 — the file stays pinned for a future
/// task that justifies it).
pub const MODELS: [ModelPin; 4] = [
    // LOW write tier
    ModelPin {
        tier: Tier::Low,
        repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
        revision: "91cad51170dc346986eccefdc2dd33a9da36ead9",
        file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        size_bytes: 1_117_320_736,
        sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
    },
    // MID write tier
    ModelPin {
        tier: Tier::Mid,
        repo: "unsloth/Qwen3-1.7B-GGUF",
        revision: "d7f544eead698dbd1f15126ef60b45a1e1933222",
        file: "Qwen3-1.7B-Q4_K_M.gguf",
        size_bytes: 1_107_409_472,
        sha256: "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897",
    },
    // HIGH write tier
    ModelPin {
        tier: Tier::High,
        repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF",
        revision: "a06e946bb6b655725eafa393f4a9745d460374c9",
        file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        size_bytes: 2_497_281_120,
        sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
    },
    // HIGH fallback (MIT) — downloadable on demand from Settings.
    ModelPin {
        tier: Tier::High,
        repo: "bartowski/microsoft_Phi-4-mini-instruct-GGUF",
        revision: "7ff82c2aaa4dde30121698a973765f39be5288c0",
        file: "microsoft_Phi-4-mini-instruct-Q4_K_M.gguf",
        size_bytes: 2_491_874_688,
        sha256: "01999f17c39cc3074afae5e9c539bc82d45f2dd7faa3917c66cbef76fce8c0c2",
    },
];

/// The model pin a tier uses for writing.
pub fn write_model_for(tier: Tier) -> &'static ModelPin {
    MODELS
        .iter()
        .find(|m| m.tier == tier && !m.file.contains("Phi-4-mini"))
        .expect("every tier has a write model")
}

/// llama.cpp build the sidecar expects (D-030: binary-bearing build tags;
/// verified 2026-11-05 — llama-server.exe present, flags confirmed).
pub const LLAMA_CPP_TAG: &str = "b12110";
pub const LLAMA_SERVER_EXE: &str = "llama-server.exe";

/// Detected hardware (test-injectable).
#[derive(Debug, Clone, Copy)]
pub struct Hardware {
    pub total_ram_gb: u64,
    pub logical_cores: u32,
}

/// Detects RAM/cores on Windows (spec §6.1: auto tier from RAM+cores).
pub fn detect() -> Hardware {
    Hardware {
        total_ram_gb: total_ram_gb(),
        logical_cores: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(2),
    }
}

#[cfg(windows)]
fn total_ram_gb() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status is sized correctly; the call only fills the struct.
    let _ = unsafe { GlobalMemoryStatusEx(&mut status) };
    (status.ullTotalPhys / (1024 * 1024 * 1024)) as u64
}

#[cfg(not(windows))]
fn total_ram_gb() -> u64 {
    8
}

/// Tier selection per D-004: LOW < 6 GB RAM / 2 cores; MID 6–12 GB / 4
/// cores; HIGH ≥ 16 GB / 8 cores.
pub fn tier_for(hw: &Hardware) -> Tier {
    if hw.total_ram_gb >= 16 && hw.logical_cores >= 8 {
        Tier::High
    } else if hw.total_ram_gb < 6 || hw.logical_cores <= 2 {
        Tier::Low
    } else {
        Tier::Mid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V-5.2 audit pin: the pinned download URL stays on the inventoried host.
    #[test]
    fn download_url_stays_on_the_inventored_host() {
        assert!(MODELS[0].download_url().starts_with(&format!(
            "https://{}/",
            crate::platform::netlog::HOST_HUGGINGFACE
        )));
    }

    #[test]
    fn tier_boundaries_match_d004() {
        let low = tier_for(&Hardware {
            total_ram_gb: 4,
            logical_cores: 2,
        });
        assert_eq!(low, Tier::Low);
        let mid8 = tier_for(&Hardware {
            total_ram_gb: 8,
            logical_cores: 4,
        });
        assert_eq!(mid8, Tier::Mid);
        // FR-23 AC: an 8 GB machine selects the 1.7B (MID).
        assert_eq!(write_model_for(mid8).file, "Qwen3-1.7B-Q4_K_M.gguf");
        let high = tier_for(&Hardware {
            total_ram_gb: 16,
            logical_cores: 8,
        });
        assert_eq!(high, Tier::High);
        assert_eq!(
            write_model_for(high).file,
            "Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
        );
        // Low RAM trumps cores: 16 GB but 2 cores stays LOW.
        assert_eq!(
            tier_for(&Hardware {
                total_ram_gb: 16,
                logical_cores: 2
            }),
            Tier::Low
        );
    }

    #[test]
    fn every_model_pin_is_complete() {
        for m in &MODELS {
            assert!(m.repo.contains('/'), "{}: repo path", m.file);
            assert_eq!(m.revision.len(), 40, "{}: full commit sha", m.file);
            assert_eq!(m.sha256.len(), 64, "{}: sha256", m.file);
            assert!(m.size_bytes > 100_000_000, "{}: plausible size", m.file);
            assert!(
                m.download_url().starts_with(&format!(
                    "https://huggingface.co/{}/resolve/{}",
                    m.repo, m.revision
                )),
                "resolve form is mandatory (without it HF returns 404)"
            );
        }
        // LOW tier gets the 1.5B (FR-23).
        assert_eq!(
            write_model_for(Tier::Low).file,
            "qwen2.5-1.5b-instruct-q4_k_m.gguf"
        );
    }
}
