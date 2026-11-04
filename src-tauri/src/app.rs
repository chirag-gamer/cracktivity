//! App wiring (Phase 3): shared state, the IPC command surface the Fluent UI
//! hydrates from (offline-first, FR-8), and the background sync driver that
//! owns the blocking IMAP transports on a dedicated OS thread (spec §4).
//!
//! Concurrency model: `Store` (rusqlite `Connection`) is `Send`/`!Sync`, so
//! it lives in an `Arc<Mutex<_>>` shared by commands and the driver. Commands
//! are `async fn` (they run on the async runtime, never the UI thread) and
//! hold the lock only for plain SQLite work — no `.await` under a guard.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};
use tauri::{AppHandle, Emitter, State};
use url::Url;

use crate::mail::actions as mail_actions;
use crate::mail::imap::{ImapAuth, ImapTransport};
use crate::mail::sync::{self, SyncOptions};
use crate::platform::providers;
use crate::store::queries::{self, ImportanceFilter, Scope};
use crate::store::{Store, StoreError};

/// Steady-state poll cadence (spec §4.4: 90 s for the no-IDLE fallback —
/// io-imap's `idle_once` is a documented NOOP poll, D-019).
const POLL_INTERVAL: Duration = Duration::from_secs(90);

// ------------------------------------------------------------------- state

/// Driver-visible account status (FR-5 connection state).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct AccountStatus {
    pub account_id: i64,
    /// connecting | syncing | ok | offline | auth_failed
    pub state: String,
    pub error: Option<String>,
}

pub enum DriverMsg {
    /// Server-first user action on one message (run even between polls).
    Action(MessageAction),
    /// Wake one account for an immediate sync cycle.
    SyncNow(i64),
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub enum MessageAction {
    MarkRead { message_id: i64, read: bool },
    SetStarred { message_id: i64, starred: bool },
    Archive { message_id: i64 },
}

/// Everything the driver thread needs; produced together with [`AppState`].
pub struct DriverChannels {
    pub rx: mpsc::Receiver<DriverMsg>,
    pub store: Arc<Mutex<Store>>,
    pub statuses: Arc<Mutex<HashMap<i64, AccountStatus>>>,
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<Mutex<Store>>,
    tx: mpsc::Sender<DriverMsg>,
    statuses: Arc<Mutex<HashMap<i64, AccountStatus>>>,
}

impl AppState {
    /// Opens the encrypted store in `profile_dir`. The caller spawns
    /// [`driver_loop`] with the returned channels (it needs the AppHandle,
    /// which does not exist yet at store-open time).
    pub fn new(profile_dir: &std::path::Path) -> Result<(AppState, DriverChannels), StoreError> {
        let store = Arc::new(Mutex::new(Store::open(
            profile_dir,
            crate::store::dek::DEFAULT_DEK_TARGET,
        )?));
        let (tx, rx) = mpsc::channel::<DriverMsg>();
        let statuses = Arc::new(Mutex::new(HashMap::new()));
        Ok((
            AppState {
                store: store.clone(),
                tx: tx.clone(),
                statuses: statuses.clone(),
            },
            DriverChannels {
                rx,
                store,
                statuses,
            },
        ))
    }

