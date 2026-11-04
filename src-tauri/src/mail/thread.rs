//! JWZ message threading (D-010) over the header fields the engine fetches.
//! Root identity = root message's Message-ID, or `subj:<normalized>` for
//! subject-grouped orphans. Provider-native X-GM-THRID short-circuits before
//! this runs (spec §4).

use std::collections::HashMap;

/// Inputs as persisted on `messages`.
#[derive(Debug, Clone)]
pub struct ThreadInput {
    /// Message row id (used as the returned grouping key).
    pub id: i64,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_raw: Option<String>,
    pub subject: Option<String>,
}

/// Output: row id → thread identity, plus the normalized subject of that
/// thread (for the `threads.subject_norm` column).
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadAssignment {
    pub id: i64,
    pub thrid: String,
    pub subject_norm: Option<String>,
}

struct Container {
    msg: Option<usize>, // index into the input vec
    parent: Option<String>,
    children: Vec<String>,
}

/// Subject normalization per JWZ §6: strip reply/forward prefixes repeatedly,
/// drop list tags, lowercase, collapse whitespace, strip punctuation.
pub fn normalize_subject(subject: Option<&str>) -> Option<String> {
    let mut s = subject?.trim().to_string();
    loop {
        let lower = s.to_ascii_lowercase();
        let stripped = ["re:", "fwd:", "fw:", "aw:", "sv:", "odp:", "antw:", "tr:"]
            .iter()
            .find_map(|p| {
                // Case-insensitive prefix match; keep the original casing after it.
                lower
                    .starts_with(*p)
                    .then(|| s.get(p.len()..))?
                    .map(|rest| rest.trim_start().to_string())
            });
        match stripped {
            Some(next) if !next.is_empty() => s = next,
            _ => break,
        }
    }
    // Drop mailing-list tags like [dev] at the start.
    if s.starts_with('[') {
        if let Some(end) = s.find(']') {
            s = s[end + 1..].trim_start().to_string();
        }
    }
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Assigns thread identities for a batch of messages (any folder mix of one
/// account; JWZ threads span folders in classic clients, but our per-account
/// threads keep threading within one account, spec §3).
pub fn assign_threads(messages: &[ThreadInput]) -> Vec<ThreadAssignment> {
    let mut containers: HashMap<String, Container> = HashMap::new();

    for (i, m) in messages.iter().enumerate() {
        // Reference chain: References then In-Reply-To, deduped, self excluded.
        let mut refs: Vec<String> = Vec::new();
        if let Some(r) = &m.references_raw {
            for tok in r.split_whitespace() {
                let tok = tok.trim_matches(|c| c == '<' || c == '>');
                if !tok.is_empty() && !refs.iter().any(|x| x == tok) {
                    refs.push(tok.to_string());
                }
            }
        }
        if let Some(irt) = &m.in_reply_to {
            let tok = irt.trim().trim_matches(|c| c == '<' || c == '>');
            if !tok.is_empty() && !refs.iter().any(|x| x == tok) {
                refs.push(tok.to_string());
            }
        }

        let own_id = m
            .message_id
            .as_deref()
            .map(|s| s.trim().trim_matches(|c| c == '<' || c == '>').to_string())
            .filter(|s| !s.is_empty());

        let mut prev: Option<String> = None;
        for r in &refs {
            get_or_make(&mut containers, r);
            if let Some(p) = &prev {
                link(&mut containers, p, r);
            }
            prev = Some(r.clone());
        }

        // Own container (by Message-ID or a synthetic placeholder).
        let own_key = own_id.clone().unwrap_or_else(|| format!("#synthetic:{i}"));
        get_or_make(&mut containers, &own_key).msg = Some(i);
        if let Some(p) = &prev {
            // Guard against reference loops pointing back at ourselves.
            if *p != own_key {
                link(&mut containers, p, &own_key);
            }
        }
    }

    // Root set: containers whose parent chain terminates. Since references
    // arrive per message in arbitrary order, walk up parents to the root.
    let mut out = Vec::with_capacity(messages.len());
    for (i, m) in messages.iter().enumerate() {
        let own_id = m
            .message_id
            .as_deref()
            .map(|s| s.trim().trim_matches(|c| c == '<' || c == '>').to_string())
            .filter(|s| !s.is_empty());
        let mut key = own_id.clone().unwrap_or_else(|| format!("#synthetic:{i}"));
        // Walk to the top of the reference chain.
        let mut hops = 0;
        while let Some(parent) = containers.get(&key).and_then(|c| c.parent.clone()) {
            key = parent;
            hops += 1;
            if hops > messages.len() + 1 {
                break; // defensive: loop guard for malformed reference cycles
            }
        }
        let root_msg = containers.get(&key).and_then(|c| c.msg);
        // A container with no parent and no children is a true singleton:
        // group those by normalized subject so same-subject orphans join.
        let isolated = containers
            .get(&key)
            .is_none_or(|c| c.parent.is_none() && c.children.is_empty());
        let thrid = if isolated {
            match normalize_subject(m.subject.as_deref()) {
                Some(s) => format!("subj:{s}"),
                None => format!("orphan:{key}"),
            }
        } else {
            match root_msg.and_then(|idx| messages[idx].message_id.clone()) {
                Some(mid) => format!("<{mid}>"),
                None => match normalize_subject(m.subject.as_deref()) {
                    Some(s) => format!("subj:{s}"),
                    None => format!("orphan:{key}"),
                },
            }
        };
        out.push(ThreadAssignment {
            id: m.id,
            thrid,
            subject_norm: normalize_subject(m.subject.as_deref()),
        });
    }
    out
}

fn get_or_make<'a>(containers: &'a mut HashMap<String, Container>, id: &str) -> &'a mut Container {
    containers
        .entry(id.to_string())
        .or_insert_with(|| Container {
            msg: None,
            parent: None,
            children: Vec::new(),
        })
}

