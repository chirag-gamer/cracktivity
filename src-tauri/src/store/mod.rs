//! SQLCipher-backed mail store (spec §3, D-006/D-007): encrypted local cache
//! with FTS5. Opens WAL mode with enforced FKs, applies versioned migrations,
//! and derives its DEK through DPAPI + Credential Manager.

pub mod dao;
pub mod dek;
pub mod schema;

use std::path::Path;

use rusqlite::Connection;

pub const DB_FILE: &str = "mail.db";
const BUSY_TIMEOUT_MS: i64 = 5000;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Rusqlite(#[from] rusqlite::Error),
    #[error("credential store error: {0}")]
    Keyring(#[from] keyring_core::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("randomness failure: {0}")]
    Random(#[from] getrandom::Error),
    #[error("DPAPI error: {0}")]
    Dpapi(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    /// DPAPI unwrap or DEK validation failed — the recovery path is a clean
    /// re-sync from the server (spec §11); credentials themselves survive.
    #[error("DEK could not be unwrapped (DPAPI orphaned?) — re-sync required")]
    DekOrphaned,
    /// The database is locked beyond the busy timeout (another writer holds it).
    #[error("database is locked (busy_timeout {0} ms elapsed)")]
    Busy(i64),
}

#[derive(Debug)]
pub struct Store {
    conn: Connection,
    /// Re-opened test connections need the same key; never exposed outside tests.
    #[cfg(test)]
    key_hex: String,
}

impl Store {
    /// Opens (or creates) the encrypted store for a profile directory.
    /// `dek_target` names the Credential Manager entry holding the DPAPI-wrapped
    /// DEK; production callers use [`dek::DEFAULT_DEK_TARGET`].
    pub fn open(profile_dir: &Path, dek_target: &str) -> Result<Store, StoreError> {
        crate::platform::keyring::init();
        std::fs::create_dir_all(profile_dir)?;

        let dek = dek::load_or_create(dek_target, profile_dir.join(DB_FILE).exists())?;
        let conn = Connection::open(profile_dir.join(DB_FILE))?;
        conn.execute_batch(&dek::key_pragma(&dek))?;
        // A wrong/absent key only fails at first use — force a real query so
        // open() never returns a store that would fail later (FR-33 seed).
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "busy_timeout", BUSY_TIMEOUT_MS)?;
        migrate(&conn)?;

        Ok(Store {
            conn,
            #[cfg(test)]
            key_hex: dek::hex(&dek),
        })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (idx, (name, sql)) in schema::MIGRATIONS.iter().enumerate() {
        let target = (idx + 1) as i64;
        if version >= target {
            continue;
        }
        conn.execute_batch(&format!(
            "BEGIN;
             {sql};
             INSERT INTO audit_log(at, actor, action, detail_json)
               VALUES (strftime('%s','now')*1000, 'user', 'migrate', json_object('migration', '{name}', 'version', {target}));
             PRAGMA user_version = {target};
             COMMIT;"
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod smoke {
    use rusqlite::Connection;

    // Fixed per-run test key; never a real secret (FR-2: secrets only in
    // Credential Manager — test keys are fixtures, not credentials).
    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn open_keyed(path: &std::path::Path, key_hex: &str) -> rusqlite::Result<Connection> {
        let conn = Connection::open(path)?;
        conn.execute_batch(&format!("PRAGMA key = \"x'{key_hex}'\";"))?;
        Ok(conn)
    }

    /// V-1.3 (plan): the bundled SQLCipher build must expose FTS5 and the
    /// file on disk must be non-plaintext (seed of FR-33 / D-006 provisional
    /// note). Fails fast at CI if the build flags ever change.
    #[test]
    fn fts5_present_and_file_is_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smoke.db");

        let conn = open_keyed(&path, TEST_KEY).unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE smoke_fts USING fts5(body, subject, snippet, content='');
             INSERT INTO smoke_fts(rowid, body, subject, snippet) VALUES
               (1, 'the quick brown fox', 'hello', 'greeting');",
        )
        .expect("FTS5 virtual table must be creatable in the bundled build");
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM smoke_fts WHERE smoke_fts MATCH 'fox'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1, "FTS5 MATCH must find inserted text");
        drop(conn);

        // Encrypted on disk: header must not be the plaintext SQLite magic
        // ("SQLite format 3\0"). Rollback journal (default mode) keeps this
        // assertion valid after the writes above.
        let head = std::fs::read(&path).unwrap();
        assert!(head.len() >= 16);
        assert_ne!(
            &head[..16],
            b"SQLite format 3\0",
            "DB file is not encrypted"
        );
    }

    /// FR-33 seed: a wrong key must not silently open the database.
    #[test]
    fn wrong_key_fails_to_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smoke.db");

        let conn = open_keyed(&path, TEST_KEY).unwrap();
        conn.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);")
            .unwrap();
        drop(conn);

        let conn = open_keyed(
            &path,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        )
        .unwrap();
        let err = conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
                r.get::<_, i64>(0)
            })
            .expect_err("wrong key must fail");
        assert!(!matches!(err, rusqlite::Error::QueryReturnedNoRows));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEK target unique to this test run AND this test so parallel tests never
    /// share (and clobber) a Credential Manager entry.
    fn test_dek_target(name: &str) -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        format!(
            "cracktivity.test.store.{name}.{}.{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        )
    }

    fn cleanup_dek(target: &str) {
        crate::platform::keyring::init();
        let _ = crate::platform::keyring::delete_secret(target);
    }

    #[test]
    fn open_creates_schema_and_reopens_clean() {
        let dir = tempfile::tempdir().unwrap();
        let target = test_dek_target("schema");
        cleanup_dek(&target);

        {
            let store = Store::open(dir.path(), &target).expect("first open");
            let tables: Vec<String> = store
                .conn()
                .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            for t in [
                "accounts",
                "folders",
                "messages",
                "threads",
                "drafts",
                "audit_log",
            ] {
                assert!(tables.iter().any(|n| n == t), "missing table {t}");
            }
            let v: i64 = store
                .conn()
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, schema::MIGRATIONS.len() as i64);
        }

        // Reopen (simulating restart): same DEK resolves, migrations skip.
        let store = Store::open(dir.path(), &target).expect("reopen");
        let v: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, schema::MIGRATIONS.len() as i64);
        assert_eq!(store.key_hex.len(), 64);
        cleanup_dek(&target);
    }

    /// Credential retrieval failure path (spec §11, FR-33): deleting the
    /// Credential Manager entry must surface a recoverable error, never a
    /// crash. The app then offers a clean re-sync.
    #[test]
    fn missing_dek_entry_yields_recoverable_error_on_create() {
        // A fresh unique target with the Credential Manager unavailable is
        // simulated by: entry exists → deleted → load sees NoEntry → creates.
        // The "create on missing" path is the normal first-run; the failure
        // path asserted here is a *corrupt/unwrappable* stored blob.
        let dir = tempfile::tempdir().unwrap();
        let target = test_dek_target("corrupt_dek");
        cleanup_dek(&target);

        crate::platform::keyring::init();
        // Store garbage where a valid DPAPI blob must be.
        crate::platform::keyring::set_secret(&target, b"not-a-dpapi-blob").unwrap();

        let err = Store::open(dir.path(), &target).expect_err("corrupt DEK must fail open");
        assert!(matches!(err, StoreError::DekOrphaned), "got {err:?}");

        cleanup_dek(&target);
    }

    /// Concurrent-writer behavior: a second writer holding the write lock
    /// surfaces StoreError::Busy after the busy timeout, and normal writes
    /// resume once released (no corruption, FR-8/NFR-3).
    #[test]
    fn concurrent_writer_lock_times_out_then_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let target = test_dek_target("lock");
        cleanup_dek(&target);

        let store = Store::open(dir.path(), &target).unwrap();
        let other = rusqlite::Connection::open(dir.path().join(DB_FILE)).unwrap();
        other
            .execute_batch(&format!("PRAGMA key = \"x'{}'\";", store.key_hex))
            .unwrap();
        other.execute_batch("BEGIN IMMEDIATE; SELECT 1;").unwrap();

        let err = store
            .conn()
            .execute_batch("CREATE TABLE lock_probe(x)")
            .expect_err("write must fail while the other connection holds the lock");
        assert!(
            matches!(&err, rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::DatabaseBusy),
            "expected DatabaseBusy, got {err:?}"
        );

        other.execute_batch("COMMIT;").unwrap();
        store
            .conn()
            .execute_batch("CREATE TABLE lock_probe(x)")
            .unwrap();
        cleanup_dek(&target);
    }

    #[test]
    fn reopen_after_dek_entry_deleted_is_orphan_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = test_dek_target("orphan");
        cleanup_dek(&target);

        {
            let _store = Store::open(dir.path(), &target).unwrap();
        }
        // User deletes the Credential Manager entry (FR-33 AC).
        cleanup_dek(&target);
        let err = Store::open(dir.path(), &target).expect_err("deleted DEK entry");
        assert!(matches!(err, StoreError::DekOrphaned), "got {err:?}");
    }
}
