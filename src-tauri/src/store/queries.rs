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

// ------------------------------------------------------------------ opened latch

/// Latches the first reader open of one message and credits the sender's
/// engagement counter (FR-14 ranking input). Returns `true` when this call
/// performed the first open — later opens are no-ops.
pub fn mark_opened(conn: &Connection, message_id: i64) -> Result<bool, StoreError> {
    let now = dao::now_ms();
    let changed = conn.execute(
        "UPDATE messages SET opened_at = ?2, updated_at = ?2
         WHERE id = ?1 AND opened_at IS NULL",
        params![message_id, now],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO sender_stats(account_id, sender_email, opened, created_at, updated_at)
         SELECT account_id, sender, 1, ?2, ?2 FROM (
           SELECT (SELECT account_id FROM messages WHERE id = ?1) AS account_id,
                  lower(json_extract((SELECT from_json FROM messages WHERE id = ?1),
                                     '$[0].email')) AS sender
         ) WHERE sender IS NOT NULL
         ON CONFLICT(account_id, sender_email) DO UPDATE SET
           opened = opened + 1, updated_at = excluded.updated_at",
        params![message_id, now],
    )?;
    Ok(true)
}

// --------------------------------------------------------------------- follow-ups

/// One "Waiting for Reply" row (FR-13): the thread's latest message is the
/// user's own (sent folder) and its age exceeds the follow-up threshold.
#[derive(Debug, Clone, Serialize)]
pub struct FollowupRow {
    pub thrid: String,
    pub account_id: i64,
    pub account_email: String,
    pub subject: String,
    pub snippet: String,
    /// Recipient of my sent message (who owes the reply).
    pub to: Option<Addr>,
    pub sent_date: i64,
    pub days_waiting: i64,
}

