//! Session host log (FR-32 evidence, D-015): every remote host this process
//! contacted, in-process only (no telemetry — this never leaves the
//! machine). The Diagnostics panel lists exactly what the egress layer did.

use std::collections::BTreeSet;
use std::sync::Mutex;

static HOSTS: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);

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
}
