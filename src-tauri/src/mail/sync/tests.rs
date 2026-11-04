//! L3 sync-engine suite (spec §13, plan V-2.1/V-2.3): scripted-mock scenarios
//! for delta sync, flag diff, expunge, reconnect, UIDVALIDITY re-baseline and
//! kill-safe backfill resumability.

use std::collections::BTreeMap;

use super::*;
use crate::platform::keyring;
use crate::store::dao;

fn dek_target() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    format!(
        "cracktivity.test.sync.{}.{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

fn cleanup(dek_target: &str) {
    keyring::init();
    let _ = keyring::delete_secret(dek_target);
}

struct Env {
    _dir: tempfile::TempDir,
    store: Store,
    account_id: i64,
    dek_target: String,
}

impl Drop for Env {
    fn drop(&mut self) {
        cleanup(&self.dek_target);
    }
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let dek_target = dek_target();
    cleanup(&dek_target);
    let store = Store::open(dir.path(), &dek_target).unwrap();
    let account_id = dao::add_account(
        store.conn(),
        &dao::NewAccount {
            provider: "imap",
            email: "me@example.com",
            display_name: None,
            auth_type: "app_password",
            credential_ref: "ref",
            imap_host: None,
            imap_port: None,
        },
    )
    .unwrap();
    Env {
        _dir: dir,
        store,
        account_id,
        dek_target,
    }
}

fn msg(
    subject: &str,
    message_id: &str,
    in_reply_to: Option<&str>,
    from: &str,
    flags: &[&str],
) -> crate::mail::mock::MockMessage {
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    headers.insert("subject".into(), subject.into());
    headers.insert("message-id".into(), message_id.into());
    headers.insert("from".into(), from.into());
    if let Some(irt) = in_reply_to {
        headers.insert("in-reply-to".into(), irt.into());
    }
    crate::mail::mock::MockMessage {
        uid: 0, // assigned by push()
        flags: flags.iter().map(|s| s.to_string()).collect(),
        headers,
        thrid: None,
        body: crate::mail::transport::BodyData {
            text: Some(format!("body of {subject}")),
            html: None,
        },
        internaldate_ms: dao::now_ms(),
        size: 512,
    }
}

fn mailbox_with(n: usize) -> crate::mail::mock::MockMailbox {
    let mut mb = crate::mail::mock::MockMailbox {
        name: "INBOX".into(),
        uidvalidity: 42,
        ..Default::default()
    };
    for i in 0..n {
        let m = msg(
            &format!("Subject {i}"),
            &format!("m{i}@x"),
            None,
            "sender@example.com",
            &[],
        );
        mb.push(m);
    }
    mb
}

fn count(conn: &rusqlite::Connection, folder_id: i64) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM messages WHERE folder_id = ?1",
        [folder_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn initial_backfill_populates_and_completes() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(7));

    let out = super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(out.backfilled, 7);
    assert!(!out.rebaselined);

    let folder = dao::folder_state_by_name(env.store.conn(), env.account_id, "INBOX")
        .unwrap()
        .unwrap();
    assert_eq!(count(env.store.conn(), folder.id), 7);
    assert_eq!(folder.highest_uid, 1, "backfill complete marker");
    assert_eq!(folder.uidnext, 8, "UIDNEXT = next assignable UID (1-based)");

    // Threading + contacts bookkeeping ran.
    let threads: i64 = env
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM threads WHERE account_id = ?1",
            [env.account_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(threads, 7);
    let received: i64 = env
        .store
        .conn()
        .query_row(
            "SELECT received FROM sender_stats WHERE sender_email = 'sender@example.com'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(received, 7);

    // FTS (FR-9 local path) finds a synced subject.
    let hits = dao::search(env.store.conn(), "Subject", 10).unwrap();
    assert_eq!(hits.len(), 7);
}

#[test]
fn new_mail_syncs_incrementally() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(3));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    // New mail arrives server-side between syncs.
    t.active_mailbox()
        .push(msg("fresh", "fresh@x", None, "new@example.com", &[]));
    let out = super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(out.fetched_new, 1);
    assert_eq!(out.backfilled, 0, "no re-backfill in steady state");

    let folder = dao::folder_state_by_name(env.store.conn(), env.account_id, "INBOX")
        .unwrap()
        .unwrap();
    assert_eq!(count(env.store.conn(), folder.id), 4);
}

#[test]
fn flag_changes_reconcile_both_ways() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(3));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    // Server-side: message 1 marked seen.
    t.active_mailbox().set_flags(1, vec!["\\Seen".into()]);
    let out = super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(out.flags_updated, 1);

    let unread: i64 = env
        .store
        .conn()
        .query_row("SELECT unread FROM messages WHERE uid = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(unread, 0, "\\Seen must flip unread off");
}

