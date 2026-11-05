//! In-process diagnostic log (Phase 5): a bounded ring buffer of app events
//! for the Settings → Diagnostics viewer. In-memory only — nothing is ever
//! written to disk (a log file could leak metadata; FR-32/D-015 posture).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CAP: usize = 500;

/// (unix_ms, "info" | "error", message)
type Line = (u64, &'static str, String);

static LINES: Mutex<VecDeque<Line>> = Mutex::new(VecDeque::new());

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Records one app event (`level`: "info" | "error"); oldest lines evicted.
pub fn record(level: &'static str, msg: impl Into<String>) {
    let line = (now_ms(), level, msg.into());
    if let Ok(mut q) = LINES.lock() {
        if q.len() >= CAP {
            q.pop_front();
        }
        q.push_back(line);
    }
}

/// The buffer, oldest first.
pub fn tail() -> Vec<Line> {
    LINES
        .lock()
        .ok()
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default()
}

/// Empties the buffer (user action from the Diagnostics viewer).
pub fn clear() {
    if let Ok(mut q) = LINES.lock() {
        q.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_caps_and_clears() {
        clear();
        for i in 0..(CAP + 25) {
            record("info", format!("line {i}"));
        }
        let lines = tail();
        assert_eq!(lines.len(), CAP);
        assert!(lines.last().unwrap().2.contains("line 524"), "newest kept");
        assert!(
            lines.first().unwrap().2.contains("line 25"),
            "oldest evicted"
        );
        clear();
        assert!(tail().is_empty());
    }
}
