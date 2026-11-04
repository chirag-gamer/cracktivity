//! Read queries + local mutations for the UI layer (FR-8 offline hydration):
//! everything here answers from the local cache only — no transport, no
//! network. Shapes are `Serialize` so commands return them verbatim.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::dao::{self, Addr};
use super::StoreError;

#[derive(Debug, Clone, Serialize)]
pub struct AccountSummary {
    pub id: i64,
    pub provider: String,
    pub email: String,
    pub display_name: Option<String>,
    pub sync_state: String,
    pub enabled: bool,
    /// Unread messages across the account's inbox folders.
    pub unread: i64,
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<AccountSummary>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.id, a.provider, a.email, a.display_name, a.sync_state, a.enabled,
                (SELECT COUNT(*) FROM messages m
                 JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
                 WHERE m.account_id = a.id AND m.unread = 1)
         FROM accounts a ORDER BY a.ord, a.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountSummary {
                id: r.get(0)?,
                provider: r.get(1)?,
                email: r.get(2)?,
                display_name: r.get(3)?,
                sync_state: r.get(4)?,
                enabled: r.get(5)?,
                unread: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Which inbox the list shows. `Unified` = all accounts (FR-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Unified,
    Account(i64),
}

/// Importance filter on the list view (FR-11 split views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportanceFilter {
    All,
    Important,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadRow {
    pub thrid: String,
    pub account_id: i64,
    pub account_email: String,
    pub subject: String,
    pub snippet: String,
    pub from: Option<Addr>,
    pub last_date: i64,
    pub msg_count: i64,
    pub unread_count: i64,
    pub starred: bool,
    pub importance: Option<String>,
}

fn map_thread_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadRow> {
    let from_json: Option<String> = r.get(5)?;
    Ok(ThreadRow {
        thrid: r.get(0)?,
        account_id: r.get(1)?,
        account_email: r.get(2)?,
        subject: r.get(3)?,
        snippet: r.get(4)?,
        from: from_json
            .and_then(|j| serde_json::from_str::<Vec<Addr>>(&j).ok())
            .and_then(|mut v| {
                if v.is_empty() {
                    None
                } else {
                    Some(v.remove(0))
                }
            }),
        last_date: r.get(6)?,
        msg_count: r.get(7)?,
        unread_count: r.get(8)?,
        starred: r.get::<_, i64>(9)? != 0,
        importance: r.get(10)?,
    })
}

const THREAD_ROW_SELECT: &str = "m.thrid, m.account_id, a.email AS account_email,
       COALESCE(m.subject, ''), COALESCE(m.snippet, ''), m.from_json,
       COALESCE(m.date_recv, m.date_sent, 0) AS last_date,
       (SELECT COUNT(*) FROM messages x WHERE x.thrid = m.thrid) AS msg_count,
       (SELECT COALESCE(SUM(x.unread), 0) FROM messages x WHERE x.thrid = m.thrid) AS unread_count,
       (SELECT COALESCE(MAX(x.starred), 0) FROM messages x WHERE x.thrid = m.thrid) AS starred,
       m.importance";

const LATEST_OF_THREAD: &str = "COALESCE(m.date_recv, m.date_sent, 0) = (
      SELECT MAX(COALESCE(x.date_recv, x.date_sent, 0))
      FROM messages x JOIN folders xf ON xf.id = x.folder_id AND xf.role = 'inbox'
      WHERE x.thrid = m.thrid)";

/// Thread list for a scope, newest first. One row per thread; ties on the
/// per-thread latest timestamp are deduped here (map by thrid, keep first).
pub fn thread_list(
    conn: &Connection,
    scope: Scope,
    filter: ImportanceFilter,
    limit: i64,
    offset: i64,
) -> Result<Vec<ThreadRow>, StoreError> {
    let mut sql = format!(
        "SELECT {THREAD_ROW_SELECT}
         FROM messages m
         JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
         JOIN accounts a ON a.id = m.account_id
         WHERE m.thrid IS NOT NULL AND {LATEST_OF_THREAD}"
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Scope::Account(id) = scope {
        sql.push_str(" AND m.account_id = ?");
        args.push(Box::new(id));
    }
    if let ImportanceFilter::Important | ImportanceFilter::Other = filter {
        sql.push_str(" AND m.importance = ?");
        args.push(Box::new(match filter {
            ImportanceFilter::Important => "important",
            _ => "other",
        }));
    }
    sql.push_str(" ORDER BY last_date DESC LIMIT ? OFFSET ?");
    args.push(Box::new(limit));
    args.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
            map_thread_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    // Same-millisecond ties can yield two "latest" rows for one thread.
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|r| seen.insert(r.thrid.clone()))
        .collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageRow {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub subject: Option<String>,
    pub from: Vec<Addr>,
    pub to: Vec<Addr>,
    pub date: i64,
    pub unread: bool,
    pub starred: bool,
    pub snippet: String,
    pub has_body: bool,
    pub importance: Option<String>,
}

fn map_message_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRow> {
    let from: Option<String> = r.get(5)?;
    let to: Option<String> = r.get(6)?;
    Ok(MessageRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        folder_id: r.get(2)?,
        uid: r.get::<_, i64>(3)? as u32,
        subject: r.get(4)?,
        from: from
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default(),
        to: to
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default(),
        date: r.get(7)?,
        unread: r.get::<_, i64>(8)? != 0,
        starred: r.get::<_, i64>(9)? != 0,
        snippet: r.get(10)?,
        has_body: r.get::<_, i64>(11)? != 0,
        importance: r.get(12)?,
    })
}

/// All messages of a thread, oldest first (FR-16 reading order).
/// `__msg-<id>` (the frontend's fallback for pre-threading search rows with
/// a null thrid) resolves to that single message.
pub fn thread_messages(conn: &Connection, thrid: &str) -> Result<Vec<MessageRow>, StoreError> {
    if let Some(id) = thrid.strip_prefix("__msg-") {
        if let Ok(id) = id.parse::<i64>() {
            let mut stmt = conn.prepare_cached(
                "SELECT id, account_id, folder_id, uid, subject, from_json, to_json,
                        COALESCE(date_sent, date_recv, 0), unread, starred,
                        COALESCE(snippet, ''), body_text IS NOT NULL OR body_html IS NOT NULL,
                        importance
                 FROM messages WHERE id = ?1",
            )?;
            let row = stmt.query_row(params![id], map_message_row).optional()?;
            return Ok(row.into_iter().collect());
        }
    }
    let mut stmt = conn.prepare_cached(
        "SELECT id, account_id, folder_id, uid, subject, from_json, to_json,
                COALESCE(date_sent, date_recv, 0), unread, starred,
                COALESCE(snippet, ''), body_text IS NOT NULL OR body_html IS NOT NULL,
                importance
         FROM messages WHERE thrid = ?1 ORDER BY COALESCE(date_sent, date_recv, 0)",
    )?;
    let rows = stmt
        .query_map(params![thrid], map_message_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize)]
pub struct BodyRow {
    pub text: Option<String>,
    pub html: Option<String>,
    pub snippet: String,
}

/// Cached body for one message (sanitization happens in the command layer,
/// spec §3: raw in the DB, sanitized at render).
pub fn message_body(conn: &Connection, message_id: i64) -> Result<Option<BodyRow>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT body_text, body_html, COALESCE(snippet, '') FROM messages WHERE id = ?1",
            params![message_id],
            |r| {
                Ok(BodyRow {
                    text: r.get(0)?,
                    html: r.get(1)?,
                    snippet: r.get(2)?,
                })
            },
        )
        .optional()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchRow {
    pub message_id: i64,
    pub thrid: Option<String>,
    pub account_id: i64,
    pub subject: String,
    pub snippet: String,
    pub from: Option<Addr>,
    pub date: i64,
    pub unread: bool,
}