#[test]
fn expunge_reconciles() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(3));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    t.active_mailbox().messages.remove(&2);
    let out = super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(out.expunged, 1);
    let folder = dao::folder_state_by_name(env.store.conn(), env.account_id, "INBOX")
        .unwrap()
        .unwrap();
    assert_eq!(count(env.store.conn(), folder.id), 2);
}

#[test]
fn uidvalidity_change_rebaselines_without_loss() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(3));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    // Server resets the mailbox (fresh UIDVALIDITY, fresh UIDs).
    let mb = t.active_mailbox();
    mb.uidvalidity = 99;
    mb.uidnext = 0;
    mb.messages.clear();
    mb.push(msg("rebased", "r1@x", None, "sender@example.com", &[]));

    let out = super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();
    assert!(out.rebaselined);
    let folder = dao::folder_state_by_name(env.store.conn(), env.account_id, "INBOX")
        .unwrap()
        .unwrap();
    assert_eq!(
        count(env.store.conn(), folder.id),
        1,
        "old UID rows dropped, new ones fetched"
    );
    assert_eq!(folder.uidvalidity, 99);
}

#[test]
fn kill_mid_backfill_resumes_without_duplicates() {
    let env = env();
    // 12 messages, batch size 5 → 3 fetch batches. Inject a disconnect after
    // enough calls for two batches (list + select + 2 fetches = 4 calls).
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(12));
    t.fail_after_calls = Some(4);

    let opts = SyncOptions {
        backfill_days: 90,
        batch_size: 5,
    };
    let err = super::sync_folder(&mut t, &env.store, env.account_id, "INBOX", &opts)
        .expect_err("injected disconnect");
    assert!(matches!(
        err,
        SyncError::Transport(TransportError::Disconnected(_))
    ));

    // Partial state survived (WAL kill-safety), cursor below completion.
    let conn = env.store.conn();
    let partial: i64 = conn
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert!(
        partial > 0 && partial < 12,
        "partial backfill persisted, got {partial}"
    );
    let cursor: u32 = conn
        .query_row("SELECT highest_uid FROM folders", [], |r| r.get(0))
        .unwrap();
    assert!(cursor > 1, "cursor mid-backfill, got {cursor}");

    // Reconnect (fresh transport) and resume: completes, no duplicates.
    let mut t2 = crate::mail::mock::MockTransport::new(t.mailboxes.first().unwrap().clone());
    let out = super::sync_folder(&mut t2, &env.store, env.account_id, "INBOX", &opts).unwrap();
    assert_eq!(out.backfilled, 12 - partial as usize);
    let total: i64 = conn
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 12);
    let distinct: i64 = conn
        .query_row("SELECT count(DISTINCT uid) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(distinct, 12);
    let cursor: u32 = conn
        .query_row("SELECT highest_uid FROM folders", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cursor, 1, "backfill complete");
}

#[test]
fn idle_events_drive_store_updates() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(2));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();
    let folder = dao::folder_state_by_name(env.store.conn(), env.account_id, "INBOX")
        .unwrap()
        .unwrap();

    // EXISTS: new message arrived.
    t.active_mailbox()
        .push(msg("idle-new", "idle@x", None, "idle@example.com", &[]));
    let out = super::apply_idle_event(
        &mut t,
        &env.store,
        env.account_id,
        folder.id,
        IdleEvent::Exists(vec![3]),
    )
    .unwrap();
    assert_eq!(out.fetched_new, 1);

    // FLAGS: server marked it seen.
    t.active_mailbox().set_flags(3, vec!["\\Seen".into()]);
    let out = super::apply_idle_event(
        &mut t,
        &env.store,
        env.account_id,
        folder.id,
        IdleEvent::FlagsChanged(vec![3]),
    )
    .unwrap();
    assert_eq!(out.flags_updated, 1);
    let unread: i64 = env
        .store
        .conn()
        .query_row("SELECT unread FROM messages WHERE uid = 3", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(unread, 0);

    // EXPUNGE.
    let out = super::apply_idle_event(
        &mut t,
        &env.store,
        env.account_id,
        folder.id,
        IdleEvent::Expunged(vec![3]),
    )
    .unwrap();
    assert_eq!(out.expunged, 1);
    assert_eq!(count(env.store.conn(), folder.id), 2);
}

#[test]
fn body_prefetch_fills_lazy_bodies() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(3));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    // Headers only: bodies are NULL until prefetch runs (spec §4.5).
    let bodies: i64 = env
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM messages WHERE body_text IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bodies, 0);

    let folder = dao::folder_state_by_name(env.store.conn(), env.account_id, "INBOX")
        .unwrap()
        .unwrap();
    let fetched = super::fetch_missing_bodies(&mut t, &env.store, folder.id, 10).unwrap();
    assert_eq!(fetched, 3);
    let text: String = env
        .store
        .conn()
        .query_row("SELECT body_text FROM messages WHERE uid = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(text, "body of Subject 0");
}

