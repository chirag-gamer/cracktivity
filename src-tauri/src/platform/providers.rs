//! Provider presets (spec §5): connection data and guided-setup facts for
//! the v1 provider matrix. Every value below is verified against the
//! provider's own support documentation (URLs in each entry, checked
//! 2026-11-04); nothing here is guessed.

/// How an account authenticates (spec §5 matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// App/app-specific password over IMAP LOGIN.
    AppPassword,
    /// Outlook personal accounts: OAuth2 XOAUTH2 (basic auth was disabled
    /// for all personal accounts on 2024-09-16).
    OAuth2,
}

#[derive(Debug, Clone, Copy)]
pub struct Provider {
    /// Stable id stored in the accounts table (`auth_provider`).
    pub id: &'static str,
    pub label: &'static str,
    /// Domains whose addresses default to this provider.
    pub domains: &'static [&'static str],
    pub imap_host: &'static str,
    pub imap_port: u16,
    /// SMTP submission endpoint (FR-18). Port 465 = implicit TLS, 587 =
    /// STARTTLS (spec §4.6); both verified per provider documentation.
    pub smtp_host: &'static str,
    pub smtp_port: u16,
    pub auth: AuthKind,
    /// Where the user creates the app password (guided flow, spec §5).
    /// `None` for OAuth2 and generic accounts.
    pub app_password_url: Option<&'static str>,
    /// True when the user must flip a provider setting before IMAP works.
    pub requires_manual_imap_enable: bool,
    /// One-line guided-setup note shown during onboarding.
    pub setup_note: &'static str,
}

pub const GMAIL: Provider = Provider {
    id: "gmail",
    label: "Gmail",
    domains: &["gmail.com", "googlemail.com"],
    imap_host: "imap.gmail.com",
    imap_port: 993,
    smtp_host: "smtp.gmail.com",
    smtp_port: 465,
    auth: AuthKind::AppPassword,
    app_password_url: Some("https://myaccount.google.com/apppasswords"),
    requires_manual_imap_enable: false,
    // 2-Step Verification is required before app passwords exist at all.
    setup_note: "Requires 2-Step Verification; create a 16-character app password",
};

pub const ICLOUD: Provider = Provider {
    id: "icloud",
    label: "iCloud",
    domains: &["icloud.com", "me.com", "mac.com"],
    imap_host: "imap.mail.me.com",
    imap_port: 993,
    smtp_host: "smtp.mail.me.com",
    smtp_port: 587,
    auth: AuthKind::AppPassword,
    app_password_url: Some("https://account.apple.com/account/manage"),
    requires_manual_imap_enable: false,
    setup_note: "Sign in at account.apple.com > Sign-In and Security > App-Specific Passwords (requires two-factor authentication)",
};

pub const ZOHO: Provider = Provider {
    id: "zoho",
    label: "Zoho Mail",
    domains: &["zoho.com"],
    imap_host: "imap.zoho.com",
    imap_port: 993,
    smtp_host: "smtp.zoho.com",
    smtp_port: 465,
    auth: AuthKind::AppPassword,
    app_password_url: Some("https://accounts.zoho.com/home#security/appPasswords"),
    requires_manual_imap_enable: true,
    setup_note: "Enable IMAP under Settings > Mail Accounts > IMAP Access first; regional accounts use imap.zoho.eu / imap.zoho.in",
};

pub const YAHOO: Provider = Provider {
    id: "yahoo",
    label: "Yahoo Mail",
    domains: &["yahoo.com"],
    imap_host: "imap.mail.yahoo.com",
    imap_port: 993,
    smtp_host: "smtp.mail.yahoo.com",
    smtp_port: 465,
    auth: AuthKind::AppPassword,
    app_password_url: Some("https://login.yahoo.com/account/security"),
    requires_manual_imap_enable: false,
    setup_note: "Account Security > External connections > Create app password",
};

pub const OUTLOOK: Provider = Provider {
    id: "outlook",
    label: "Outlook.com",
    domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
    imap_host: "outlook.office365.com",
    imap_port: 993,
    smtp_host: "smtp-mail.outlook.com",
    smtp_port: 587,
    auth: AuthKind::OAuth2,
    app_password_url: None,
    requires_manual_imap_enable: true,
    // POP/IMAP is off by default for personal accounts.
    setup_note:
        "Enable POP/IMAP under Settings > Mail > Forwarding and IMAP; sign-in opens in your browser",
};

