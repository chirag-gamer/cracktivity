//! On-device importance classification (D-013): deterministic rules only —
//! instant, offline, zero egress (FR-12). A tiny learned layer may join later
//! (Phase 4); it must never gate delivery of these rules.
//!
//! Priority (first match wins):
//! 1. user correction for this sender (`sender_stats.classification_overrides`)
//! 2. bulk-shaped (`List-Unsubscribe` present) → Other
//! 3. security/alert signal in subject → Important
//! 4. addressed directly To: the account owner → Important
//! 5. sender with two-way history (`contacts.sent_to > 0`) → Important
//! 6. everything else (Cc:, Bcc:, unknown) → Other — review-later, never trash

use std::collections::HashMap;

use crate::store::dao::NewMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importance {
    Important,
    Other,
}

impl Importance {
    pub fn as_str(self) -> &'static str {
        match self {
            Importance::Important => "important",
            Importance::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "important" => Some(Importance::Important),
            "other" => Some(Importance::Other),
            _ => None,
        }
    }
}

/// Account-level context the rules need, loaded once per sync batch.
#[derive(Debug, Default, Clone)]
pub struct ClassifyContext {
    /// Lowercased addresses owned by this account's user.
    pub my_addresses: Vec<String>,
    /// Per-sender user corrections ("moving a message teaches it", FR-11).
    pub sender_overrides: HashMap<String, Importance>,
    /// Senders the user has actually written to (`contacts.sent_to > 0`).
    pub two_way_senders: Vec<String>,
}

/// Classification result with the rules that fired (shown in the UI and
/// stored in `importance_reasons_json`).
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub importance: Importance,
    pub reasons: Vec<&'static str>,
}

/// Subject phrases that mark security traffic; plain substring match with
/// word-boundary enforcement for short tokens. Curated, lower-case.
const SECURITY_PHRASES: [&str; 14] = [
    "verification code",
    "verify your",
    "one-time code",
    "one-time password",
    "security alert",
    "security code",
    "sign-in code",
    "sign in code",
    "sign-in attempt",
    "login code",
    "authentication code",
    "two-factor",
    "otp",
    "2fa",
];