/// Quotes each user token as an FTS5 prefix phrase so arbitrary input can
/// never be parsed as FTS query syntax (quotes doubled per FTS5 escaping).
pub fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .take(8)
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Local FTS5 search over the cached mail (FR-9 v1 local-only path).
pub fn search(conn: &Connection, query: &str, limit: i64) -> Result<Vec<SearchRow>, StoreError> {
    let fts = fts_query(query);
    if fts.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare_cached(
        "SELECT m.id, m.thrid, m.account_id, COALESCE(m.subject, ''),
                COALESCE(m.snippet, ''), m.from_json,
                COALESCE(m.date_recv, m.date_sent, 0), m.unread
         FROM messages_fts f JOIN messages m ON m.id = f.rowid
         WHERE messages_fts MATCH ?1
         ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![fts, limit], |r| {
            let from_json: Option<String> = r.get(5)?;
            Ok(SearchRow {
                message_id: r.get(0)?,
                thrid: r.get(1)?,
                account_id: r.get(2)?,
                subject: r.get(3)?,
                snippet: r.get(4)?,
                from: from_json
                    .and_then(|j| serde_json::from_str::<Vec<Addr>>(&j).ok())
                    .and_then(|mut v| {
                        if v.is_empty() {
                            None
                        } else {
                            Some(v.remove(0))
                        }
                    }),
                date: r.get(6)?,
                unread: r.get::<_, i64>(7)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ------------------------------------------------------------ local mutations

pub fn set_message_read(
    conn: &Connection,
    message_id: i64,
    unread: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE messages SET unread = ?2, updated_at = ?3 WHERE id = ?1",
        params![message_id, unread as i64, dao::now_ms()],
    )?;
    Ok(())
}

pub fn set_message_starred(
    conn: &Connection,
    message_id: i64,
    starred: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE messages SET starred = ?2, updated_at = ?3 WHERE id = ?1",
        params![message_id, starred as i64, dao::now_ms()],
    )?;
    Ok(())
}

/// Locally applies a flag change coming back from the server (the action
/// layer updates the server first; this keeps the cache in step).
pub fn apply_server_flags(
    conn: &Connection,
    folder_id: i64,
    uid: u32,
    read: Option<bool>,
    starred: Option<bool>,
) -> Result<(), StoreError> {
    match (read, starred) {
        (None, None) => {}
        (Some(r), None) => {
            conn.execute(
                "UPDATE messages SET unread = ?3, updated_at = ?4
                 WHERE folder_id = ?1 AND uid = ?2",
                params![folder_id, uid, !r as i64, dao::now_ms()],
            )?;
        }
        (None, Some(s)) => {
            conn.execute(
                "UPDATE messages SET starred = ?3, updated_at = ?4
                 WHERE folder_id = ?1 AND uid = ?2",
                params![folder_id, uid, s as i64, dao::now_ms()],
            )?;
        }
        (Some(r), Some(s)) => {
            conn.execute(
                "UPDATE messages SET unread = ?3, starred = ?4, updated_at = ?5
                 WHERE folder_id = ?1 AND uid = ?2",
                params![folder_id, uid, !r as i64, s as i64, dao::now_ms()],
            )?;
        }
    }
    Ok(())
}

/// Removes rows after a successful server-side UID MOVE (archive): the
/// message lives on in the server's archive folder, which v1 does not sync.
pub fn delete_message_rows(conn: &Connection, ids: &[i64]) -> Result<usize, StoreError> {
    let mut total = 0;
    for chunk in ids.chunks(500) {
        let marks = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM messages WHERE id IN ({marks})");
        total += conn.execute(&sql, rusqlite::params_from_iter(chunk.iter()))?;
    }
    Ok(total)
}

/// What the action layer needs to address one message on the server.
#[derive(Debug, Clone)]
pub struct MessageAddress {
    pub account_id: i64,
    pub folder_id: i64,
    pub folder_name: String,
    pub uid: u32,
    pub sender_email: Option<String>,
}

pub fn message_address(
    conn: &Connection,
    message_id: i64,
) -> Result<Option<MessageAddress>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT m.account_id, m.folder_id, f.name, m.uid, m.from_json
             FROM messages m JOIN folders f ON f.id = m.folder_id
             WHERE m.id = ?1",
            params![message_id],
            |r| {
                let from_json: Option<String> = r.get(4)?;
                let sender = from_json
                    .and_then(|j| serde_json::from_str::<Vec<Addr>>(&j).ok())
                    .and_then(|v| v.into_iter().next())
                    .map(|a| a.email);
                Ok(MessageAddress {
                    account_id: r.get(0)?,
                    folder_id: r.get(1)?,
                    folder_name: r.get(2)?,
                    uid: r.get::<_, i64>(3)? as u32,
                    sender_email: sender,
                })
            },
        )
        .optional()?)
}

