//! Data access for the sync engine (spec §3/§4). Deliberately thin: SQL +
//! row mapping only; batching/resumability policy lives in `mail::sync`.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::StoreError;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// RFC 5322 address as stored in the `*_json` columns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Addr {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub email: String,
}

/// One fetched message header-set ready to be persisted (spec §4 backfill).
#[derive(Debug, Clone, Default)]
pub struct NewMessage {
    pub uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_raw: Option<String>,
    pub subject: Option<String>,
    pub from: Option<Vec<Addr>>,
    pub to: Option<Vec<Addr>>,
    pub cc: Option<Vec<Addr>>,
    pub date_sent: Option<i64>,
    pub date_recv: Option<i64>,
    pub size: Option<i64>,
    pub flags: Vec<String>,
    pub list_unsubscribe: Option<String>,
    pub list_unsub_post: Option<String>,
    pub thrid: Option<String>,
    /// Classification verdict (D-013); `None` leaves the row unclassified.
    pub importance: Option<String>,
    pub importance_reasons: Vec<String>,
}

impl NewMessage {
    pub fn unread(&self) -> bool {
        !self.flags.iter().any(|f| f == "\\Seen")
    }
}

pub struct FolderState {
    pub id: i64,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highest_uid: u32,
}

pub fn folder_state_by_name(
    conn: &Connection,
    account_id: i64,
    name: &str,
) -> Result<Option<FolderState>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT id, uidvalidity, uidnext, highest_uid FROM folders
             WHERE account_id = ?1 AND name = ?2",
            params![account_id, name],
            |r| {
                Ok(FolderState {
                    id: r.get(0)?,
                    uidvalidity: r.get(1)?,
                    uidnext: r.get(2)?,
                    highest_uid: r.get(3)?,
                })
            },
        )
        .optional()?)
}

pub fn set_folder_uidnext(
    conn: &Connection,
    folder_id: i64,
    uidnext: u32,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE folders SET uidnext = ?2, updated_at = ?3 WHERE id = ?1",
        params![folder_id, uidnext, now_ms()],
    )?;
    Ok(())
}

/// Backfill cursor (spec §4.3 kill-safety): during backfill this holds the
/// lowest contiguous covered UID; `1` means backfill complete.
pub fn set_folder_highest_uid(
    conn: &Connection,
    folder_id: i64,
    highest: u32,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE folders SET highest_uid = ?2, updated_at = ?3 WHERE id = ?1",
        params![folder_id, highest, now_ms()],
    )?;
    Ok(())
}

