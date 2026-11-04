//! Sync engine (spec §4): header-only backfill in idempotent batches, new-mail
//! incremental fetch, expunge/flag reconciliation without QRESYNC, lazy body
//! fetch, JWZ threading and contact bookkeeping. Pure functions over
//! `MailTransport` + `Store` so the L3 mock drives every path.

use std::time::Duration;

use crate::mail::headers::{parse_address_list, parse_rfc2822_date};
use crate::mail::thread::assign_threads;
use crate::mail::transport::{FetchedMeta, IdleEvent, MailTransport, TransportError, UidRange};
use crate::store::dao::{self, NewMessage};
use crate::store::Store;

pub const DEFAULT_BACKFILL_DAYS: i64 = 90;
pub const DEFAULT_BATCH_SIZE: usize = 500;
pub const BODY_PREFETCH_LIMIT: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("store: {0}")]
    Store(#[from] crate::store::StoreError),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
}

#[derive(Debug, Default, PartialEq)]
pub struct SyncOutcome {
    pub backfilled: usize,
    pub fetched_new: usize,
    pub flags_updated: usize,
    pub expunged: usize,
    pub rebaselined: bool,
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub backfill_days: i64,
    pub batch_size: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions {
            backfill_days: DEFAULT_BACKFILL_DAYS,
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }
}

fn meta_to_new_message(m: &FetchedMeta) -> NewMessage {
    let h = &m.headers;
    NewMessage {
        uid: m.uid,
        message_id: h.get("message-id").cloned(),
        in_reply_to: h.get("in-reply-to").cloned(),
        references_raw: h.get("references").cloned(),
        subject: h.get("subject").cloned(),
        from: h.get("from").map(|s| parse_address_list(s)),
        to: h.get("to").map(|s| parse_address_list(s)),
        cc: h.get("cc").map(|s| parse_address_list(s)),
        date_sent: h.get("date").and_then(|s| parse_rfc2822_date(s)),
        date_recv: Some(m.internaldate_ms),
        size: Some(m.size as i64),
        flags: m.flags.clone(),
        list_unsubscribe: h.get("list-unsubscribe").cloned(),
        list_unsub_post: h.get("list-unsubscribe-post").cloned(),
        thrid: m.thrid.clone(),
        // Verdict filled in by store_batch (classification needs DB context).
        importance: None,
        importance_reasons: Vec::new(),
    }
}

/// Runs one full sync of `mailbox`: select, incremental fetch of new mail,
/// backfill when needed, reconciliation, threading and contact bookkeeping.
pub fn sync_folder(
    t: &mut dyn MailTransport,
    store: &Store,
    account_id: i64,
    mailbox: &str,
    opts: &SyncOptions,
) -> Result<SyncOutcome, SyncError> {
    let mut outcome = SyncOutcome::default();
    let conn = store.conn();

    // Watermarks are read *before* upsert_folder writes the fresh SELECT data.
    let prior = dao::folder_state_by_name(conn, account_id, mailbox)?;
    let role = t
        .list_folders()?
        .into_iter()
        .find(|f| f.name == mailbox)
        .and_then(|f| f.role);

    let sel = t.select(mailbox)?;
    dao::upsert_folder(
        conn,
        account_id,
        mailbox,
        role.map(|r| r.as_str()),
        &dao::FolderUpsert {
            uidvalidity: sel.uidvalidity,
            uidnext: sel.uidnext,
            highestmodseq: sel.highestmodseq,
        },
    )?;
    let folder_id = dao::folder_state_by_name(conn, account_id, mailbox)?
        .expect("folder row exists after upsert")
        .id;

    match (&prior, sel.uidvalidity) {
        // Fresh folder: backfill covers everything up to UIDNEXT.
        (None, _) => {
            outcome.backfilled += backfill(t, conn, account_id, folder_id, sel.uidnext, opts)?;
        }
        // UIDVALIDITY changed: upsert already dropped the folder's rows and
        // reset its watermarks — re-baseline by backfilling again (spec §4.2).
        (Some(_), _)
            if prior
                .as_ref()
                .is_some_and(|p| p.uidvalidity != sel.uidvalidity) =>
        {
            outcome.rebaselined = true;
            outcome.backfilled += backfill(t, conn, account_id, folder_id, sel.uidnext, opts)?;
        }
        // Steady state: new mail above the prior UIDNEXT watermark.
        (Some(prior_state), _) => {
            if sel.uidnext > prior_state.uidnext {
                let metas = t.uid_fetch_headers(UidRange::Interval {
                    from: prior_state.uidnext,
                    to: sel.uidnext - 1,
                })?;
                outcome.fetched_new += store_batch(conn, account_id, folder_id, &metas)?;
            }
            if prior_state.highest_uid != 1 {
                outcome.backfilled += backfill(t, conn, account_id, folder_id, sel.uidnext, opts)?;
            }
        }
    }

    // Reconciliation without QRESYNC (spec §4.4).
    let (expunged, flags_updated) = reconcile(conn, t, folder_id)?;
    outcome.expunged = expunged;
    outcome.flags_updated = flags_updated;

    dao::set_account_sync_state(conn, account_id, "synced", Some(dao::now_ms()))?;
    rethread(store, account_id)?;

    Ok(outcome)
}

