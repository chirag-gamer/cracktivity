//! DEK lifecycle (D-007, spec §5): random 32-byte key → DPAPI-wrapped →
//! wrapped blob in Credential Manager. Unwrapped only in memory at DB open.
//! The Credential Manager entry is the only place the wrapped key lives.

use crate::platform::{dpapi, keyring};

pub const DEK_LEN: usize = 32;
pub const DEFAULT_DEK_TARGET: &str = "cracktivity.dek";

pub fn random_dek() -> Result<[u8; DEK_LEN], getrandom::Error> {
    let mut dek = [0u8; DEK_LEN];
    getrandom::fill(&mut dek)?;
    Ok(dek)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// SQLCipher PRAGMA key literal for a raw binary key (`x'…'` form — the
/// documented way to pass raw key material; hex-string keys would be hashed).
pub fn key_pragma(dek: &[u8]) -> String {
    format!("PRAGMA key = \"x'{}'\";", hex(dek))
}

/// Loads the DEK, creating + persisting a wrapped one on first run.
/// `dek_target` names the Credential Manager entry (tests use a unique one;
/// production uses `DEFAULT_DEK_TARGET`). `db_exists` distinguishes a fresh
/// profile (create a DEK) from a lost entry next to an existing encrypted DB —
/// that is the spec §11 orphan/recovery case, never a silent re-key.
pub fn load_or_create(dek_target: &str, db_exists: bool) -> Result<Vec<u8>, super::StoreError> {
    match keyring::get_secret(dek_target) {
        Ok(wrapped) => {
            let dek = dpapi::unprotect(&wrapped).map_err(|_| super::StoreError::DekOrphaned)?;
            if dek.len() != DEK_LEN {
                return Err(super::StoreError::DekOrphaned);
            }
            Ok(dek)
        }
        Err(keyring_core::Error::NoEntry) if db_exists => {
            // The only DEK reference is gone while encrypted mail data exists.
            Err(super::StoreError::DekOrphaned)
        }
        Err(keyring_core::Error::NoEntry) => {
            let dek = random_dek()?;
            let wrapped = dpapi::protect(&dek).map_err(|e| super::StoreError::Dpapi(e.0))?;
            keyring::set_secret(dek_target, &wrapped)?;
            Ok(dek.to_vec())
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::keyring;

    #[test]
    fn dek_round_trip_via_credential_manager() {
        keyring::init();
        let target = format!("cracktivity.test.dek.{}", std::process::id());
        let _ = keyring::delete_secret(&target); // clean slate if a prior run died

        let first = load_or_create(&target, false).expect("create dek");
        assert_eq!(first.len(), DEK_LEN);

        // Second load must return the *same* key (persistence works).
        let second = load_or_create(&target, true).expect("reload dek");
        assert_eq!(first, second);

        // The Credential Manager blob must be DPAPI-wrapped, never plaintext.
        let stored = keyring::get_secret(&target).unwrap();
        assert_ne!(
            stored, first,
            "raw DEK must never sit in Credential Manager"
        );

        keyring::delete_secret(&target).unwrap();
    }

    /// Spec §11: a lost Credential Manager entry next to existing encrypted
    /// mail data is the orphan case — recoverable error, never a silent re-key.
    #[test]
    fn lost_dek_entry_with_existing_db_is_orphan() {
        keyring::init();
        let target = format!("cracktivity.test.dek.orphan.{}", std::process::id());
        let _ = keyring::delete_secret(&target);

        let err = load_or_create(&target, true).expect_err("existing DB without DEK entry");
        assert!(matches!(err, super::super::StoreError::DekOrphaned));

        // Fresh profile (no DB) still creates a new DEK.
        let created = load_or_create(&target, false).expect("fresh profile creates DEK");
        assert_eq!(created.len(), DEK_LEN);
        keyring::delete_secret(&target).unwrap();
    }

    #[test]
    fn key_pragma_uses_raw_hex_form() {
        assert_eq!(key_pragma(&[0x00, 0xff]), "PRAGMA key = \"x'00ff'\";");
    }
}
