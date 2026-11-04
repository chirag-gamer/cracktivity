//! Minimal, tolerant parsing of the limited header set the sync engine
//! fetches (spec §4.3). Mail data is attacker-controlled: parsing never
//! panics — malformed input yields `None`/empty, never an error path into
//! the UI.

use std::collections::BTreeMap;

use crate::store::dao::Addr;

/// Unfolds continuation lines and maps lower-cased field names to values.
/// A repeated single-valued field keeps its last occurrence; repeated
/// multi-valued fields (Received-like) would be joined, but the engine only
/// requests the fields in spec §4.3.
pub fn parse_header_block(raw: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in raw.lines() {
        if line.is_empty() {
            current = None; // end of headers
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = &current {
                out.entry(key.clone()).and_modify(|v| {
                    v.push(' ');
                    v.push_str(line.trim());
                });
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            current = Some(key.clone());
            out.insert(key, value.trim().to_string());
        } else {
            current = None;
        }
    }
    out
}

/// Best-effort RFC 5322 address-list parse for display: `Name <a@b>`,
/// `a@b`, and quoted display names, comma separated. Unparseable entries are
/// skipped rather than failing the sync.
pub fn parse_address_list(raw: &str) -> Vec<Addr> {
    let mut out = Vec::new();
    for part in split_address_list(raw) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, email) = split_name_addr(part);
        if is_plausible_email(&email) {
            out.push(Addr { name, email });
        }
    }
    out
}

fn split_address_list(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize; // angle brackets
    let mut in_quotes = false;
    let mut cur = String::new();
    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            '<' if !in_quotes => {
                depth += 1;
                cur.push(ch);
            }
            '>' if !in_quotes => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            ',' if depth == 0 && !in_quotes => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

fn split_name_addr(part: &str) -> (Option<String>, String) {
    let lt = part.rfind('<');
    let gt = part.rfind('>');
    if let (Some(lt), Some(gt)) = (lt, gt) {
        if gt > lt {
            let email = part[lt + 1..gt].trim().to_string();
            let name = part[..lt].trim().trim_matches('"').trim().to_string();
            return (if name.is_empty() { None } else { Some(name) }, email);
        }
    }
    (None, part.trim().to_string())
}

fn is_plausible_email(s: &str) -> bool {
    match (s.find('@'), s.rfind('@')) {
        (Some(at), Some(last)) if at > 0 && last < s.len() - 1 && at == last => {
            !s.contains(' ') && !s.contains('<') && !s.contains('>')
        }
        _ => false,
    }
}

/// Parses an RFC 2822 date into epoch ms; `None` on anything we can't read
/// (the caller falls back to INTERNALDATE).
pub fn parse_rfc2822_date(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc2822(raw.trim())
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_continuation_lines_and_lowercases() {
        let raw = "From: Ada <ada@example.com>\r\nSubject: Hello\r\n\tWorld\r\nX-A: 1\r\n";
        let h = parse_header_block(raw);
        assert_eq!(h.get("from").unwrap(), "Ada <ada@example.com>");
        assert_eq!(h.get("subject").unwrap(), "Hello World");
        assert_eq!(h.get("x-a").unwrap(), "1");
    }

    #[test]
    fn malformed_lines_never_panic() {
        for raw in [
            "",
            "no colon line\r\n: novalue",
            "From:\r\n From  \r\n\r\nbody",
        ] {
            let _ = parse_header_block(raw);
        }
    }

    #[test]
    fn parses_address_forms() {
        let addrs = parse_address_list(
            "Ada Lovelace <ada@example.com>, bob@corp.io, \"Q, Ltd\" <q@ltd.io>",
        );
        assert_eq!(addrs.len(), 3);
        assert_eq!(
            addrs[0],
            Addr {
                name: Some("Ada Lovelace".into()),
                email: "ada@example.com".into()
            }
        );
        assert_eq!(addrs[1].email, "bob@corp.io");
        assert_eq!(addrs[2].name.as_deref(), Some("Q, Ltd"));
    }

    #[test]
    fn rfc2822_date_parses() {
        let ms = parse_rfc2822_date("Mon, 07 Nov 2022 10:00:00 +0000").unwrap();
        assert_eq!(ms, 1_667_815_200_000);
        assert!(parse_rfc2822_date("not a date").is_none());
    }
}