    /// Test constructor: state over an existing store, no driver thread.
    #[cfg(test)]
    pub fn with_store(store: Store) -> AppState {
        let (tx, _rx) = mpsc::channel();
        AppState {
            store: Arc::new(Mutex::new(store)),
            tx,
            statuses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// A dead driver thread must not wedge commands; ignore send errors.
    fn send(&self, msg: DriverMsg) {
        let _ = self.tx.send(msg);
    }

    /// Shared store handle for setup-time work (e.g. the e2e seed).
    pub fn store_ref(&self) -> &Arc<Mutex<Store>> {
        &self.store
    }
}

// ------------------------------------------------------------------ driver

#[derive(Debug, Clone)]
struct AccountRow {
    id: i64,
    provider: String,
    email: String,
    auth_type: String,
    credential_ref: String,
    imap_host: Option<String>,
    imap_port: Option<u16>,
}

const ACCOUNT_COLS: &str = "id, provider, email, auth_type, credential_ref, imap_host, imap_port";

fn map_account_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRow> {
    Ok(AccountRow {
        id: r.get(0)?,
        provider: r.get(1)?,
        email: r.get(2)?,
        auth_type: r.get(3)?,
        credential_ref: r.get(4)?,
        imap_host: r.get(5)?,
        imap_port: r.get(6)?,
    })
}

fn enabled_accounts(conn: &rusqlite::Connection) -> Vec<AccountRow> {
    let Ok(mut stmt) = conn.prepare(&format!(
        "SELECT {ACCOUNT_COLS} FROM accounts WHERE enabled = 1 ORDER BY ord, id"
    )) else {
        return Vec::new();
    };
    stmt.query_map([], map_account_row)
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

fn account_of_message(conn: &rusqlite::Connection, message_id: i64) -> Option<AccountRow> {
    let account_id = queries::message_address(conn, message_id).ok()??.account_id;
    conn.query_row(
        &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?1"),
        [account_id],
        map_account_row,
    )
    .ok()
}

fn set_status(
    statuses: &Arc<Mutex<HashMap<i64, AccountStatus>>>,
    app: Option<&AppHandle>,
    account_id: i64,
    state: &str,
    error: Option<String>,
) {
    let st = AccountStatus {
        account_id,
        state: state.into(),
        error,
    };
    if let Some(app) = app {
        let _ = app.emit("account-status", &st);
    }
    if let Ok(mut map) = statuses.lock() {
        map.insert(account_id, st);
    }
}

/// Resolves host/port: preset providers carry verified constants; manual
/// accounts read their stored values (FR-1).
fn endpoint_of(acc: &AccountRow) -> Result<(String, u16), String> {
    if let Some(preset) = providers::ALL.iter().find(|p| p.id == acc.provider) {
        return Ok((preset.imap_host.into(), preset.imap_port));
    }
    let host = acc
        .imap_host
        .clone()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "manual account is missing its IMAP host".to_string())?;
    Ok((host, acc.imap_port.unwrap_or(993)))
}

fn connect(acc: &AccountRow) -> Result<ImapTransport, String> {
    let (host, port) = endpoint_of(acc)?;
    let url: Url = format!("imaps://{host}:{port}")
        .parse()
        .map_err(|e| format!("bad IMAP endpoint {host}:{port}: {e}"))?;
    let auth = match acc.auth_type.as_str() {
        "app_password" => {
            let secret = providers::get_account_secret(&acc.credential_ref)
                .map_err(|e| format!("credential read failed: {e}"))?;
            ImapAuth::Plain {
                user: acc.email.clone(),
                password: SecretString::from(secret),
            }
        }
        "oauth" => {
            let refresh_token = providers::get_account_secret(&acc.credential_ref)
                .map_err(|e| format!("credential read failed: {e}"))?;
            let tokens = crate::platform::oauth::refresh(&SecretString::from(refresh_token))
                .map_err(|e| format!("token refresh failed: {e}"))?;
            // Refresh tokens rotate on use (D-022): persist the replacement.
            let target = providers::refresh_token_target(acc.id);
            let rotated = tokens.refresh_token.expose_secret();
            providers::set_account_secret(&target, rotated)
                .map_err(|e| format!("rotated refresh token could not be stored: {e}"))?;
            ImapAuth::Xoauth2 {
                user: acc.email.clone(),
                token: tokens.access_token,
            }
        }
        other => return Err(format!("unknown auth type {other}")),
    };
    ImapTransport::connect(&url, auth)
        .map(|(t, _caps)| t)
        .map_err(|e| e.to_string())
}

/// One full cycle for an account: connect if needed, sync INBOX, prefetch
/// bodies. Transport-level failures drop the connection (reconnect next
/// cycle with backoff, spec §11) and set FR-5 state.
fn sync_cycle(
    app: Option<&AppHandle>,
    store: &Arc<Mutex<Store>>,
    statuses: &Arc<Mutex<HashMap<i64, AccountStatus>>>,
    acc: &AccountRow,
    transport: &mut Option<ImapTransport>,
) -> bool {
    if transport.is_none() {
        set_status(statuses, app, acc.id, "connecting", None);
        match connect(acc) {
            Ok(t) => *transport = Some(t),
            Err(e) => {
                let authish = e.to_lowercase().contains("auth") || e.contains("token");
                set_status(
                    statuses,
                    app,
                    acc.id,
                    if authish { "auth_failed" } else { "offline" },
                    Some(e),
                );
                return false;
            }
        }
    }
    set_status(statuses, app, acc.id, "syncing", None);
    let Ok(st) = store.lock() else { return false };
    let result = match transport.as_mut() {
        Some(t) => {
            sync::sync_folder(t, &st, acc.id, "INBOX", &SyncOptions::default()).and_then(|_| {
                if let Some(f) = dao::folder_state_by_name(st.conn(), acc.id, "INBOX")? {
                    sync::fetch_missing_bodies(t, &st, f.id, sync::BODY_PREFETCH_LIMIT)?;
                }
                Ok(())
            })
        }
        None => return false,
    };
    match result {
        Ok(()) => {
            set_status(statuses, app, acc.id, "ok", None);
            if let Some(app) = app {
                let _ = app.emit("store-changed", ());
            }
            true
        }
        Err(e) => {
            if matches!(
                e,
                sync::SyncError::Transport(crate::mail::transport::TransportError::Disconnected(_))
                    | sync::SyncError::Transport(crate::mail::transport::TransportError::Auth(_))
            ) {
                *transport = None;
            }
            set_status(statuses, app, acc.id, "offline", Some(e.to_string()));
            false
        }
    }
}

/// Runs a user action against the account's transport (server first, then
/// the local mirror inside `mail::actions`).
fn run_action(
    app: Option<&AppHandle>,
    store: &Arc<Mutex<Store>>,
    statuses: &Arc<Mutex<HashMap<i64, AccountStatus>>>,
    acc: &AccountRow,
    transport: &mut Option<ImapTransport>,
    action: MessageAction,
) {
    if transport.is_none() {
        set_status(statuses, app, acc.id, "connecting", None);
        match connect(acc) {
            Ok(t) => *transport = Some(t),
            Err(e) => {
                set_status(statuses, app, acc.id, "offline", Some(e));
                if let Some(app) = app {
                    let _ = app.emit("action-failed", format!("{action:?}"));
                }
                return;
            }
        }
    }
    let result = {
        let Ok(st) = store.lock() else { return };
        let Some(t) = transport.as_mut() else { return };
        match action {
            MessageAction::MarkRead { message_id, read } => {
                mail_actions::mark_read(t, &st, message_id, read)
            }
            MessageAction::SetStarred {
                message_id,
                starred,
            } => mail_actions::set_starred(t, &st, message_id, starred),
            MessageAction::Archive { message_id } => mail_actions::archive(t, &st, message_id),
        }
    };
    match result {
        Ok(()) => {
            set_status(statuses, app, acc.id, "ok", None);
            if let Some(app) = app {
                let _ = app.emit("store-changed", ());
            }
        }
        Err(e) => {
            if let Some(app) = app {
                let _ = app.emit("action-failed", format!("{e}"));
            }
            set_status(statuses, app, acc.id, "offline", Some(e.to_string()));
        }
    }
}

/// The driver thread body: blocking IMAP + SQLite, woken by channel.
/// `app` is `None` in unit tests.
pub fn driver_loop(ch: DriverChannels, app: Option<AppHandle>) -> ! {
    let DriverChannels {
        rx,
        store,
        statuses,
    } = ch;
    let app = app.as_ref();
    let mut transports: HashMap<i64, Option<ImapTransport>> = HashMap::new();
    let mut next_due: HashMap<i64, Instant> = HashMap::new();
    let mut failures: HashMap<i64, u32> = HashMap::new();

    loop {
        let accounts = store
            .lock()
            .map(|st| enabled_accounts(st.conn()))
            .unwrap_or_default();
        let now = Instant::now();
        let due: Vec<AccountRow> = accounts
            .iter()
            .filter(|a| next_due.get(&a.id).is_none_or(|t| *t <= now))
            .cloned()
            .collect();

        if !due.is_empty() {
            for acc in &due {
                let ok = sync_cycle(
                    app,
                    &store,
                    &statuses,
                    acc,
                    transports.entry(acc.id).or_insert(None),
                );
                let attempt = if ok {
                    failures.insert(acc.id, 0);
                    0
                } else {
                    *failures.entry(acc.id).and_modify(|a| *a += 1).or_insert(1)
                };
                let delay = if attempt == 0 {
                    POLL_INTERVAL
                } else {
                    sync::backoff_delay(attempt)
                };
                next_due.insert(acc.id, Instant::now() + delay);
            }
            continue; // accounts may have changed; re-evaluate before blocking
        }

        // Nothing due: block for the next driver message, capped by the
        // nearest deadline (new accounts must not wait a full poll cycle).
        let wait = next_due
            .values()
            .min()
            .map(|t| t.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(5))
            .min(Duration::from_secs(5));
        match rx.recv_timeout(wait) {
            Ok(DriverMsg::Quit) => std::process::exit(0),
            Ok(DriverMsg::SyncNow(id)) => {
                next_due.remove(&id);
            }
            Ok(DriverMsg::Action(action)) => {
                let message_id = match action {
                    MessageAction::MarkRead { message_id, .. }
                    | MessageAction::SetStarred { message_id, .. }
                    | MessageAction::Archive { message_id } => message_id,
                };
                let acc = store
                    .lock()
                    .ok()
                    .and_then(|st| account_of_message(st.conn(), message_id));
                match acc {
                    Some(acc) => {
                        run_action(
                            app,
                            &store,
                            &statuses,
                            &acc,
                            transports.entry(acc.id).or_insert(None),
                            action,
                        );
                    }
                    None => {
                        if let Some(app) = app {
                            let _ = app.emit("action-failed", format!("{action:?}"));
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => std::process::exit(0),
        }
    }
}

// ---------------------------------------------------------------- commands

use crate::store::dao;

type CmdResult<T> = Result<T, String>;

fn lock_store<'a>(state: &State<'a, AppState>) -> CmdResult<std::sync::MutexGuard<'a, Store>> {
    state
        .inner()
        .store
        .lock()
        .map_err(|_| "store poisoned".into())
}

fn scope_of(account_id: Option<i64>) -> Scope {
    match account_id {
        Some(id) => Scope::Account(id),
        None => Scope::Unified,
    }
}

fn filter_of(filter: Option<String>) -> ImportanceFilter {
    match filter.as_deref() {
        Some("important") => ImportanceFilter::Important,
        Some("other") => ImportanceFilter::Other,
        _ => ImportanceFilter::All,
    }
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> CmdResult<Vec<queries::AccountSummary>> {
    let st = lock_store(&state)?;
    queries::list_accounts(st.conn()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn account_statuses(state: State<'_, AppState>) -> CmdResult<Vec<AccountStatus>> {
    let map = state.inner().statuses.lock().map_err(|_| "poisoned")?;
    Ok(map.values().cloned().collect())
}

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    filter: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> CmdResult<Vec<queries::ThreadRow>> {
    let st = lock_store(&state)?;
    queries::thread_list(
        st.conn(),
        scope_of(account_id),
        filter_of(filter),
        limit.unwrap_or(200),
        offset.unwrap_or(0),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn thread_messages(
    state: State<'_, AppState>,
    thrid: String,
) -> CmdResult<Vec<queries::MessageRow>> {
    let st = lock_store(&state)?;
    queries::thread_messages(st.conn(), &thrid).map_err(|e| e.to_string())
}

/// One message's body: cached text + the sanitized render (spec §8 v1).
#[derive(serde::Serialize)]
pub struct OpenedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub blocked_images: usize,
    pub snippet: String,
}

#[tauri::command]
pub async fn message_body(state: State<'_, AppState>, message_id: i64) -> CmdResult<OpenedBody> {
    let st = lock_store(&state)?;
    let body = queries::message_body(st.conn(), message_id)
        .map_err(|e| e.to_string())?
        .ok_or("message not found")?;
    let rendered = body.html.as_deref().map(crate::render::render_document);
    Ok(OpenedBody {
        text: body.text,
        html: rendered.as_ref().map(|r| r.html.clone()),
        blocked_images: rendered.as_ref().map_or(0, |r| r.blocked_images),
        snippet: body.snippet,
    })
}

#[tauri::command]
pub async fn search_messages(
    state: State<'_, AppState>,
    query: String,
) -> CmdResult<Vec<queries::SearchRow>> {
    let st = lock_store(&state)?;
    queries::search(st.conn(), &query, 200).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unread_total(state: State<'_, AppState>) -> CmdResult<i64> {
    let st = lock_store(&state)?;
    queries::total_unread(st.conn()).map_err(|e| e.to_string())
}

/// Marks read/unread: local mirror now (snappy UI), server flag via the
/// driver; if the server op fails, the next reconcile restores server truth.
#[tauri::command]
pub async fn mark_read(state: State<'_, AppState>, message_id: i64, read: bool) -> CmdResult<()> {
    {
        let st = lock_store(&state)?;
        queries::set_message_read(st.conn(), message_id, !read).map_err(|e| e.to_string())?;
    }
    state
        .inner()
        .send(DriverMsg::Action(MessageAction::MarkRead {
            message_id,
            read,
        }));
    Ok(())
}

#[tauri::command]
pub async fn set_starred(
    state: State<'_, AppState>,
    message_id: i64,
    starred: bool,
) -> CmdResult<()> {
    {
        let st = lock_store(&state)?;
        queries::set_message_starred(st.conn(), message_id, starred).map_err(|e| e.to_string())?;
    }
    state
        .inner()
        .send(DriverMsg::Action(MessageAction::SetStarred {
            message_id,
            starred,
        }));
    Ok(())
}

/// Archive goes through the driver (server MOVE first — otherwise the
/// message would resurrect on the next reconcile); the store-changed event
/// refreshes the UI when it lands.
#[tauri::command]
pub async fn archive_message(state: State<'_, AppState>, message_id: i64) -> CmdResult<()> {
    state
        .inner()
        .send(DriverMsg::Action(MessageAction::Archive { message_id }));
    Ok(())
}

/// Bucket correction + classifier teaching (FR-11 AC) — local only.
#[tauri::command]
pub async fn set_importance(
    state: State<'_, AppState>,
    message_id: i64,
    important: bool,
) -> CmdResult<()> {
    let st = lock_store(&state)?;
    mail_actions::set_importance(&st, message_id, important).map_err(|e| e.to_string())
}

/// Adds an account (FR-1): preset or manual IMAP; the app password goes to
/// Credential Manager only; the driver wakes immediately.
#[tauri::command]
pub async fn add_account(
    state: State<'_, AppState>,
    app: AppHandle,
    provider_id: String,
    email: String,
    password: String,
    imap_host: Option<String>,
    imap_port: Option<u16>,
) -> CmdResult<queries::AccountSummary> {
    let email_clean = email.trim().to_string();
    if !email_clean.contains('@') {
        return Err("enter a valid email address".into());
    }
    let preset = providers::ALL
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("unknown provider {provider_id}"))?;
    if preset.auth == providers::AuthKind::OAuth2 {
        return Err(
            "Outlook.com uses browser sign-in; the connect flow ships with the Entra \
             registration (tracked for V-2.4)"
                .into(),
        );
    }
    if preset.id == "generic" && imap_host.as_deref().is_none_or(|h| h.trim().is_empty()) {
        return Err("manual accounts need the IMAP server host".into());
    }

    let account_id = {
        let st = lock_store(&state)?;
        // Insert first (the Credential Manager target is id-keyed), store the
        // secret, then link the row; a secret failure removes the half-made row.
        let id = dao::add_account(
            st.conn(),
            &dao::NewAccount {
                provider: preset.id,
                email: &email_clean,
                display_name: None,
                auth_type: "app_password",
                credential_ref: "",
                imap_host: imap_host.as_deref().filter(|h| !h.trim().is_empty()),
                imap_port,
            },
        )
        .map_err(|e| e.to_string())?;
        let credential_ref = providers::app_password_target(id);
        match providers::set_account_secret(&credential_ref, password.trim()) {
            Ok(()) => {
                st.conn()
                    .execute(
                        "UPDATE accounts SET credential_ref = ?2 WHERE id = ?1",
                        rusqlite::params![id, credential_ref],
                    )
                    .map_err(|e| e.to_string())?;
                id
            }
            Err(e) => {
                let _ = st
                    .conn()
                    .execute("DELETE FROM accounts WHERE id = ?1", [id]);
                return Err(format!("could not store the app password: {e}"));
            }
        }
    };
    state.inner().send(DriverMsg::SyncNow(account_id));
    let _ = app; // reserved for a future "sync started" toast
    let st = lock_store(&state)?;
    queries::list_accounts(st.conn())
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| "account vanished".into())
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> CmdResult<()> {
    let ids: Vec<i64> = {
        let st = lock_store(&state)?;
        let mut stmt = st
            .conn()
            .prepare("SELECT id FROM accounts WHERE enabled = 1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    for id in ids {
        state.inner().send(DriverMsg::SyncNow(id));
    }
    Ok(())
}

/// Esc handler in the panel webview (FR-28).
#[tauri::command]
pub async fn hide_panel(app: AppHandle) -> CmdResult<()> {
    crate::platform::shell::hide_panel(&app).map_err(|e| e.to_string())
}

// -------------------------------------------------------------------- seed

/// Demo data for `--e2e-seed` (V-3.1): a disabled account (the driver must
/// not try to reach a real server from tests) with deterministic threads
/// covering the important/other split, unread/starred mix, and one HTML body
/// that exercises the sanitize pipeline. Idempotent: the profile persists
/// across runs, so a second launch must not duplicate the demo account.
pub fn seed_demo(store: &Store) -> i64 {
    let conn = store.conn();
    if let Ok(existing) = conn.query_row(
        "SELECT id FROM accounts WHERE credential_ref = 'cracktivity.e2e.seed' LIMIT 1",
        [],
        |r| r.get::<_, i64>(0),
    ) {
        return existing;
    }
    let account = dao::add_account(
        conn,
        &dao::NewAccount {
            provider: "imap",
            email: "demo@example.com",
            display_name: Some("Demo Account"),
            auth_type: "app_password",
            credential_ref: "cracktivity.e2e.seed",
            imap_host: None,
            imap_port: None,
        },
    )
    .unwrap();
    conn.execute("UPDATE accounts SET enabled = 0 WHERE id = ?1", [account])
        .unwrap();
    let folder = dao::upsert_folder(
        conn,
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
    .unwrap();

    let now = dao::now_ms();
    // (uid, thrid, subject, from name/email, to, unread, starred, importance,
    //  minutes ago, body text, html)
    type SeedRow = (
        u32,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        bool,
        bool,
        &'static str,
        i64,
        Option<&'static str>,
        Option<&'static str>,
    );
    let seed_rows: Vec<SeedRow> = vec![
        (
            1,
            "seed-personal",
            "Lunch on Friday?",
            "Priya",
            "priya@example.org",
            "demo@example.com",
            true,
            false,
            "important",
            8,
            Some("Same place as last time? 12:30 works for me."),
            None,
        ),
        (
            2,
            "seed-personal",
            "Re: Lunch on Friday?",
            "Demo",
            "demo@example.com",
            "priya@example.org",
            false,
            false,
            "important",
            5,
            Some("Perfect, see you there."),
            None,
        ),
        (
            3,
            "seed-newsletter",
            "This week in Rust",
            "Rust Weekly",
            "news@rust.example",
            "demo@example.com",
            true,
            false,
            "other",
            30,
            Some("Issue #900: async closures, crates, and jobs."),
            None,
        ),
        (
            4,
            "seed-receipt",
            "Your receipt from Grocer",
            "Grocer",
            "receipts@grocer.example",
            "demo@example.com",
            false,
            false,
            "other",
            120,
            Some("Order #12345 — $42.10. Thanks for shopping!"),
            None,
        ),
        (
            5,
            "seed-html",
            "Quarterly report attached",
            "Bank Statements",
            "statements@bank.example",
            "demo@example.com",
            true,
            true,
            "important",
            240,
            Some("Your quarterly statement is ready."),
            Some(
                r#"<p>Your <b>Q3 statement</b> is ready.</p><script>alert('evil')</script><img src="https://tracker.example/px.gif"><a href="https://bank.example/s?utm_source=mail&token=9">View statement</a>"#,
            ),
        ),
        (
            6,
            "seed-code",
            "Your sign-in code",
            "Service",
            "no-reply@service.example",
            "demo@example.com",
            true,
            false,
            "important",
            2,
            Some("Your verification code is 451902."),
            None,
        ),
    ];

    let batch: Vec<dao::NewMessage> = seed_rows
        .iter()
        .map(
            |(
                uid,
                thrid,
                subject,
                from_name,
                from_email,
                to,
                unread,
                starred,
                importance,
                mins_ago,
                _body,
                _html,
            )| {
                let mut flags = if *unread {
                    vec![]
                } else {
                    vec!["\\Seen".to_string()]
                };
                if *starred {
                    flags.push("\\Flagged".into());
                }
                dao::NewMessage {
                    uid: *uid,
                    message_id: Some(format!("<{uid}.demo@example.com>")),
                    in_reply_to: None,
                    references_raw: None,
                    subject: Some((*subject).into()),
                    from: Some(vec![dao::Addr {
                        name: Some((*from_name).into()),
                        email: (*from_email).into(),
                    }]),
                    to: Some(vec![dao::Addr {
                        name: None,
                        email: (*to).into(),
                    }]),
                    cc: None,
                    date_sent: Some(now - mins_ago * 60_000),
                    date_recv: Some(now - mins_ago * 60_000),
                    size: Some(2048),
                    flags,
                    list_unsubscribe: (*importance == "other")
                        .then(|| "<https://x.example/u>".into()),
                    list_unsub_post: None,
                    thrid: Some((*thrid).into()),
                    importance: Some((*importance).into()),
                    importance_reasons: vec!["seed".into()],
                }
            },
        )
        .collect();
    dao::insert_messages(conn, account, folder.id, &batch).unwrap();

    // Bodies land through the same lazy-body path the sync engine uses.
    for (uid, _, _, _, _, _, _, _, _, _, body, html) in &seed_rows {
        if body.is_some() || html.is_some() {
            if let Some(id) = dao::message_id_by_uid(conn, folder.id, *uid).unwrap() {
                let snippet = body.unwrap_or("");
                dao::set_body(
                    conn,
                    id,
                    *body,
                    *html,
                    &snippet.chars().take(160).collect::<String>(),
                )
                .unwrap();
            }
        }
    }
    account
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let target = format!("cracktivity.test.app.{}.{}", std::process::id(), line!());
        let store = Store::open(dir.path(), &target).unwrap();
        seed_demo(&store);
        (dir, AppState::with_store(store))
        // DEK entry unique per test; left behind harmlessly on dev machines,
        // ephemeral on CI runners.
    }

    #[test]
    fn seed_demo_produces_the_expected_views() {
        let (_dir, state) = seeded_state();
        let st = state.store.lock().unwrap();

        let accounts = queries::list_accounts(st.conn()).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].unread, 4, "uids 1,3,5,6 unread");

        let all =
            queries::thread_list(st.conn(), Scope::Unified, ImportanceFilter::All, 50, 0).unwrap();
        assert_eq!(all.len(), 5, "6 messages across 5 threads");

        let important = queries::thread_list(
            st.conn(),
            Scope::Unified,
            ImportanceFilter::Important,
            50,
            0,
        )
        .unwrap();
        assert_eq!(important.len(), 3);

        // FTS over seeded bodies.
        let hits = queries::search(st.conn(), "451902", 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].thrid.as_deref(), Some("seed-code"));
    }

    /// Body fetch + sanitize through the same code path the command uses.
    #[test]
    fn seeded_html_sanitizes_for_the_reader() {
        let (_dir, state) = seeded_state();
        let st = state.store.lock().unwrap();
        let id: i64 = st
            .conn()
            .query_row(
                "SELECT id FROM messages WHERE thrid = 'seed-html'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // seed_demo stored the HTML body via the lazy-body path.
        let body = queries::message_body(st.conn(), id).unwrap().unwrap();
        let html = body.html.expect("seeded html body");
        assert!(html.contains("script"), "raw (unsanitized) in the cache");

        let rendered = crate::render::render_document(&html);
        assert!(!rendered.html.contains("script"));
        assert!(!rendered.html.contains("tracker.example"));
        assert!(rendered.html.contains("Q3"));
        assert_eq!(rendered.blocked_images, 1);
    }

    /// Manual accounts without a host fail loudly (FR-1 honest error).
    #[test]
    fn manual_account_requires_host() {
        let acc = AccountRow {
            id: 1,
            provider: "imap".into(),
            email: "a@b.c".into(),
            auth_type: "app_password".into(),
            credential_ref: "x".into(),
            imap_host: None,
            imap_port: None,
        };
        assert!(endpoint_of(&acc).is_err());
    }

    /// L2 (D-018): the real command layer answers over the MockRuntime IPC
    /// path against a seeded encrypted store — the same surface the Fluent
    /// UI invokes (offline hydration, FR-8).
    #[test]
    fn commands_answer_over_mock_ipc() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        use tauri::webview::InvokeRequest;
        use tauri::Manager;

        let (_dir, state) = seeded_state();

        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                super::list_accounts,
                super::thread_list,
                super::thread_messages,
                super::search_messages,
                super::unread_total,
                super::mark_read,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        app.manage(state);
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let invoke = |cmd: &str, body: serde_json::Value| {
            tauri::test::get_ipc_response(
                &webview,
                InvokeRequest {
                    cmd: cmd.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(body),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .map(|b| b.deserialize::<serde_json::Value>().unwrap())
            .expect("ipc response")
        };

        // Unified list returns the seeded threads (camelCase args in).
        let threads = invoke(
            "thread_list",
            serde_json::json!({ "accountId": null, "filter": "all", "limit": 50, "offset": 0 }),
        );
        let rows = threads.as_array().unwrap();
        assert_eq!(rows.len(), 5, "6 seeded messages in 5 threads");
        assert!(rows[0]["thrid"].as_str().is_some());

        // Important filter narrows to 3.
        let important = invoke("thread_list", serde_json::json!({ "filter": "important" }));
        assert_eq!(important.as_array().unwrap().len(), 3);

        // FTS search finds the seeded code mail.
        let hits = invoke("search_messages", serde_json::json!({ "query": "451902" }));
        assert_eq!(hits.as_array().unwrap().len(), 1);

        // Unread total then a mark_read round trip.
        let unread = invoke("unread_total", serde_json::json!({}));
        assert_eq!(unread.as_i64(), Some(4));
        let code_id: i64 = app
            .state::<AppState>()
            .store
            .lock()
            .unwrap()
            .conn()
            .query_row(
                "SELECT id FROM messages WHERE thrid = 'seed-code'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        invoke(
            "mark_read",
            serde_json::json!({ "messageId": code_id, "read": true }),
        );
        let unread = invoke("unread_total", serde_json::json!({}));
        assert_eq!(unread.as_i64(), Some(3), "local mirror applied at once");
    }
}