pub fn set_account_sync_state(
    conn: &Connection,
    account_id: i64,
    sync_state: &str,
    full_sync_at: Option<i64>,
) -> Result<(), StoreError> {
    match full_sync_at {
        Some(ts) => {
            conn.execute(
                "UPDATE accounts SET sync_state = ?2, last_full_sync_at = ?3, updated_at = ?3
                 WHERE id = ?1",
                params![account_id, sync_state, ts],
            )?;
        }
        None => {
            conn.execute(
                "UPDATE accounts SET sync_state = ?2, updated_at = ?3 WHERE id = ?1",
                params![account_id, sync_state, now_ms()],
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- threading

/// Header fields JWZ needs, for every message of the account.
pub fn thread_inputs(
    conn: &Connection,
    account_id: i64,
) -> Result<Vec<crate::mail::thread::ThreadInput>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, message_id, in_reply_to, references_raw, subject
         FROM messages WHERE account_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![account_id], |r| {
            Ok(crate::mail::thread::ThreadInput {
                id: r.get(0)?,
                message_id: r.get(1)?,
                in_reply_to: r.get(2)?,
                references_raw: r.get(3)?,
                subject: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Writes JWZ assignments back: messages.thrid + threads/thread_msgs rows.
pub fn apply_threads(
    conn: &Connection,
    account_id: i64,
    assignments: &[crate::mail::thread::ThreadAssignment],
) -> Result<(), StoreError> {
    let now = now_ms();
    let tx = conn.unchecked_transaction()?;
    {
        let mut upd_thrid =
            tx.prepare_cached("UPDATE messages SET thrid = ?2, updated_at = ?3 WHERE id = ?1")?;
        let mut find_thread = tx.prepare_cached(
            "SELECT t.id FROM threads t
             JOIN thread_msgs tm ON tm.thread_id = t.id
             JOIN messages m ON m.id = tm.message_id
             WHERE m.account_id = ?1 AND m.thrid = ?2 LIMIT 1",
        )?;
        let mut insert_thread = tx.prepare_cached(
            "INSERT INTO threads(account_id, subject_norm, participants_json, last_activity,
                                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )?;
        let mut link_tm = tx.prepare_cached(
            "INSERT OR IGNORE INTO thread_msgs(thread_id, message_id) VALUES (?1, ?2)",
        )?;
        let mut upd_thread = tx.prepare_cached(
            "UPDATE threads SET participants_json = ?2, last_activity = ?3, updated_at = ?3
             WHERE id = ?1",
        )?;

        for a in assignments {
            upd_thrid.execute(params![a.id, a.thrid, now])?;
        }
        // Group by final thrid and refresh each thread's denormalized fields.
        let mut by_thrid: std::collections::HashMap<
            &str,
            Vec<&crate::mail::thread::ThreadAssignment>,
        > = std::collections::HashMap::new();
        for a in assignments {
            by_thrid.entry(a.thrid.as_str()).or_default().push(a);
        }
        for (thrid, group) in &by_thrid {
            let ids: Vec<i64> = group.iter().map(|a| a.id).collect();
            // ids are internal row ids — safe to inline as integers.
            let id_list = ids
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT COALESCE(group_concat(from_json, ','), '[]'),
                        COALESCE(MAX(COALESCE(date_recv, date_sent)), 0)
                 FROM messages WHERE account_id = {account_id} AND id IN ({id_list})"
            );
            let (participants, last_activity): (String, i64) =
                tx.query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let subject_norm = group
                .iter()
                .find_map(|a| a.subject_norm.clone())
                .unwrap_or_default();

            let existing: Option<i64> = find_thread
                .query_row(params![account_id, thrid], |r| r.get(0))
                .optional()?;
            let thread_id = match existing {
                Some(id) => {
                    upd_thread.execute(params![id, participants, last_activity])?;
                    id
                }
                None => {
                    insert_thread.execute(params![
                        account_id,
                        subject_norm,
                        participants,
                        last_activity,
                        now
                    ])?;
                    tx.last_insert_rowid()
                }
            };
            for id in ids {
                link_tm.execute(params![thread_id, id])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------- accounts

/// Fields for a new accounts row (spec §3).
pub struct NewAccount<'a> {
    pub provider: &'a str,
    pub email: &'a str,
    pub display_name: Option<&'a str>,
    pub auth_type: &'a str,
    pub credential_ref: &'a str,
    pub imap_host: Option<&'a str>,
    pub imap_port: Option<u16>,
}

pub fn add_account(conn: &Connection, acc: &NewAccount<'_>) -> Result<i64, StoreError> {
    let NewAccount {
        provider,
        email,
        display_name,
        auth_type,
        credential_ref,
        imap_host,
        imap_port,
    } = acc;
    let now = now_ms();
    conn.execute(
        "INSERT INTO accounts(provider, email, display_name, auth_type, credential_ref,
                              imap_host, imap_port, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            provider,
            email,
            display_name,
            auth_type,
            credential_ref,
            imap_host,
            imap_port,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

// ----------------------------------------------------------------- folders

pub struct FolderUpsert {
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highestmodseq: Option<String>,
}

/// Creates or updates folder metadata. Returns `None` when UIDVALIDITY
/// changed — the caller must re-baseline: UID-keyed rows were dropped and
/// `highest_uid` reset (spec §4.2, FR-8 no-data-loss rule).
pub fn upsert_folder(
    conn: &Connection,
    account_id: i64,
    name: &str,
    role: Option<&str>,
    st: &FolderUpsert,
) -> Result<Option<FolderState>, StoreError> {
    let now = now_ms();
    let existing: Option<(i64, u32, u32)> = conn
        .query_row(
            "SELECT id, uidvalidity, highest_uid FROM folders
             WHERE account_id = ?1 AND name = ?2",
            params![account_id, name],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;

    match existing {
        Some((id, old_uv, old_highest)) if old_uv == st.uidvalidity => {
            conn.execute(
                "UPDATE folders SET role = COALESCE(?2, role), uidnext = ?3,
                        highestmodseq = COALESCE(?4, highestmodseq), updated_at = ?5
                 WHERE id = ?1",
                params![id, role, st.uidnext, st.highestmodseq, now],
            )?;
            Ok(Some(FolderState {
                id,
                uidvalidity: old_uv,
                uidnext: st.uidnext,
                highest_uid: old_highest,
            }))
        }
        Some((id, _old_uv, _)) => {
            conn.execute("DELETE FROM messages WHERE folder_id = ?1", params![id])?;
            conn.execute(
                "UPDATE folders SET role = COALESCE(?2, role), uidvalidity = ?3, uidnext = ?4,
                        highestmodseq = COALESCE(?5, highestmodseq), highest_uid = 0,
                        updated_at = ?6
                 WHERE id = ?1",
                params![id, role, st.uidvalidity, st.uidnext, st.highestmodseq, now],
            )?;
            Ok(None)
        }
        None => {
            conn.execute(
                "INSERT INTO folders(account_id, name, role, uidvalidity, uidnext,
                                     highestmodseq, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    account_id,
                    name,
                    role,
                    st.uidvalidity,
                    st.uidnext,
                    st.highestmodseq,
                    now
                ],
            )?;
            Ok(Some(FolderState {
                id: conn.last_insert_rowid(),
                uidvalidity: st.uidvalidity,
                uidnext: st.uidnext,
                highest_uid: 0,
            }))
        }
    }
}

// ---------------------------------------------------------------- messages

/// Inserts a batch in one transaction (spec §4.3: writes of 500). Idempotent
/// per (account_id, folder_id, uid) so a killed mid-batch sync can replay.
pub fn insert_messages(
    conn: &Connection,
    account_id: i64,
    folder_id: i64,
    batch: &[NewMessage],
) -> Result<usize, StoreError> {
    let now = now_ms();
    let mut inserted = 0usize;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO messages(account_id, folder_id, uid, message_id, in_reply_to,
                  references_raw, subject, from_json, to_json, cc_json, date_sent, date_recv,
                  size, flags, list_unsubscribe, list_unsub_post, thrid, unread,
                  importance, importance_reasons_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?21)
             ON CONFLICT(account_id, folder_id, uid) DO UPDATE SET
               message_id = excluded.message_id,
               in_reply_to = excluded.in_reply_to,
               references_raw = excluded.references_raw,
               subject = excluded.subject,
               from_json = excluded.from_json,
               to_json = excluded.to_json,
               cc_json = excluded.cc_json,
               date_sent = excluded.date_sent,
               date_recv = excluded.date_recv,
               size = excluded.size,
               flags = excluded.flags,
               list_unsubscribe = excluded.list_unsubscribe,
               list_unsub_post = excluded.list_unsub_post,
               thrid = excluded.thrid,
               unread = excluded.unread,
               importance = COALESCE(excluded.importance, messages.importance),
               importance_reasons_json = COALESCE(excluded.importance_reasons_json, messages.importance_reasons_json),
               updated_at = excluded.updated_at",
        )?;
        for m in batch {
            let from = m
                .from
                .as_ref()
                .map(|a| serde_json::to_string(a).unwrap_or_default());
            let to =
                m.to.as_ref()
                    .map(|a| serde_json::to_string(a).unwrap_or_default());
            let cc =
                m.cc.as_ref()
                    .map(|a| serde_json::to_string(a).unwrap_or_default());
            let flags = serde_json::to_string(&m.flags).unwrap_or_else(|_| "[]".into());
            let reasons =
                serde_json::to_string(&m.importance_reasons).unwrap_or_else(|_| "[]".into());
            let n = stmt.execute(params![
                account_id,
                folder_id,
                m.uid,
                m.message_id,
                m.in_reply_to,
                m.references_raw,
                m.subject,
                from,
                to,
                cc,
                m.date_sent,
                m.date_recv,
                m.size,
                flags,
                m.list_unsubscribe,
                m.list_unsub_post,
                m.thrid,
                m.unread() as i64,
                m.importance,
                reasons,
                now,
            ])?;
            // execute returns 1 for insert, 1 for the update branch too; count
            // real inserts via changes() delta is unnecessary — report touched.
            let _ = n;
            inserted += 1;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

/// Sets flags for one message (flag diff reconciliation, spec §4.4).
pub fn update_flags(
    conn: &Connection,
    folder_id: i64,
    uid: u32,
    flags: &[String],
) -> Result<(), StoreError> {
    let json = serde_json::to_string(flags)?;
    conn.execute(
        "UPDATE messages SET flags = ?3, unread = ?4, updated_at = ?5
         WHERE folder_id = ?1 AND uid = ?2",
        params![
            folder_id,
            uid,
            json,
            !flags.iter().any(|f| f == "\\Seen") as i64,
            now_ms()
        ],
    )?;
    Ok(())
}

/// Removes expunged UIDs for a folder (reconciliation without QRESYNC).
pub fn delete_uids(conn: &Connection, folder_id: i64, uids: &[u32]) -> Result<usize, StoreError> {
    let mut total = 0;
    for chunk in uids.chunks(500) {
        let marks = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql =
            format!("DELETE FROM messages WHERE folder_id = {folder_id} AND uid IN ({marks})");
        let mut stmt = conn.prepare_cached(&sql)?;
        total += stmt.execute(rusqlite::params_from_iter(chunk.iter()))?;
    }
    Ok(total)
}

pub fn folder_uids(conn: &Connection, folder_id: i64) -> Result<Vec<u32>, StoreError> {
    let mut stmt = conn.prepare_cached("SELECT uid FROM messages WHERE folder_id = ?1")?;
    let rows = stmt
        .query_map(params![folder_id], |r| r.get(0))?
        .collect::<Result<Vec<u32>, _>>()?;
    Ok(rows)
}

pub fn message_flags(
    conn: &Connection,
    folder_id: i64,
    uids: &[u32],
) -> Result<Vec<(u32, Vec<String>)>, StoreError> {
    let mut out = Vec::new();
    for chunk in uids.chunks(500) {
        let marks = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT uid, flags FROM messages WHERE uid IN ({marks}) AND folder_id = ?{}",
            chunk.len() + 1
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let int_values: Vec<i64> = chunk
            .iter()
            .map(|u| *u as i64)
            .chain(std::iter::once(folder_id))
            .collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(int_values), |r| {
            let uid: u32 = r.get(0)?;
            let flags: String = r.get(1)?;
            Ok((uid, serde_json::from_str(&flags).unwrap_or_default()))
        })?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

/// Stores a fetched body (lazy body fetch, spec §4.5).
pub fn set_body(
    conn: &Connection,
    message_id: i64,
    body_text: Option<&str>,
    body_html: Option<&str>,
    snippet: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE messages SET body_text = ?2, body_html = ?3, snippet = ?4, updated_at = ?5
         WHERE id = ?1",
        params![message_id, body_text, body_html, snippet, now_ms()],
    )?;
    Ok(())
}

pub fn message_id_by_uid(
    conn: &Connection,
    folder_id: i64,
    uid: u32,
) -> Result<Option<i64>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT id FROM messages WHERE folder_id = ?1 AND uid = ?2",
            params![folder_id, uid],
            |r| r.get(0),
        )
        .optional()?)
}

/// Full-text search over cached mail (FR-9 local path).
pub fn search(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<(i64, String, String)>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT m.id, COALESCE(m.subject, ''), COALESCE(m.from_json, '')
         FROM messages_fts f JOIN messages m ON m.id = f.rowid
         WHERE messages_fts MATCH ?1
         ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------- contacts

/// Bookkeeping on receive (spec §4): sender contact + per-sender counter.
pub fn note_received(conn: &Connection, account_id: i64, from: &[Addr]) -> Result<(), StoreError> {
    let now = now_ms();
    for addr in from {
        conn.execute(
            "INSERT INTO contacts(account_id, email, name, recv_from, last_seen,
                                  created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?4, ?4)
             ON CONFLICT(email) DO UPDATE SET
               recv_from = recv_from + 1, name = COALESCE(excluded.name, name),
               last_seen = excluded.last_seen, updated_at = excluded.updated_at",
            params![account_id, addr.email, addr.name, now],
        )?;
        conn.execute(
            "INSERT INTO sender_stats(account_id, sender_email, received, created_at, updated_at)
             VALUES (?1, ?2, 1, ?3, ?3)
             ON CONFLICT(account_id, sender_email) DO UPDATE SET
               received = received + 1, updated_at = excluded.updated_at",
            params![account_id, addr.email, now],
        )?;
    }
    Ok(())
}

/// Bookkeeping on send (used by the Phase-3 send path; listed here so the
/// counter lives next to `note_received`).
pub fn note_sent(conn: &Connection, to: &[Addr]) -> Result<(), StoreError> {
    let now = now_ms();
    for addr in to {
        conn.execute(
            "INSERT INTO contacts(email, sent_to, last_seen, created_at, updated_at)
             VALUES (?1, 1, ?2, ?2, ?2)
             ON CONFLICT(email) DO UPDATE SET
               sent_to = sent_to + 1, last_seen = excluded.last_seen,
               updated_at = excluded.updated_at",
            params![addr.email, now],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------- settings

/// Per-sender classification corrections (D-013): JSON map
/// sender → 'important'|'other' in `sender_stats.classification_overrides`.
pub fn classification_overrides(
    conn: &Connection,
    account_id: i64,
) -> Result<std::collections::HashMap<String, String>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT sender_email, classification_overrides FROM sender_stats
         WHERE account_id = ?1 AND classification_overrides IN ('important','other')",
    )?;
    let rows = stmt
        .query_map(params![account_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
    Ok(rows)
}

pub fn set_classification_override(
    conn: &Connection,
    account_id: i64,
    sender_email: &str,
    importance: &str,
) -> Result<(), StoreError> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO sender_stats(account_id, sender_email, classification_overrides,
                                  created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(account_id, sender_email) DO UPDATE SET
           classification_overrides = excluded.classification_overrides,
           updated_at = excluded.updated_at",
        params![account_id, sender_email, importance, now],
    )?;
    Ok(())
}

/// The account's own address(es), lowercased — `ClassifyContext::my_addresses`.
pub fn account_addresses(conn: &Connection, account_id: i64) -> Result<Vec<String>, StoreError> {
    let email: Option<String> = conn
        .query_row(
            "SELECT email FROM accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(email.into_iter().map(|e| e.to_lowercase()).collect())
}

/// Senders the user has actually written to (`contacts.sent_to > 0`).
pub fn two_way_senders(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare_cached("SELECT email FROM contacts WHERE sent_to > 0")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<serde_json::Value>, StoreError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn set_setting(
    conn: &Connection,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), StoreError> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO settings(key, value_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
           updated_at = excluded.updated_at",
        params![key, serde_json::to_string(value)?, now],
    )?;
    Ok(())
}
