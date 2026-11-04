//! User actions over the mail store (FR-30 keys e/u/s, FR-11 corrections):
//! every mutation goes to the server first when a transport is connected,
//! then mirrors locally, so flag reconciliation never fights the user.
//! Offline flag edits are intentionally rejected (v1 has no outbound queue);
//! reading/searching stay fully offline (FR-8).

use crate::mail::transport::MailTransport;
use crate::store::dao;
use crate::store::{queries, Store, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("offline — try again when the account is connected")]
    Offline,
    #[error("message not found")]
    NotFound,
    #[error("no archive folder on this account")]
    NoArchiveFolder,
    #[error("transport: {0}")]
    Transport(#[from] crate::mail::transport::TransportError),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),
}

fn address_of(store: &Store, message_id: i64) -> Result<queries::MessageAddress, ActionError> {
    queries::message_address(store.conn(), message_id)?.ok_or(ActionError::NotFound)
}

/// Marks one message read/unread: `UID STORE ±\Seen` then local mirror.
pub fn mark_read(
    t: &mut dyn MailTransport,
    store: &Store,
    message_id: i64,
    read: bool,
) -> Result<(), ActionError> {
    let addr = address_of(store, message_id)?;
    t.select(&addr.folder_name)?;
    if read {
        t.uid_store_add(&[addr.uid], &["\\Seen".into()])?;
    } else {
        t.uid_store_remove(&[addr.uid], &["\\Seen".into()])?;
    }
    queries::apply_server_flags(store.conn(), addr.folder_id, addr.uid, Some(read), None)?;
    Ok(())
}

/// Stars/unstars: `UID STORE ±\Flagged` then local mirror.
pub fn set_starred(
    t: &mut dyn MailTransport,
    store: &Store,
    message_id: i64,
    starred: bool,
) -> Result<(), ActionError> {
    let addr = address_of(store, message_id)?;
    t.select(&addr.folder_name)?;
    if starred {
        t.uid_store_add(&[addr.uid], &["\\Flagged".into()])?;
    } else {
        t.uid_store_remove(&[addr.uid], &["\\Flagged".into()])?;
    }
    queries::apply_server_flags(store.conn(), addr.folder_id, addr.uid, None, Some(starred))?;
    Ok(())
}

/// Archives: `UID MOVE` to the account's archive folder (\Archive, falling
/// back to \All — Gmail has no \Archive; All Mail is its archive target),
/// then drops the local row. The message stays on the server; v1 does not
/// sync archive folders.
pub fn archive(
    t: &mut dyn MailTransport,
    store: &Store,
    message_id: i64,
) -> Result<(), ActionError> {
    let addr = address_of(store, message_id)?;
    let folders = t.list_folders()?;
    let target = folders
        .iter()
        .find(|f| f.role == Some(crate::mail::transport::SpecialRole::Archive))
        .or_else(|| {
            folders
                .iter()
                .find(|f| f.role == Some(crate::mail::transport::SpecialRole::All))
        })
        .map(|f| f.name.clone())
        .ok_or(ActionError::NoArchiveFolder)?;
    t.select(&addr.folder_name)?;
    t.uid_move(&[addr.uid], &target)?;
    queries::delete_message_rows(store.conn(), &[message_id])?;
    Ok(())
}