/// Total unread across all accounts' inboxes (tray/panel badge input, FR-31).
pub fn total_unread(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM messages m
         JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
         WHERE m.unread = 1",
        [],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{dao::NewMessage, Store};

    fn test_dek_target(name: &str) -> String {
        format!(
            "cracktivity.test.queries.{name}.{}.{}",
            std::process::id(),
            line!()
        )
    }

    fn cleanup(target: &str) {
        crate::platform::keyring::init();
        let _ = crate::platform::keyring::delete_secret(target);
    }

    fn seed(name: &str) -> (tempfile::TempDir, Store, i64, String) {
        let dir = tempfile::tempdir().unwrap();
        let target = test_dek_target(name);
        let store = Store::open(dir.path(), &target).unwrap();
        let account = dao::add_account(
            store.conn(),
            &dao::NewAccount {
                provider: "gmail",
                email: "me@example.com",
                display_name: None,
                auth_type: "app_password",
                credential_ref: "cracktivity.account.test",
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
                uidnext: 100,
                highestmodseq: None,
            },
        )
        .unwrap()
        .expect("folder state");
        let mk =
            |uid: u32, thrid: &str, subject: &str, from: &str, unread: bool, importance: &str| {
                NewMessage {
                    uid,
                    thrid: Some(thrid.into()),
                    subject: Some(subject.into()),
                    from: Some(vec![Addr {
                        name: Some("Sender".into()),
                        email: from.into(),
                    }]),
                    to: Some(vec![Addr {
                        name: None,
                        email: "me@example.com".into(),
                    }]),
                    date_recv: Some(1_700_000_000_000 + uid as i64 * 1000),
                    flags: if unread {
                        vec![]
                    } else {
                        vec!["\\Seen".into()]
                    },
                    importance: Some(importance.into()),
                    importance_reasons: vec!["test".into()],
                    ..Default::default()
                }
            };
        dao::insert_messages(
            store.conn(),
            account,
            folder.id,
            &[
                mk(1, "t-a", "Newsletter", "news@corp.com", true, "other"),
                mk(2, "t-a", "Re: Newsletter", "news@corp.com", false, "other"),
                mk(3, "t-b", "Lunch?", "friend@example.org", true, "important"),
                mk(4, "t-c", "Receipt", "shop@example.net", false, "other"),
            ],
        )
        .unwrap();
        // The tempdir+store outlive this fn; the caller cleans the DEK entry.
        (dir, store, account, target)
    }

    #[test]
    fn thread_list_groups_and_counts() {
        let (_dir, store, account, dek) = seed("threads");
        let rows = thread_list(store.conn(), Scope::Unified, ImportanceFilter::All, 50, 0).unwrap();
        assert_eq!(rows.len(), 3, "4 messages in 3 threads: {rows:?}");
        // Newest first: t-c (uid 4) before t-b before t-a.
        assert_eq!(rows[0].thrid, "t-c");
        assert_eq!(rows[2].thrid, "t-a");
        let t_a = &rows[2];
        assert_eq!(t_a.msg_count, 2);
        assert_eq!(t_a.unread_count, 1);
        assert_eq!(t_a.from.as_ref().unwrap().email, "news@corp.com");
        assert_eq!(t_a.account_id, account);

        let scoped = thread_list(
            store.conn(),
            Scope::Account(account),
            ImportanceFilter::All,
            50,
            0,
        )
        .unwrap();
        assert_eq!(scoped.len(), 3);

        let important = thread_list(
            store.conn(),
            Scope::Unified,
            ImportanceFilter::Important,
            50,
            0,
        )
        .unwrap();
        assert_eq!(important.len(), 1);
        assert_eq!(important[0].thrid, "t-b");
        cleanup(&dek);
    }

    #[test]
    fn thread_messages_and_body() {
        let (_dir, store, _account, dek) = seed("bodies");
        let msgs = thread_messages(store.conn(), "t-a").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].uid, 1, "oldest first");
        assert!(msgs[0].unread);
        assert!(!msgs[1].unread);
        assert!(!msgs[0].has_body);

        dao::set_body(
            store.conn(),
            msgs[0].id,
            Some("hello body"),
            None,
            "hello body",
        )
        .unwrap();
        let body = message_body(store.conn(), msgs[0].id).unwrap().unwrap();
        assert_eq!(body.text.as_deref(), Some("hello body"));
        assert!(thread_messages(store.conn(), "t-a").unwrap()[0].has_body);
        cleanup(&dek);
    }

    #[test]
    fn search_finds_bodies_and_quotes_input() {
        let (_dir, store, _account, dek) = seed("search");
        let msgs = thread_messages(store.conn(), "t-a").unwrap();
        dao::set_body(
            store.conn(),
            msgs[0].id,
            Some("quarterly report inside"),
            None,
            "report",
        )
        .unwrap();

        let hits = search(store.conn(), "report", 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, msgs[0].id);
        assert_eq!(hits[0].thrid.as_deref(), Some("t-a"));

        // FTS syntax from user input must be neutralized: bare "NEAR" is an
        // FTS5 operator, quoted phrase is not.
        assert_eq!(fts_query("a\"b NEAR ("), "\"a\"\"b\"* \"NEAR\"* \"(\"*");
        // Empty and whitespace-only queries return no rows, no error.
        assert!(search(store.conn(), "   ", 20).unwrap().is_empty());
        cleanup(&dek);
    }

    #[test]
    fn unread_and_mutations() {
        let (_dir, store, _account, dek) = seed("mutations");
        assert_eq!(total_unread(store.conn()).unwrap(), 2);

        let msgs = thread_messages(store.conn(), "t-b").unwrap();
        set_message_read(store.conn(), msgs[0].id, false).unwrap();
        assert_eq!(total_unread(store.conn()).unwrap(), 1);

        set_message_starred(store.conn(), msgs[0].id, true).unwrap();
        assert!(thread_messages(store.conn(), "t-b").unwrap()[0].starred);

        apply_server_flags(
            store.conn(),
            msgs[0].folder_id,
            msgs[0].uid,
            Some(true),
            None,
        )
        .unwrap();
        assert!(!thread_messages(store.conn(), "t-b").unwrap()[0].unread);

        let addr = message_address(store.conn(), msgs[0].id)
            .unwrap()
            .expect("address");
        assert_eq!(addr.folder_name, "INBOX");
        assert_eq!(addr.sender_email.as_deref(), Some("friend@example.org"));

        assert_eq!(delete_message_rows(store.conn(), &[msgs[0].id]).unwrap(), 1);
        assert!(thread_messages(store.conn(), "t-b").unwrap().is_empty());
        cleanup(&dek);
    }

    #[test]
    fn msg_fallback_resolves_single_row() {
        let (_dir, store, _account, dek) = seed("msgfallback");
        // A pre-threading row: thrid NULL (search rows fall back to __msg-<id>).
        let msgs = thread_messages(store.conn(), "t-b").unwrap();
        assert!(!msgs.is_empty());
        let id = msgs[0].id;
        store
            .conn()
            .execute("UPDATE messages SET thrid = NULL WHERE id = ?1", [id])
            .unwrap();

        let resolved = thread_messages(store.conn(), &format!("__msg-{id}")).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, id);
        assert_eq!(resolved[0].subject.as_deref(), Some("Lunch?"));
        assert!(thread_messages(store.conn(), "__msg-notanumber")
            .unwrap()
            .is_empty());
        cleanup(&dek);
    }

    #[test]
    fn accounts_carry_unread() {
        let (_dir, store, _account, dek) = seed("accounts");
        let accounts = list_accounts(store.conn()).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].unread, 2);
        assert_eq!(accounts[0].provider, "gmail");
        cleanup(&dek);
    }
}