/// `needle` occurring in `hay` not glued to surrounding alphanumerics —
/// keeps short tokens ("otp") from matching inside words ("hotpoint").
fn contains_word(hay: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let at = start + pos;
        let before_ok = hay[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = &hay[at + needle.len()..];
        let after_ok = after.chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}

fn is_security_signal(subject: &str) -> bool {
    let s = subject.to_lowercase();
    SECURITY_PHRASES.iter().any(|p| contains_word(&s, p))
}

fn addr_matches(addrs: &[crate::store::dao::Addr], mine: &[String]) -> bool {
    addrs
        .iter()
        .any(|a| mine.iter().any(|m| m == &a.email.to_lowercase()))
}

pub fn classify(msg: &NewMessage, ctx: &ClassifyContext) -> Verdict {
    let sender = msg
        .from
        .as_ref()
        .and_then(|f| f.first())
        .map(|a| a.email.to_lowercase())
        .unwrap_or_default();

    // 1. The user is always right.
    if let Some(over) = ctx.sender_overrides.get(&sender) {
        return Verdict {
            importance: *over,
            reasons: vec!["user-correction"],
        };
    }

    // 2. Bulk-shaped mail.
    if msg.list_unsubscribe.is_some() {
        return Verdict {
            importance: Importance::Other,
            reasons: vec!["list-unsubscribe"],
        };
    }

    // 3. Security traffic is Important even when Cc'd or from a no-reply.
    if let Some(subject) = msg.subject.as_deref() {
        if is_security_signal(subject) {
            return Verdict {
                importance: Importance::Important,
                reasons: vec!["security-signal"],
            };
        }
    }

    // 4. Directly addressed to me.
    if let Some(to) = msg.to.as_deref() {
        if addr_matches(to, &ctx.my_addresses) {
            return Verdict {
                importance: Importance::Important,
                reasons: vec!["to-you"],
            };
        }
    }

    // 5. Someone I have actually written to.
    if !sender.is_empty() && ctx.two_way_senders.iter().any(|s| s == &sender) {
        return Verdict {
            importance: Importance::Important,
            reasons: vec!["two-way-history"],
        };
    }

    Verdict {
        importance: Importance::Other,
        reasons: vec!["default"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::dao::Addr;

    fn ctx() -> ClassifyContext {
        ClassifyContext {
            my_addresses: vec!["me@example.com".into()],
            sender_overrides: HashMap::new(),
            two_way_senders: vec![],
        }
    }

    fn msg(subject: &str, from: &str, to: &str) -> NewMessage {
        NewMessage {
            subject: Some(subject.into()),
            from: Some(vec![Addr {
                name: None,
                email: from.into(),
            }]),
            to: Some(vec![Addr {
                name: None,
                email: to.into(),
            }]),
            ..Default::default()
        }
    }

    /// FR-11 AC, case 1: a seeded newsletter (List-Unsubscribe, not To: me)
    /// lands in Other.
    #[test]
    fn newsletter_lands_in_other() {
        let mut m = msg("Weekly digest", "news@corp.com", "me@example.com");
        m.list_unsubscribe = Some("<https://corp.com/unsub>".into());
        let v = classify(&m, &ctx());
        assert_eq!(v.importance, Importance::Other);
        assert_eq!(v.reasons, vec!["list-unsubscribe"]);
    }

    /// FR-11 AC, case 2: a personal reply to my thread lands in Important.
    #[test]
    fn personal_reply_lands_in_important() {
        let m = msg("Re: lunch", "friend@example.org", "me@example.com");
        assert_eq!(classify(&m, &ctx()).importance, Importance::Important);
    }

    /// FR-11 AC, case 3: a correction teaches the model for that sender.
    #[test]
    fn correction_updates_model_for_sender() {
        let m = msg("Your receipt", "shop@example.net", "me@example.com");
        let mut cx = ctx();
        cx.sender_overrides
            .insert("shop@example.net".into(), Importance::Important);
        let v = classify(&m, &cx);
        assert_eq!(v.importance, Importance::Important);
        assert_eq!(v.reasons, vec!["user-correction"]);
    }

    #[test]
    fn security_subject_is_important_even_from_noreply() {
        let m = msg(
            "Your sign-in code",
            "no-reply@example.com",
            "me@example.com",
        );
        assert_eq!(classify(&m, &ctx()).importance, Importance::Important);
    }

    /// Word-boundary check: "otp" must not fire inside "hotpoint". The mail
    /// is not To: me so only the security rule could make it Important.
    #[test]
    fn short_tokens_respect_word_boundaries() {
        let m = msg("Hotpoint oven sale", "shop@example.net", "all@corp.com");
        assert_eq!(classify(&m, &ctx()).importance, Importance::Other);

        let real = msg("Your OTP code", "shop@example.net", "all@corp.com");
        assert_eq!(
            classify(&real, &ctx()).importance,
            Importance::Important,
            "a genuine standalone token still fires"
        );
    }

    #[test]
    fn cc_only_mail_is_other_and_two_way_history_upgrades() {
        let cc_only = NewMessage {
            subject: Some("team update".into()),
            from: Some(vec![Addr {
                name: None,
                email: "boss@corp.com".into(),
            }]),
            to: Some(vec![Addr {
                name: None,
                email: "all@corp.com".into(),
            }]),
            ..Default::default()
        };
        assert_eq!(classify(&cc_only, &ctx()).importance, Importance::Other);

        let mut cx = ctx();
        cx.two_way_senders.push("boss@corp.com".into());
        let v = classify(&cc_only, &cx);
        assert_eq!(v.importance, Importance::Important);
        assert_eq!(v.reasons, vec!["two-way-history"]);
    }

    #[test]
    fn address_match_is_case_insensitive() {
        let m = msg("hi", "a@b.c", "Me@Example.COM");
        assert_eq!(classify(&m, &ctx()).importance, Importance::Important);
    }
}