#[test]
fn reply_thread_groups_across_syncs() {
    let env = env();
    let mut t = crate::mail::mock::MockTransport::new(mailbox_with(1));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    t.active_mailbox().push(msg(
        "Re: Subject 0",
        "reply@x",
        Some("m0@x"),
        "other@example.com",
        &[],
    ));
    super::sync_folder(
        &mut t,
        &env.store,
        env.account_id,
        "INBOX",
        &Default::default(),
    )
    .unwrap();

    let thrids: Vec<String> = env
        .store
        .conn()
        .prepare("SELECT DISTINCT thrid FROM messages ORDER BY uid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        thrids.len(),
        1,
        "reply must join the root's thread, got {thrids:?}"
    );
    assert_eq!(thrids[0], "<m0@x>");
}

/// V-2.5 (FR-2/FR-33): neither the raw DEK nor a planted secret marker may
/// exist anywhere in the on-disk app data directory.
#[test]
fn secrets_never_reach_disk() {
    let dir = tempfile::tempdir().unwrap();
    let dek_target = format!("cracktivity.test.secrets.{}", std::process::id());
    cleanup(&dek_target);

    keyring::init();
    // Plant a distinctive "app password" the way the account flow would.
    let secret = b"app-password-MARKER-0xDEADBEEF";
    keyring::set_secret("cracktivity.test.acct", secret).unwrap();

    let store = Store::open(dir.path(), &dek_target).unwrap();
    let _ = dao::add_account(
        store.conn(),
        &dao::NewAccount {
            provider: "imap",
            email: "x@y.z",
            display_name: None,
            auth_type: "app_password",
            credential_ref: "cracktivity.test.acct",
            imap_host: None,
            imap_port: None,
        },
    );

    // Load the DEK exactly as the app does, to have its raw bytes to grep for.
    let wrapped = keyring::get_secret(&dek_target).unwrap();
    let dek = crate::platform::dpapi::unprotect(&wrapped).unwrap();

    let mut leaked = Vec::new();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        if bytes.windows(secret.len()).any(|w| w == secret) {
            leaked.push(format!("app-password in {}", path.display()));
        }
        if bytes.windows(dek.len()).any(|w| w == dek) {
            leaked.push(format!("raw DEK in {}", path.display()));
        }
    }
    keyring::delete_secret("cracktivity.test.acct").unwrap();
    cleanup(&dek_target);
    assert!(leaked.is_empty(), "secrets on disk: {leaked:?}");
}

#[test]
fn backoff_is_exponential_with_cap() {
    assert_eq!(super::backoff_delay(0), Duration::from_secs(1));
    assert_eq!(super::backoff_delay(1), Duration::from_secs(2));
    assert_eq!(super::backoff_delay(3), Duration::from_secs(8));
    assert_eq!(super::backoff_delay(7), Duration::from_secs(60));
    assert_eq!(super::backoff_delay(50), Duration::from_secs(60), "capped");
}

/// Phase 3: classification runs inside the sync path (D-013) — the stored
/// `importance` column distinguishes newsletters from direct mail with zero
/// network beyond the mail transport itself (FR-12 AC's store-side seed).
#[test]
fn sync_classifies_newsletters_and_direct_mail() {
    let e = env();
    let mut mb = crate::mail::mock::MockMailbox {
        name: "INBOX".into(),
        uidvalidity: 7,
        ..Default::default()
    };
    let mut news = msg("Weekly digest", "n1@x", None, "news@corp.com", &[]);
    news.headers.insert("to".into(), "me@example.com".into());
    news.headers
        .insert("list-unsubscribe".into(), "<https://corp.com/unsub>".into());
    mb.push(news);
    let mut direct = msg("Lunch tomorrow?", "p1@x", None, "friend@example.org", &[]);
    direct.headers.insert("to".into(), "me@example.com".into());
    mb.push(direct);
    // A second mail from the newsletter sender *after* the user corrected it
    // teaches via sender_stats (FR-11 AC, verified at the rules level in
    // classify::tests; here we assert the plain pre-correction buckets).
    let mut t = crate::mail::mock::MockTransport::new(mb);
    t.folders = vec![crate::mail::transport::FolderInfo {
        name: "INBOX".into(),
        role: Some(crate::mail::transport::SpecialRole::Inbox),
        subscribed: true,
    }];

    sync_folder(
        &mut t,
        &e.store,
        e.account_id,
        "INBOX",
        &SyncOptions::default(),
    )
    .unwrap();

    let rows: Vec<(String, Option<String>)> = e
        .store
        .conn()
        .prepare("SELECT subject, importance FROM messages ORDER BY subject")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        ("Lunch tomorrow?".into(), Some("important".into())),
        "direct mail to the account owner"
    );
    assert_eq!(
        rows[1],
        ("Weekly digest".into(), Some("other".into())),
        "List-Unsubscribe bulk mail"
    );
}
