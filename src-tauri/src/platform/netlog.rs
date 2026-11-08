//! Session host log (FR-32 evidence, D-015): every remote host this process
//! contacted, in-process only (no telemetry — this never leaves the
//! machine). The Diagnostics panel lists exactly what the egress layer did.

use std::collections::BTreeSet;
use std::sync::Mutex;

static HOSTS: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);

// App-owned egress hosts (FR-32/D-015 evidence): the only hosts the app can
// contact on its own initiative. User-content-driven egress (remote images
// from mail HTML, List-Unsubscribe POSTs, user-entered IMAP/SMTP hosts) is
// deliberately outside this inventory — the Diagnostics panel lists exactly
// what a session hit, which is the audit surface.

pub const HOST_MS_OAUTH: &str = "login.microsoftonline.com";
pub const HOST_HUGGINGFACE: &str = "huggingface.co";
pub const HOST_OPENAI: &str = "api.openai.com";
pub const HOST_OPENAI_AUTH: &str = "auth.openai.com";
pub const HOST_XAI: &str = "api.x.ai";
pub const HOST_ANTHROPIC: &str = "api.anthropic.com";
/// llama-server + the loopback image fetch proxy.
pub const HOST_LOOPBACK: &str = "127.0.0.1";

/// Every host the app contacts by its own code paths (no user content, no
/// user configuration). The V-5.2 audit test pins this set against the
/// provider presets and the AI/OAuth endpoint constants.
pub const STATIC_EGRESS_HOSTS: &[&str] = &[
    HOST_MS_OAUTH,
    HOST_HUGGINGFACE,
    HOST_OPENAI,
    HOST_OPENAI_AUTH,
    HOST_XAI,
    HOST_ANTHROPIC,
    HOST_LOOPBACK,
    // Provider preset servers (kept in sync with providers::ALL by the audit
    // test; GENERIC is user-configured and intentionally absent).
    "imap.gmail.com",
    "smtp.gmail.com",
    "imap.mail.me.com",
    "smtp.mail.me.com",
    "imap.zoho.com",
    "smtp.zoho.com",
    "imap.mail.yahoo.com",
    "smtp.mail.yahoo.com",
    "outlook.office365.com",
    "smtp-mail.outlook.com",
];

/// Records one contacted host (hostname only — no paths, no queries).
pub fn record(host: &str) {
    if host.is_empty() {
        return;
    }
    if let Ok(mut slot) = HOSTS.lock() {
        slot.get_or_insert_with(BTreeSet::new)
            .insert(host.to_lowercase());
    }
}

/// Host from a URL, recorded.
pub fn record_url(url: &str) {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            record(host);
        }
    }
}

/// The session's host list, sorted.
pub fn hosts() -> Vec<String> {
    HOSTS
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|set| set.iter().cloned().collect()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_dedups_hosts() {
        record("imap.gmail.com");
        record_url("https://huggingface.co/repo/resolve/abc/f.gguf");
        record_url("https://HUGGINGFACE.co/again");
        let hosts = hosts();
        assert!(hosts.contains(&"imap.gmail.com".to_string()));
        assert_eq!(
            hosts.iter().filter(|h| *h == "huggingface.co").count(),
            1,
            "dedup: {hosts:?}"
        );
        assert!(!hosts.iter().any(|h| h.contains("resolve")), "host only");
    }

    /// V-5.2: the static inventory must exactly cover every egress host the
    /// app's own code paths can reach — OAuth endpoints, the pinned model
    /// download host, the BYO AI engines, loopback, and every provider
    /// preset's IMAP/SMTP servers. User-content and user-configured hosts
    /// are excluded by design (D-044). `expected` is derived from the
    /// runtime sources themselves (parsed endpoint URLs + the provider
    /// presets), NOT from the inventory — so equality holds in both
    /// directions (D-046 corrected the one-sided version).
    #[test]
    fn static_inventory_covers_all_runtime_egress() {
        use crate::platform::providers::ALL;
        use url::Url;

        fn host_of(url: &str) -> Option<String> {
            Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        }

        // Runtime-derived egress sources (each also pinned by its module test).
        let mut expected: BTreeSet<String> = [
            // OAuth endpoints (oauth.rs consts).
            host_of(crate::platform::oauth::AUTHORIZE_URL),
            host_of(crate::platform::oauth::TOKEN_URL),
            host_of(crate::ai::siwc::AUTHORIZE_URL),
            host_of(crate::ai::siwc::TOKEN_URL),
            // Pinned model download host (models.rs).
            host_of(&crate::ai::models::MODELS[0].download_url()),
            // BYO engine endpoints (chat.rs).
            host_of(&crate::ai::chat::OpenAiCompat::openai("k", "m").base_url),
            host_of(&crate::ai::chat::OpenAiCompat::xai("k", "m").base_url),
            host_of(crate::ai::chat::ANTHROPIC_URL),
        ]
        .into_iter()
        .flatten()
        .collect();
        // llama-server + loopback image proxy.
        expected.insert(HOST_LOOPBACK.to_lowercase());

        // Provider presets: every IMAP/SMTP host must be in the inventory.
        // GENERIC is skipped — its hosts are user-supplied at add time.
        for p in ALL.iter() {
            if p.imap_host.is_empty() {
                continue;
            }
            expected.insert(p.imap_host.to_lowercase());
            expected.insert(p.smtp_host.to_lowercase());
        }

        let inventory: BTreeSet<String> = STATIC_EGRESS_HOSTS
            .iter()
            .map(|h| h.to_lowercase())
            .collect();

        let missing: Vec<&String> = expected.difference(&inventory).collect();
        assert!(
            missing.is_empty(),
            "runtime egress hosts missing from the audit inventory: {missing:?}"
        );
        let extra: Vec<&String> = inventory.difference(&expected).collect();
        assert!(
            extra.is_empty(),
            "inventory lists hosts no code path contacts: {extra:?}"
        );
        assert_eq!(inventory, expected, "audit must be exact set equality");
    }
}