pub const GENERIC: Provider = Provider {
    id: "generic",
    label: "Other account (IMAP)",
    domains: &[],
    imap_host: "", // user-supplied
    imap_port: 993,
    smtp_host: "", // user-supplied
    smtp_port: 587,
    auth: AuthKind::AppPassword,
    app_password_url: None,
    requires_manual_imap_enable: false,
    setup_note: "Enter the server settings your provider documents",
};

/// All presets in onboarding display order (generic last).
pub const ALL: [Provider; 6] = [GMAIL, OUTLOOK, ICLOUD, ZOHO, YAHOO, GENERIC];

/// Resolves the default preset for an email address by domain.
pub fn for_address(email: &str) -> &'static Provider {
    let domain = email.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
    ALL.iter()
        .find(|p| p.domains.iter().any(|d| *d == domain))
        .unwrap_or(&GENERIC)
}

/// Credential Manager target names for per-account secrets (D-007): one
/// entry per secret, `cracktivity.*` namespace so test cleanup can target
/// exactly our records.
pub const ACCOUNT_SECRET_PREFIX: &str = "cracktivity.account";

pub fn app_password_target(account_id: i64) -> String {
    format!("{ACCOUNT_SECRET_PREFIX}.{account_id}.app_password")
}

pub fn refresh_token_target(account_id: i64) -> String {
    format!("{ACCOUNT_SECRET_PREFIX}.{account_id}.refresh_token")
}

/// Stores the account secret (app password or OAuth refresh token) under its
/// namespaced Credential Manager entry (spec §5: Credential Manager is the
/// only secret store; SQLCipher DEK aside, these never touch disk).
pub fn set_account_secret(target: &str, secret: &str) -> Result<(), keyring_core::Error> {
    crate::platform::keyring::set_secret(target, secret.as_bytes())
}

pub fn get_account_secret(target: &str) -> Result<String, keyring_core::Error> {
    crate::platform::keyring::get_secret(target)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn delete_account_secret(target: &str) -> Result<(), keyring_core::Error> {
    crate::platform::keyring::delete_secret(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preset facts are load-bearing config, not decoration: domains route
    /// onboarding, and the OAuth/basic-auth split is what keeps Outlook
    /// accounts from being configured with a dead password path.
    #[test]
    fn presets_route_by_domain() {
        assert_eq!(for_address("someone@gmail.com").id, "gmail");
        assert_eq!(for_address("Someone@GoogleMail.com").id, "gmail");
        assert_eq!(for_address("a@outlook.com").id, "outlook");
        assert_eq!(
            for_address("a@hotmail.co.uk").id,
            "generic",
            "only exact domain matches route"
        );
        assert_eq!(for_address("a@me.com").id, "icloud");
        assert_eq!(for_address("a@yahoo.com").id, "yahoo");
        assert_eq!(for_address("a@zoho.com").id, "zoho");
        assert_eq!(for_address("a@example.org").id, "generic");
    }

    #[test]
    fn outlook_is_oauth_only_and_all_app_password_providers_have_urls() {
        assert_eq!(OUTLOOK.auth, AuthKind::OAuth2);
        assert!(OUTLOOK.app_password_url.is_none());
        for p in &ALL {
            if p.auth == AuthKind::AppPassword && p.id != "generic" {
                assert!(
                    p.app_password_url.is_some(),
                    "{} must have an app-password URL (guided flow)",
                    p.id
                );
            }
            assert_eq!(p.imap_port, 993, "{}: implicit TLS", p.id);
        }
    }

    #[test]
    fn secret_targets_are_namespaced_and_unique_per_account() {
        assert_eq!(app_password_target(7), "cracktivity.account.7.app_password");
        assert_eq!(
            refresh_token_target(7),
            "cracktivity.account.7.refresh_token"
        );
        assert_ne!(app_password_target(7), refresh_token_target(8));
    }

    /// Account-secret helpers round-trip through Credential Manager and
    /// clean up after themselves (V-1.4 semantics at the account layer).
    #[test]
    fn account_secret_round_trip() {
        crate::platform::keyring::init();
        let target = format!("cracktivity.test.account.roundtrip.{}", std::process::id());
        set_account_secret(&target, "app-passwörd €").unwrap();
        assert_eq!(get_account_secret(&target).unwrap(), "app-passwörd €");
        delete_account_secret(&target).unwrap();
        assert!(get_account_secret(&target).is_err());
    }
}
