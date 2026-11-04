//! Credential Manager wrapper (D-007): every secret is one generic credential
//! with an explicit `cracktivity.*` target name and Local persistence.
//!
//! The wrapper owns store initialization so `Entry` creation can never race
//! an unset default store (keyring-core returns `NoDefaultStore` otherwise).

use std::collections::HashMap;
use std::sync::{Mutex, Once, OnceLock};

static INIT: Once = Once::new();

// Windows Credential Manager does not guarantee that concurrent CredWriteW/
// CredReadW calls (even on different entries) are visible in call order —
// documented by uv-keyring, keyring-rs#163 and measured by downstream users:
// parallel set→get cycles miss 10%+ of reads; serialized access misses zero.
// cracktivity touches credentials only at startup (sequential), but the
// wrapper serializes anyway so correctness does not depend on callers.
static CRED_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cred_lock() -> std::sync::MutexGuard<'static, ()> {
    CRED_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Installs the Windows-native store as the keyring default. Idempotent;
/// call once at startup before any entry is created (spec §5).
pub fn init() {
    INIT.call_once(|| {
        keyring_core::set_default_store(
            windows_native_keyring_store::Store::new().expect(
                "windows-native-keyring-store must initialize (Windows Credential Manager)",
            ),
        );
    });
}

/// Service prefix for every cracktivity credential target name. Kept in one
/// place so `cmdkey /list` entries are recognizable and test cleanup can
/// target exactly our namespace.
pub const SERVICE: &str = "cracktivity";

/// Creates an entry under our namespace with Local persistence (D-007:
/// persist = Local; Enterprise is the store's default and must be overridden).
fn entry_for(target: &str) -> keyring_core::Entry {
    let modifiers: HashMap<&str, &str> =
        HashMap::from([("target", target), ("persistence", "Local")]);
    keyring_core::Entry::new_with_modifiers(SERVICE, "app", &modifiers)
        .expect("credential entry name must be valid")
}

pub fn set_secret(target: &str, secret: &[u8]) -> Result<(), keyring_core::Error> {
    let _g = cred_lock();
    entry_for(target).set_secret(secret)
}

pub fn get_secret(target: &str) -> Result<Vec<u8>, keyring_core::Error> {
    let _g = cred_lock();
    entry_for(target).get_secret()
}

pub fn delete_secret(target: &str) -> Result<(), keyring_core::Error> {
    let _g = cred_lock();
    entry_for(target).delete_credential()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring_core::Error;

    /// V-1.4 (plan): resolves the Phase-0 open question about byte encoding —
    /// secrets round-trip byte-exactly (UTF-8, embedded NUL, 0xFF) through
    /// CredWriteW/CredReadW, with the requested Local persistence.
    #[test]
    fn byte_round_trip_and_persistence_local() {
        init();
        // Uses the raw Entry API (attributes/password paths); take the shared
        // lock so parallel tests cannot race this test's CredReadW/CredWriteW.
        let _g = cred_lock();
        let target = format!("{SERVICE}.test.roundtrip.{}", std::process::id());
        let entry = entry_for(&target);

        let secret: &[u8] = b"app-\xC2\xB7-password\x00\xFFtail";
        entry.set_secret(secret).expect("set_secret");
        assert_eq!(entry.get_secret().expect("get_secret"), secret);

        // String path must agree for UTF-8 secrets (error paths checked below
        // are the documented keyring-core BadEncoding case for non-UTF-8 blobs).
        entry.set_password("pässwörd €").expect("set_password");
        assert_eq!(entry.get_password().expect("get_password"), "pässwörd €");

        let attrs = entry.get_attributes().expect("get_attributes");
        assert_eq!(
            attrs.get("persistence").map(String::as_str),
            Some("Local"),
            "D-007 requires persist=Local; store default is Enterprise"
        );
        assert_eq!(
            attrs.get("target_name").map(String::as_str),
            Some(target.as_str())
        );

        entry.delete_credential().expect("delete_credential");
        assert!(matches!(entry.get_secret(), Err(Error::NoEntry)));
    }

    /// Resolves the second Phase-0 open question: the Credential Manager
    /// CRED_MAX_CREDENTIAL_BLOB_SIZE cap (2560 bytes) — secrets larger than
    /// the cap must fail loudly, never truncate (D-007).
    #[test]
    fn oversized_secret_is_rejected_not_truncated() {
        init();
        let _g = cred_lock();
        let target = format!("{SERVICE}.test.oversize.{}", std::process::id());
        let entry = entry_for(&target);

        let big = vec![0x61u8; 2561];
        // Any rejection is acceptable; the load-bearing assertion is that
        // nothing got stored (checked below), never truncated.
        if entry.set_secret(&big).is_ok() {
            panic!("expected oversized secret to be rejected by the cap");
        }
        assert!(matches!(
            entry.get_secret(),
            Err(Error::NoEntry) | Err(Error::BadEncoding(_))
        ));
        let _ = entry.delete_credential();
    }

    /// Regression test for the Credential Manager visibility race: concurrent
    /// set→get→delete cycles on independent entries must not miss (the wrapper
    /// serializes access; see the module comment on CRED_LOCK).
    #[test]
    fn concurrent_set_get_delete_never_misses() {
        init();
        let mut handles = vec![];
        for t in 0..8 {
            handles.push(std::thread::spawn(move || {
                let mut misses = 0;
                for i in 0..40 {
                    let target = format!("{SERVICE}.test.stress.{t}.{i}.{}", std::process::id());
                    let blob = format!("blob-{t}-{i}").into_bytes();
                    set_secret(&target, &blob).expect("set");
                    match get_secret(&target) {
                        Ok(got) => {
                            if got != blob {
                                misses += 1;
                                eprintln!("MISMATCH t{t} i{i}");
                            }
                        }
                        Err(Error::NoEntry) => {
                            misses += 1;
                            eprintln!("NOENTRY t{t} i{i}");
                        }
                        Err(e) => panic!("unexpected {e:?}"),
                    }
                    delete_secret(&target).expect("delete");
                }
                misses
            }));
        }
        let total: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(total, 0, "credential set→get misses under concurrency");
    }
}