fn link(containers: &mut HashMap<String, Container>, parent: &str, child: &str) {
    if parent == child {
        return;
    }
    // Prevent cycles: never make an ancestor a child of its own descendant.
    let mut cursor = child.to_string();
    let mut hops = 0;
    while let Some(p) = containers.get(&cursor).and_then(|c| c.parent.clone()) {
        if p == parent {
            return; // already linked; would create a cycle
        }
        cursor = p;
        hops += 1;
        if hops > 10_000 {
            return;
        }
    }
    if let Some(c) = containers.get_mut(child) {
        c.parent = Some(parent.to_string());
    }
    if let Some(p) = containers.get_mut(parent) {
        p.children.push(child.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: i64, mid: &str, irt: Option<&str>, subject: &str) -> ThreadInput {
        ThreadInput {
            id,
            message_id: Some(mid.to_string()),
            in_reply_to: irt.map(|s| s.to_string()),
            references_raw: None,
            subject: Some(subject.to_string()),
        }
    }

    #[test]
    fn reply_chain_groups_under_root() {
        let msgs = vec![
            msg(1, "a@x", None, "Launch plan"),
            msg(2, "b@x", Some("a@x"), "Re: Launch plan"),
            msg(3, "c@x", Some("b@x"), "Re: Launch plan"),
        ];
        let out = assign_threads(&msgs);
        assert_eq!(out[0].thrid, "<a@x>");
        assert_eq!(out[1].thrid, "<a@x>");
        assert_eq!(out[2].thrid, "<a@x>");
    }

    #[test]
    fn references_chain_outranks_missing_in_reply_to() {
        let msgs = vec![
            ThreadInput {
                id: 1,
                message_id: Some("a@x".into()),
                in_reply_to: None,
                references_raw: None,
                subject: Some("S".into()),
            },
            ThreadInput {
                id: 2,
                message_id: Some("c@x".into()),
                in_reply_to: None,
                references_raw: Some("<a@x> <b@x>".into()),
                subject: Some("S".into()),
            },
        ];
        let out = assign_threads(&msgs);
        assert_eq!(out[1].thrid, "<a@x>");
    }

    #[test]
    fn orphan_messages_group_by_normalized_subject() {
        let msgs = vec![
            msg(1, "a@x", None, "Re: [list] Meeting Notes!"),
            msg(2, "b@x", None, "meeting notes"),
        ];
        let out = assign_threads(&msgs);
        assert_eq!(out[0].thrid, "subj:meeting notes");
        assert_eq!(out[1].thrid, "subj:meeting notes");
    }

    #[test]
    fn forward_prefixes_normalize() {
        assert_eq!(
            normalize_subject(Some("FWD: AW: Re: Hello World")),
            normalize_subject(Some("hello world"))
        );
    }

    #[test]
    fn cyclic_references_terminate() {
        let msgs = vec![
            ThreadInput {
                id: 1,
                message_id: Some("a@x".into()),
                in_reply_to: Some("b@x".into()),
                references_raw: Some("<a@x>".into()),
                subject: Some("loop".into()),
            },
            ThreadInput {
                id: 2,
                message_id: Some("b@x".into()),
                in_reply_to: Some("a@x".into()),
                references_raw: Some("<b@x>".into()),
                subject: Some("loop".into()),
            },
        ];
        let out = assign_threads(&msgs);
        // Both must terminate on *some* identity without hanging.
        assert!(!out.is_empty());
    }
}