/// Threads awaiting a reply: the newest message of the thread lives in a
/// sent-role folder and is at least `days` old. `now` is injected so tests
/// wind the clock (V-4.2). Oldest-waiting first.
pub fn waiting_for_reply(
    conn: &Connection,
    scope: Scope,
    now: i64,
    days: i64,
    limit: i64,
) -> Result<Vec<FollowupRow>, StoreError> {
    let cutoff = now - days * 86_400_000;
    let mut sql = String::from(
        "SELECT m.thrid, m.account_id, a.email,
                COALESCE(m.subject, ''), COALESCE(m.snippet, ''), m.to_json,
                COALESCE(m.date_sent, m.date_recv, 0) AS sent_date
         FROM messages m
         JOIN folders f ON f.id = m.folder_id AND f.role = 'sent'
         JOIN accounts a ON a.id = m.account_id
         WHERE m.thrid IS NOT NULL
           AND COALESCE(m.date_sent, m.date_recv, 0) = (
             SELECT MAX(COALESCE(x.date_sent, x.date_recv, 0))
             FROM messages x WHERE x.thrid = m.thrid)
           AND sent_date <= ?1
           AND sent_date > 0",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(cutoff)];
    if let Scope::Account(id) = scope {
        sql.push_str(" AND m.account_id = ?");
        args.push(Box::new(id));
    }
    sql.push_str(" ORDER BY sent_date ASC LIMIT ?");
    args.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
            |r| {
                let to_json: Option<String> = r.get(5)?;
                let sent_date: i64 = r.get(6)?;
                Ok(FollowupRow {
                    thrid: r.get(0)?,
                    account_id: r.get(1)?,
                    account_email: r.get(2)?,
                    subject: r.get(3)?,
                    snippet: r.get(4)?,
                    to: to_json
                        .and_then(|j| serde_json::from_str::<Vec<Addr>>(&j).ok())
                        .and_then(|mut v| {
                            if v.is_empty() {
                                None
                            } else {
                                Some(v.remove(0))
                            }
                        }),
                    sent_date,
                    days_waiting: (now - sent_date).max(0) / 86_400_000,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    // Same-millisecond ties can yield duplicate rows for one thread.
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|r| seen.insert(r.thrid.clone()))
        .collect())
}

// ------------------------------------------------------------- subscription cleanup

/// One sender row of the cleanup view (FR-14): messages carrying
/// List-Unsubscribe, ranked never-opened first.
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionRow {
    pub sender_email: String,
    pub sender_name: Option<String>,
    pub account_id: i64,
    pub received: i64,
    pub opened: i64,
    pub last_date: i64,
    /// Unsubscribe headers of the sender's newest message.
    pub list_unsubscribe: Option<String>,
    pub list_unsub_post: Option<String>,
}

/// Senders with unsubscribe-capable mail in the inbox folders, ranked:
/// lowest open rate first (never-opened first), then most mail first.
pub fn subscription_senders(conn: &Connection) -> Result<Vec<SubscriptionRow>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT sender, COUNT(*),
                SUM(CASE WHEN opened_at IS NOT NULL THEN 1 ELSE 0 END),
                MAX(COALESCE(date_recv, date_sent, 0)),
                (SELECT m2.list_unsubscribe FROM messages m2
                 WHERE m2.account_id = m.account_id AND m2.from_json LIKE '%' || sender || '%'
                   AND m2.list_unsubscribe IS NOT NULL
                 ORDER BY COALESCE(m2.date_recv, m2.date_sent, 0) DESC LIMIT 1),
                (SELECT m2.list_unsub_post FROM messages m2
                 WHERE m2.account_id = m.account_id AND m2.from_json LIKE '%' || sender || '%'
                   AND m2.list_unsub_post IS NOT NULL
                 ORDER BY COALESCE(m2.date_recv, m2.date_sent, 0) DESC LIMIT 1),
                m.account_id, (SELECT m3.from_json FROM messages m3
                 WHERE m3.account_id = m.account_id AND m3.from_json LIKE '%' || sender || '%'
                 ORDER BY COALESCE(m3.date_recv, m3.date_sent, 0) DESC LIMIT 1)
         FROM (
           SELECT m.account_id, lower(json_extract(m.from_json, '$[0].email')) AS sender,
                  m.opened_at, m.date_recv, m.date_sent
           FROM messages m
           JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
           WHERE m.list_unsubscribe IS NOT NULL AND sender IS NOT NULL
         ) m
         GROUP BY m.account_id, sender
         ORDER BY (SUM(CASE WHEN opened_at IS NOT NULL THEN 1 ELSE 0 END) + 0.0) / COUNT(*) ASC,
                  COUNT(*) DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let from_json: Option<String> = r.get(7)?;
            let name = from_json
                .and_then(|j| serde_json::from_str::<Vec<Addr>>(&j).ok())
                .and_then(|v| v.into_iter().next())
                .and_then(|a| a.name);
            Ok(SubscriptionRow {
                sender_email: r.get(0)?,
                sender_name: name,
                account_id: r.get(6)?,
                received: r.get(1)?,
                opened: r.get::<_, i64>(2)?.max(0),
                last_date: r.get(3)?,
                list_unsubscribe: r.get(4)?,
                list_unsub_post: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// All inbox message ids from one sender — the bulk-archive target set (FR-14).
pub fn sender_message_ids(conn: &Connection, sender_email: &str) -> Result<Vec<i64>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT m.id FROM messages m
         JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
         WHERE lower(json_extract(m.from_json, '$[0].email')) = lower(?1)",
    )?;
    let rows = stmt
        .query_map(params![sender_email], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// -------------------------------------------------------------- image allowlist

/// The per-sender remote-image decision (FR-15/19): `None` = undecided (ask
/// per message), `Some(bool)` = allowlist answer.
pub fn sender_show_images(conn: &Connection, message_id: i64) -> Result<Option<bool>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT ss.show_images FROM sender_stats ss
             JOIN messages m ON m.id = ?1
               AND ss.account_id = m.account_id
               AND ss.sender_email = lower(json_extract(m.from_json, '$[0].email'))",
            params![message_id],
            |r| r.get::<_, Option<i64>>(0).map(|v| v.map(|x| x != 0)),
        )
        .optional()?
        .flatten())
}

/// Persists "Always show images from this sender" (or the reversal).
pub fn set_sender_show_images(
    conn: &Connection,
    account_id: i64,
    sender_email: &str,
    allow: bool,
) -> Result<(), StoreError> {
    let now = dao::now_ms();
    conn.execute(
        "INSERT INTO sender_stats(account_id, sender_email, show_images, created_at, updated_at)
         VALUES (?1, lower(?2), ?3, ?4, ?4)
         ON CONFLICT(account_id, sender_email) DO UPDATE SET
           show_images = excluded.show_images, updated_at = excluded.updated_at",
        params![account_id, sender_email, allow as i64, now],
    )?;
    Ok(())
}

/// Caches the tracker count computed at render time (FR-15 badge source).
pub fn set_tracking_count(
    conn: &Connection,
    message_id: i64,
    tracking_count: usize,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE messages SET tracking_count = ?2, updated_at = ?3 WHERE id = ?1",
        params![message_id, tracking_count as i64, dao::now_ms()],
    )?;
    Ok(())
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
                smtp_host: None,
                smtp_port: None,
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

    // ---------------------------------------------------- Phase 4: store layer

    fn sent_folder(store: &Store, account: i64) -> i64 {
        dao::upsert_folder(
            store.conn(),
            account,
            "Sent",
            Some("sent"),
            &dao::FolderUpsert {
                uidvalidity: 1,
                uidnext: 1,
                highestmodseq: None,
            },
        )
        .unwrap()
        .unwrap()
        .id
    }

    #[allow(clippy::too_many_arguments)]
    fn insert(
        store: &Store,
        account: i64,
        folder_id: i64,
        uid: u32,
        thrid: Option<&str>,
        from: &str,
        to: &str,
        date: i64,
        unsub: Option<&str>,
    ) -> i64 {
        let mut m = NewMessage {
            uid,
            thrid: thrid.map(Into::into),
            subject: Some(format!("subject {uid}")),
            from: Some(vec![Addr {
                name: Some("Sender".into()),
                email: from.into(),
            }]),
            to: Some(vec![Addr {
                name: None,
                email: to.into(),
            }]),
            date_sent: Some(date),
            date_recv: Some(date),
            importance: Some("important".into()),
            importance_reasons: vec!["test".into()],
            list_unsubscribe: unsub.map(Into::into),
            ..Default::default()
        };
        if from == "me@example.com" {
            m.flags.push("\\Seen".into());
        }
        dao::insert_messages(store.conn(), account, folder_id, &[m]).unwrap();
        store
            .conn()
            .query_row(
                "SELECT id FROM messages WHERE folder_id = ?1 AND uid = ?2",
                [folder_id, uid as i64],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// FR-14 ranking input: the first open latches and credits the sender
    /// exactly once; later opens are no-ops.
    #[test]
    fn opened_latch_credits_sender_once() {
        let (_dir, store, account, dek) = seed("opened");
        let folder = store
            .conn()
            .query_row("SELECT id FROM folders LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let day = 86_400_000i64;
        let id = insert(
            &store,
            account,
            folder,
            50,
            Some("t-open"),
            "news@bulk.example",
            "me@example.com",
            1_700_000_000_000,
            Some("<https://x/u>"),
        );

        assert!(mark_opened(store.conn(), id).unwrap(), "first open counts");
        assert!(
            !mark_opened(store.conn(), id).unwrap(),
            "re-open is a no-op"
        );

        let (opened_at, opened): (i64, i64) = store
            .conn()
            .query_row(
                "SELECT (SELECT opened_at FROM messages WHERE id = ?1) IS NOT NULL,
                        (SELECT COALESCE(MAX(opened), 0) FROM sender_stats
                         WHERE account_id = ?2 AND sender_email = 'news@bulk.example')",
                [id, account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(opened_at, 1);
        assert_eq!(opened, 1);
        let _ = day;
        cleanup(&dek);
    }

    /// V-4.2 (plan): the follow-up clock is injected — winding `now` forward
    /// past the threshold surfaces the thread; a later reply clears it.
    #[test]
    fn followups_respect_injected_clock_and_clear_on_reply() {
        let (_dir, store, account, dek) = seed("followups");
        let inbox = store
            .conn()
            .query_row("SELECT id FROM folders WHERE role = 'inbox'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let sent = sent_folder(&store, account);
        let day = 86_400_000i64;
        let sent_at = 1_700_000_000_000;

        // I sent a message 5 days before the probe instant.
        insert(
            &store,
            account,
            sent,
            60,
            Some("t-wait"),
            "me@example.com",
            "them@example.org",
            sent_at,
            None,
        );

        // One day after sending: nothing is waiting (threshold 3).
        assert!(
            waiting_for_reply(store.conn(), Scope::Unified, sent_at + day, 3, 50)
                .unwrap()
                .is_empty()
        );
        // Five days after sending: the thread is waiting.
        let rows =
            waiting_for_reply(store.conn(), Scope::Unified, sent_at + 5 * day, 3, 50).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thrid, "t-wait");
        assert_eq!(rows[0].days_waiting, 5);
        assert_eq!(rows[0].to.as_ref().unwrap().email, "them@example.org");

        // A newer incoming reply clears the wait (latest message is no longer mine).
        insert(
            &store,
            account,
            inbox,
            61,
            Some("t-wait"),
            "them@example.org",
            "me@example.com",
            sent_at + 6 * day,
            None,
        );
        assert!(
            waiting_for_reply(store.conn(), Scope::Unified, sent_at + 8 * day, 3, 50)
                .unwrap()
                .is_empty()
        );
        cleanup(&dek);
    }

    /// FR-14: senders rank never-opened first; per-sender archive targets
    /// resolve to inbox rows only.
    #[test]
    fn subscription_senders_rank_and_archive_targets_resolve() {
        let (_dir, store, account, dek) = seed("cleanup");
        let folder = store
            .conn()
            .query_row("SELECT id FROM folders WHERE role = 'inbox'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let day = 86_400_000i64;
        let t0 = 1_700_000_000_000;
        // Sender A: two messages, never opened. Sender B: one, opened.
        insert(
            &store,
            account,
            folder,
            70,
            Some("t-a1"),
            "a@bulk.example",
            "me@example.com",
            t0,
            Some("<https://a.example/u>"),
        );
        insert(
            &store,
            account,
            folder,
            71,
            Some("t-a2"),
            "a@bulk.example",
            "me@example.com",
            t0 + day,
            Some("<https://a.example/u>"),
        );
        let b_id = insert(
            &store,
            account,
            folder,
            72,
            Some("t-b1"),
            "b@bulk.example",
            "me@example.com",
            t0 + 2 * day,
            Some("<https://b.example/u>"),
        );
        mark_opened(store.conn(), b_id).unwrap();
        // Not subscribed (no List-Unsubscribe) — must never appear.
        insert(
            &store,
            account,
            folder,
            73,
            Some("t-c1"),
            "c@plain.example",
            "me@example.com",
            t0,
            None,
        );

        let rows = subscription_senders(store.conn()).unwrap();
        assert_eq!(rows.len(), 2, "only unsub-capable senders: {rows:?}");
        assert_eq!(rows[0].sender_email, "a@bulk.example", "never-opened first");
        assert_eq!(rows[0].received, 2);
        assert_eq!(rows[0].opened, 0);
        assert_eq!(rows[1].sender_email, "b@bulk.example");
        assert_eq!(rows[1].opened, 1);
        assert_eq!(rows[1].list_unsub_post, None);
        assert_eq!(
            rows[1].list_unsubscribe.as_deref(),
            Some("<https://b.example/u>")
        );

        let ids = sender_message_ids(store.conn(), "a@bulk.example").unwrap();
        assert_eq!(ids.len(), 2);
        cleanup(&dek);
    }

    /// Drafts round-trip through the outbox lifecycle (FR-17/18 seed).
    #[test]
    fn draft_lifecycle() {
        let (_dir, store, account, dek) = seed("drafts");
        let id = dao::add_draft(
            store.conn(),
            &dao::NewDraft {
                account_id: account,
                thread_id: None,
                to: &[Addr {
                    name: None,
                    email: "x@y.z".into(),
                }],
                cc: &[],
                subject: "Hi",
                body_text: "Body",
                in_reply_to: Some("<m@y.z>"),
            },
        )
        .unwrap();
        dao::update_draft(
            store.conn(),
            id,
            &[Addr {
                name: None,
                email: "x@y.z".into(),
            }],
            &[],
            "Hi!",
            "Body!",
        )
        .unwrap();
        dao::set_draft_status(store.conn(), id, "queued", None).unwrap();
        // A queued draft is no longer editable.
        assert!(dao::update_draft(store.conn(), id, &[], &[], "x", "y").is_err());
        let pending = dao::list_drafts(store.conn(), true).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, "queued");
        dao::set_draft_status(store.conn(), id, "failed", Some("smtp refused")).unwrap();
        assert_eq!(
            dao::draft_by_id(store.conn(), id)
                .unwrap()
                .unwrap()
                .error
                .as_deref(),
            Some("smtp refused")
        );
        dao::delete_draft(store.conn(), id).unwrap();
        assert!(dao::draft_by_id(store.conn(), id).unwrap().is_none());
        cleanup(&dek);
    }
}