/// Inserts fetched metas + classification + contact bookkeeping; returns
/// rows touched. Classification context loads once per batch (D-013: rules
/// only, instant, offline).
fn store_batch(
    conn: &rusqlite::Connection,
    account_id: i64,
    folder_id: i64,
    metas: &[FetchedMeta],
) -> Result<usize, SyncError> {
    if metas.is_empty() {
        return Ok(0);
    }
    let ctx = crate::classify::ClassifyContext {
        my_addresses: dao::account_addresses(conn, account_id)?,
        sender_overrides: dao::classification_overrides(conn, account_id)?
            .into_iter()
            .filter_map(|(k, v)| crate::classify::Importance::parse(&v).map(|i| (k, i)))
            .collect(),
        two_way_senders: dao::two_way_senders(conn)?,
    };
    let batch: Vec<NewMessage> = metas
        .iter()
        .map(|m| {
            let mut nm = meta_to_new_message(m);
            let verdict = crate::classify::classify(&nm, &ctx);
            nm.importance = Some(verdict.importance.as_str().to_string());
            nm.importance_reasons = verdict.reasons.into_iter().map(|r| r.to_string()).collect();
            nm
        })
        .collect();
    let froms: Vec<_> = batch
        .iter()
        .filter_map(|m| m.from.clone())
        .flatten()
        .collect();
    let n = dao::insert_messages(conn, account_id, folder_id, &batch)?;
    if !froms.is_empty() {
        dao::note_received(conn, account_id, &froms)?;
    }
    Ok(n)
}

/// Newest-first batched backfill; each batch commits together with the cursor
/// so a kill mid-backfill resumes without duplicates or loss (spec §4.3, V-2.3).
/// Cursor semantics on `folders.highest_uid`: lowest contiguous covered UID,
/// `1` = backfill complete.
fn backfill(
    t: &mut dyn MailTransport,
    conn: &rusqlite::Connection,
    account_id: i64,
    folder_id: i64,
    server_uidnext: u32,
    opts: &SyncOptions,
) -> Result<usize, SyncError> {
    let mut total = 0usize;
    let cutoff_ms = dao::now_ms() - opts.backfill_days * 86_400_000;
    loop {
        let cursor = folder_highest(conn, folder_id)?;
        let batch_end = match cursor {
            0 => server_uidnext.saturating_sub(1),
            1 => break, // already complete
            n => n - 1, // continue below the covered range
        };
        if batch_end == 0 {
            dao::set_folder_highest_uid(conn, folder_id, 1)?;
            break;
        }
        let batch_start = batch_end.saturating_sub(opts.batch_size as u32 - 1).max(1);
        let metas = t.uid_fetch_headers(UidRange::Interval {
            from: batch_start,
            to: batch_end,
        })?;
        if metas.is_empty() {
            dao::set_folder_highest_uid(conn, folder_id, 1)?;
            break;
        }
        total += store_batch(conn, account_id, folder_id, &metas)?;
        // Cursor advances to the batch start only after the batch write; a
        // kill between the two replays the batch idempotently (upsert).
        dao::set_folder_highest_uid(conn, folder_id, batch_start)?;

        let oldest = metas.iter().map(|m| m.internaldate_ms).min().unwrap_or(0);
        if (oldest != 0 && oldest < cutoff_ms) || batch_start == 1 {
            dao::set_folder_highest_uid(conn, folder_id, 1)?;
            break;
        }
    }
    Ok(total)
}

fn folder_highest(conn: &rusqlite::Connection, folder_id: i64) -> Result<u32, SyncError> {
    Ok(conn.query_row(
        "SELECT highest_uid FROM folders WHERE id = ?1",
        [folder_id],
        |r| r.get(0),
    )?)
}

/// Expunge + flag reconciliation without QRESYNC (spec §4.4): UID SEARCH ALL
/// diff for vanishes, then a FLAGS snapshot diff for changes.
fn reconcile(
    conn: &rusqlite::Connection,
    t: &mut dyn MailTransport,
    folder_id: i64,
) -> Result<(usize, usize), SyncError> {
    let local = dao::folder_uids(conn, folder_id)?;
    if local.is_empty() {
        return Ok((0, 0));
    }
    let remote: std::collections::HashSet<u32> = t.uid_search_all()?.into_iter().collect();
    let expunged: Vec<u32> = local
        .iter()
        .copied()
        .filter(|u| !remote.contains(u))
        .collect();
    let removed = dao::delete_uids(conn, folder_id, &expunged)?;

    let mut flags_updated = 0usize;
    let local_flags = dao::message_flags(conn, folder_id, &local)?;
    let local_map: std::collections::HashMap<u32, Vec<String>> = local_flags.into_iter().collect();
    for (uid, remote_flags) in t.fetch_flags(UidRange::All)? {
        if let Some(local_flags) = local_map.get(&uid) {
            if local_flags.len() != remote_flags.len()
                || local_flags.iter().any(|f| !remote_flags.contains(f))
            {
                dao::update_flags(conn, folder_id, uid, &remote_flags)?;
                flags_updated += 1;
            }
        }
    }
    Ok((removed, flags_updated))
}