/// Moves a message between buckets and teaches the classifier (FR-11 AC:
/// a later similar message lands in the corrected bucket, D-013 overrides).
pub fn set_importance(store: &Store, message_id: i64, important: bool) -> Result<(), ActionError> {
    let addr = address_of(store, message_id)?;
    let value = if important { "important" } else { "other" };
    store.conn().execute(
        "UPDATE messages SET importance = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![message_id, value, dao::now_ms()],
    )?;
    // Teach per-sender for future mail (the AC's "similar later message").
    if let Some(sender) = addr.sender_email.as_deref() {
        dao::set_classification_override(store.conn(), addr.account_id, sender, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::mock::{MockMailbox, MockMessage, MockTransport};
    use crate::mail::transport::{FolderInfo, SpecialRole};
    use crate::store::dao::{Addr, NewMessage};

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        dek: String,
        transport: MockTransport,
    }

    fn fixture(name: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let target = format!(
            "cracktivity.test.actions.{name}.{}.{}",
            std::process::id(),
            line!()
        );
        let store = Store::open(dir.path(), &target).unwrap();
        let account = dao::add_account(
            store.conn(),
            &dao::NewAccount {
                provider: "gmail",
                email: "me@example.com",
                display_name: None,
                auth_type: "app_password",
                credential_ref: "cracktivity.test",
                imap_host: None,
                imap_port: None,
            },
        )
        .unwrap();
        let folder = dao::upsert_folder(
            store.conn(),
            account,
            "INBOX",
            Some("inbox"),
            &dao::FolderUpsert {
                uidvalidity: 1,
                uidnext: 2,
                highestmodseq: None,
            },
        )
        .unwrap()
        .unwrap();
        // Server and local rows share UID 1 so actions address both in step.
        let mut inbox = MockMailbox {
            name: "INBOX".into(),
            uidvalidity: 1,
            uidnext: 1,
            ..Default::default()
        };
        inbox.push(MockMessage {
            uid: 0,
            flags: vec![],
            headers: [
                ("subject".to_string(), "hello".to_string()),
                ("from".to_string(), "a@b.c".to_string()),
            ]
            .into_iter()
            .collect(),
            thrid: None,
            body: Default::default(),
            internaldate_ms: 1,
            size: 10,
        });
        dao::insert_messages(
            store.conn(),
            account,
            folder.id,
            &[NewMessage {
                uid: 1,
                thrid: Some("t1".into()),
                subject: Some("hello".into()),
                from: Some(vec![Addr {
                    name: None,
                    email: "a@b.c".into(),
                }]),
                to: Some(vec![Addr {
                    name: None,
                    email: "me@example.com".into(),
                }]),
                importance: Some("other".into()),
                ..Default::default()
            }],
        )
        .unwrap();
        let mut transport = MockTransport::new(inbox);
        transport.folders = vec![
            FolderInfo {
                name: "INBOX".into(),
                role: Some(SpecialRole::Inbox),
                subscribed: true,
            },
            FolderInfo {
                name: "[Gmail]/All Mail".into(),
                role: Some(SpecialRole::All),
                subscribed: true,
            },
        ];
        Fixture {
            _dir: dir,
            store,
            dek: target,
            transport,
        }
    }

    fn dek_cleanup(target: &str) {
        crate::platform::keyring::init();
        let _ = crate::platform::keyring::delete_secret(target);
    }

    fn first_message_id(store: &Store) -> i64 {
        store
            .conn()
            .query_row("SELECT id FROM messages LIMIT 1", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn mark_read_and_star_round_trip() {
        let mut f = fixture("flags");
        let id = first_message_id(&f.store);

        mark_read(&mut f.transport, &f.store, id, true).unwrap();
        assert!(
            !thread_msg(&f.store, id).0,
            "read message not unread locally"
        );
        assert!(
            f.transport.active_mailbox().messages[&1]
                .flags
                .contains(&"\\Seen".to_string()),
            "server saw \\Seen"
        );

        set_starred(&mut f.transport, &f.store, id, true).unwrap();
        assert!(thread_msg(&f.store, id).1);
        assert!(
            f.transport.active_mailbox().messages[&1]
                .flags
                .contains(&"\\Flagged".to_string()),
            "server saw \\Flagged"
        );

        mark_read(&mut f.transport, &f.store, id, false).unwrap();
        assert!(thread_msg(&f.store, id).0);
        dek_cleanup(&f.dek);
    }

    #[test]
    fn archive_moves_on_server_and_drops_local_row() {
        let mut f = fixture("archive");
        // The mock's uid_move needs the destination mailbox to exist.
        f.transport.mailboxes.push(MockMailbox {
            name: "[Gmail]/All Mail".into(),
            uidvalidity: 1,
            uidnext: 1,
            ..Default::default()
        });
        let id = first_message_id(&f.store);

        archive(&mut f.transport, &f.store, id).unwrap();
        assert!(thread_msg_opt(&f.store, id).is_none(), "local row dropped");
        assert!(
            f.transport.mailboxes[0].messages.is_empty(),
            "source mailbox empty"
        );
        assert_eq!(
            f.transport.mailboxes[1].messages.len(),
            1,
            "message landed in All Mail"
        );
        dek_cleanup(&f.dek);
    }

    #[test]
    fn archive_without_any_archive_folder_is_actionable() {
        let mut f = fixture("noarchive");
        f.transport
            .folders
            .retain(|fo| fo.role != Some(SpecialRole::All));
        let id = first_message_id(&f.store);
        let err = archive(&mut f.transport, &f.store, id).unwrap_err();
        assert!(matches!(err, ActionError::NoArchiveFolder), "got {err:?}");
        assert!(
            thread_msg_opt(&f.store, id).is_some(),
            "row untouched on failure"
        );
        dek_cleanup(&f.dek);
    }

    #[test]
    fn importance_correction_teaches_the_sender() {
        let f = fixture("correct");
        let id = first_message_id(&f.store);

        set_importance(&f.store, id, true).unwrap();
        assert_eq!(thread_importance(&f.store, id), "important");

        let overrides = dao::classification_overrides(f.store.conn(), 1).unwrap();
        assert_eq!(
            overrides.get("a@b.c").map(String::as_str),
            Some("important")
        );
        dek_cleanup(&f.dek);
    }

    fn thread_msg(store: &Store, id: i64) -> (bool, bool) {
        store
            .conn()
            .query_row(
                "SELECT unread, starred FROM messages WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, i64>(1)? != 0)),
            )
            .unwrap()
    }

    fn thread_msg_opt(store: &Store, id: i64) -> Option<i64> {
        store
            .conn()
            .query_row("SELECT id FROM messages WHERE id = ?1", [id], |r| r.get(0))
            .ok()
    }

    fn thread_importance(store: &Store, id: i64) -> String {
        store
            .conn()
            .query_row("SELECT importance FROM messages WHERE id = ?1", [id], |r| {
                r.get::<_, Option<String>>(0)
            })
            .unwrap()
            .unwrap()
    }
}