/// Recomputes JWZ threading for the account and persists it (D-010).
/// X-GM-THRID values were stored at insert time and flow through unmodified.
pub fn rethread(store: &Store, account_id: i64) -> Result<(), SyncError> {
    let conn = store.conn();
    let inputs = dao::thread_inputs(conn, account_id)?;
    if inputs.is_empty() {
        return Ok(());
    }
    let assignments = assign_threads(&inputs);
    dao::apply_threads(conn, account_id, &assignments)?;
    Ok(())
}

/// Lazy body fetch for the newest unread messages missing bodies (spec §4.5).
pub fn fetch_missing_bodies(
    t: &mut dyn MailTransport,
    store: &Store,
    folder_id: i64,
    limit: usize,
) -> Result<usize, SyncError> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, uid, subject FROM messages
         WHERE folder_id = ?1 AND unread = 1 AND body_text IS NULL AND body_html IS NULL
         ORDER BY date_recv DESC LIMIT ?2",
    )?;
    let targets: Vec<(i64, u32, Option<String>)> = stmt
        .query_map([folder_id, limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut fetched = 0usize;
    for (message_id, uid, subject) in targets {
        if let Some(body) = t.fetch_body(uid)? {
            let snippet = snippet_from(
                body.text.as_deref(),
                body.html.as_deref(),
                subject.as_deref().unwrap_or(""),
            );
            dao::set_body(
                conn,
                message_id,
                body.text.as_deref(),
                body.html.as_deref(),
                &snippet,
            )?;
            fetched += 1;
        }
    }
    Ok(fetched)
}

/// Snippet = first plain-text characters of the body (HTML crudely stripped
/// for snippet purposes only; real sanitization is the Phase-3 render pipeline).
fn snippet_from(text: Option<&str>, html: Option<&str>, subject: &str) -> String {
    let raw = match (text, html) {
        (Some(t), _) => t.to_string(),
        (None, Some(h)) => h.to_string(),
        _ => subject.to_string(),
    };
    let stripped = if raw.contains('<') {
        let mut out = String::with_capacity(raw.len());
        let mut in_tag = false;
        for ch in raw.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag => out.push(c),
                _ => {}
            }
        }
        out
    } else {
        raw
    };
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

/// Maps one IDLE round onto the store (spec §4.4 steady state); the async
/// driver loop calls this after each `idle_once`.
pub fn apply_idle_event(
    t: &mut dyn MailTransport,
    store: &Store,
    account_id: i64,
    folder_id: i64,
    event: IdleEvent,
) -> Result<SyncOutcome, SyncError> {
    let conn = store.conn();
    let mut outcome = SyncOutcome::default();
    match event {
        IdleEvent::Quiet => {}
        IdleEvent::Exists(_) => {
            outcome.fetched_new += fetch_new_above_watermark(t, conn, account_id, folder_id)?;
        }
        IdleEvent::FlagsChanged(uids) => {
            let (from, to) = uids
                .iter()
                .fold((u32::MAX, 0u32), |(lo, hi), u| (lo.min(*u), hi.max(*u)));
            for (uid, flags) in t.fetch_flags(UidRange::Interval { from, to })? {
                if uids.contains(&uid) {
                    dao::update_flags(conn, folder_id, uid, &flags)?;
                    outcome.flags_updated += 1;
                }
            }
        }
        IdleEvent::Expunged(uids) => {
            outcome.expunged += dao::delete_uids(conn, folder_id, &uids)?;
        }
    }
    rethread(store, account_id)?;
    Ok(outcome)
}

fn fetch_new_above_watermark(
    t: &mut dyn MailTransport,
    conn: &rusqlite::Connection,
    account_id: i64,
    folder_id: i64,
) -> Result<usize, SyncError> {
    let mailbox: String =
        conn.query_row("SELECT name FROM folders WHERE id = ?1", [folder_id], |r| {
            r.get(0)
        })?;
    let stored_uidnext: u32 = conn.query_row(
        "SELECT uidnext FROM folders WHERE id = ?1",
        [folder_id],
        |r| r.get(0),
    )?;
    let sel = t.select(&mailbox)?;
    if sel.uidnext <= stored_uidnext {
        return Ok(0);
    }
    let metas = t.uid_fetch_headers(UidRange::Interval {
        from: stored_uidnext,
        to: sel.uidnext - 1,
    })?;
    let n = store_batch(conn, account_id, folder_id, &metas)?;
    dao::set_folder_uidnext(conn, folder_id, sel.uidnext)?;
    Ok(n)
}

/// Reconnect backoff: 1 s doubling to a 60 s cap (spec §11).
pub fn backoff_delay(attempt: u32) -> Duration {
    let secs = 1u64.checked_shl(attempt.min(6)).unwrap_or(60);
    Duration::from_secs(secs.min(60))
}

#[cfg(test)]
mod tests;
