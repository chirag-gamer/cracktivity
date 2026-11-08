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
use crate::mail::transport::MailTransport as _;
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
    /// Bulk-archives every inbox message from one sender (FR-14). The reply
    /// carries the undo references for the session's Undo.
    BulkArchive {
        sender: String,
        reply: mpsc::Sender<Result<Vec<mail_actions::ArchivedRef>, String>>,
    },
    /// Sends a queued draft over SMTP (FR-18) on the driver thread; the reply
    /// reports success or the reason (the outbox row keeps the error too).
    SendDraft {
        draft_id: i64,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

#[derive(Debug, Clone)]
pub enum MessageAction {
    MarkRead {
        message_id: i64,
        read: bool,
    },
    SetStarred {
        message_id: i64,
        starred: bool,
    },
    Archive {
        message_id: i64,
    },
    /// Reverses one bulk-archive entry (FR-14 Undo): server MOVE back.
    RestoreArchived {
        account_id: i64,
        from_folder: String,
        uid: u32,
        to_folder: String,
    },
}

/// Everything the driver thread needs; produced together with [`AppState`].
/// The store here is a **second connection**: WAL allows the driver's
/// network-bound sync loop and the command layer's short queries to run
/// concurrently — the command-side mutex is never held across I/O
/// (FR-6: the list stays usable during backfill; review-pass D-028).
pub struct DriverChannels {
    pub rx: mpsc::Receiver<DriverMsg>,
    pub store: Store,
    pub statuses: Arc<Mutex<HashMap<i64, AccountStatus>>>,
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<Mutex<Store>>,
    tx: mpsc::Sender<DriverMsg>,
    statuses: Arc<Mutex<HashMap<i64, AccountStatus>>>,
    /// Loopback image-fetch proxy for allowed remote images (spec §8.5).
    /// `None` only if the listener could not bind — renders then fall back
    /// to direct https loads with the honest-IP caveat.
    proxy: Option<Arc<crate::render::proxy::ImageProxy>>,
    /// AI runtime (models, sidecar) behind its own lock so a running
    /// summarize/draft task does not block store queries (FR-21/22).
    ai: Arc<Mutex<crate::ai::AiState>>,
    /// The app data dir (D-006 split) — storage-usage diagnostics input.
    data_dir: std::path::PathBuf,
}

impl AppState {
    /// Opens the encrypted store in `app_data_dir` (`profile/` inside it per
    /// D-006; models live beside it in `models/`). The caller spawns
    /// [`driver_loop`] with the returned channels (it needs the AppHandle,
    /// which does not exist yet at store-open time).
    pub fn new(app_data_dir: &std::path::Path) -> Result<(AppState, DriverChannels), StoreError> {
        let profile_dir = app_data_dir.join("profile");
        let store = Arc::new(Mutex::new(Store::open(
            &profile_dir,
            crate::store::dek::DEFAULT_DEK_TARGET,
        )?));
        // Dedicated connection for the driver (see DriverChannels).
        let driver_store = Store::open(&profile_dir, crate::store::dek::DEFAULT_DEK_TARGET)?;
        let (tx, rx) = mpsc::channel::<DriverMsg>();
        let statuses = Arc::new(Mutex::new(HashMap::new()));
        let proxy = crate::render::proxy::start()
            .map(Arc::new)
            .map_err(StoreError::Io)?;
        Ok((
            AppState {
                store: store.clone(),
                tx,
                statuses: statuses.clone(),
                proxy: Some(proxy),
                ai: Arc::new(Mutex::new(crate::ai::AiState::new(
                    app_data_dir,
                    &app_data_dir.join("resources-fallback"),
                ))),
                data_dir: app_data_dir.to_path_buf(),
            },
            DriverChannels {
                rx,
                store: driver_store,
                statuses,
            },
        ))
    }

    /// Points the AI runtime at the app's real resource dir (called from
    /// setup once the AppHandle exists).
    pub fn set_ai_resource_dir(&self, resource_dir: &std::path::Path) {
        if let Ok(mut ai) = self.ai.lock() {
            ai.resource_dir = resource_dir.to_path_buf();
        }
    }

    /// AI state handle (commands run blocking inference on it off-runtime).
    pub fn ai_ref(&self) -> &Arc<Mutex<crate::ai::AiState>> {
        &self.ai
    }

    /// Test constructor: state over an existing store, no driver thread.
    #[cfg(test)]
    pub fn with_store(store: Store) -> AppState {
        let (tx, _rx) = mpsc::channel();
        let proxy = crate::render::proxy::start().ok().map(Arc::new);
        let dir = std::env::temp_dir();
        AppState {
            store: Arc::new(Mutex::new(store)),
            tx,
            statuses: Arc::new(Mutex::new(HashMap::new())),
            proxy,
            ai: Arc::new(Mutex::new(crate::ai::AiState::new(&dir, &dir))),
            data_dir: dir.clone(),
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
        if !preset.imap_host.is_empty() {
            return Ok((preset.imap_host.into(), preset.imap_port));
        }
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
    crate::platform::netlog::record(&host);
    ImapTransport::connect(&url, auth)
        .map(|(t, _caps)| t)
        .map_err(|e| e.to_string())
}

/// One full cycle for an account: connect if needed, sync INBOX, prefetch
/// bodies. Transport-level failures drop the connection (reconnect next
/// cycle with backoff, spec §11) and set FR-5 state.
fn sync_cycle(
    app: Option<&AppHandle>,
    store: &Store,
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
    let result = match transport.as_mut() {
        Some(t) => {
            sync::sync_folder(t, store, acc.id, "INBOX", &SyncOptions::default()).and_then(|_| {
                // Headers-only sent sync: the FR-13 follow-up view reads the
                // user's own latest-per-thread rows from here.
                let sent_name = t
                    .list_folders()?
                    .into_iter()
                    .find(|f| f.role == Some(crate::mail::transport::SpecialRole::Sent))
                    .map(|f| f.name);
                if let Some(name) = sent_name {
                    sync::sync_folder(t, store, acc.id, &name, &SyncOptions::default())?;
                }
                if let Some(f) = dao::folder_state_by_name(store.conn(), acc.id, "INBOX")? {
                    sync::fetch_missing_bodies(t, store, f.id, sync::BODY_PREFETCH_LIMIT)?;
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
    store: &Store,
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
    let Some(t) = transport.as_mut() else { return };
    let result: Result<(), mail_actions::ActionError> = match action {
        MessageAction::MarkRead { message_id, read } => {
            mail_actions::mark_read(t, store, message_id, read)
        }
        MessageAction::SetStarred {
            message_id,
            starred,
        } => mail_actions::set_starred(t, store, message_id, starred),
        MessageAction::Archive { message_id } => {
            // The single-archive path discards the undo ref; bulk undo is the
            // only consumer (FR-14).
            mail_actions::archive(t, store, message_id).map(|_| ())
        }
        MessageAction::RestoreArchived {
            account_id: _,
            from_folder,
            uid,
            to_folder,
        } => mail_actions::restore_archived(t, &from_folder, uid, &to_folder),
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

/// Bulk-archives every inbox message from `sender` (FR-14); returns the
/// undo references the session keeps for Undo. A failure mid-way aborts —
/// already-archived messages stay archived (still undoable, refs returned
/// only on success).
fn bulk_archive(
    store: &Store,
    transports: &mut HashMap<i64, Option<ImapTransport>>,
    sender: &str,
) -> Result<Vec<mail_actions::ArchivedRef>, String> {
    let ids = queries::sender_message_ids(store.conn(), sender).map_err(|e| e.to_string())?;
    let mut refs = Vec::new();
    for id in ids {
        let acc = account_of_message(store.conn(), id).ok_or("message vanished")?;
        let transport = transports.entry(acc.id).or_insert(None);
        if transport.is_none() {
            *transport = Some(connect(&acc)?);
        }
        let t = transport.as_mut().expect("just connected");
        refs.push(mail_actions::archive(t, store, id).map_err(|e| e.to_string())?);
    }
    Ok(refs)
}

/// Runs the queued send for one draft (FR-18) on the driver thread: status
/// transitions happen here so a killed driver never leaves a phantom
/// `sending` row without a terminal state on retry.
fn send_draft_via_driver(
    store: &Store,
    accounts: &[AccountRow],
    transports: &mut HashMap<i64, Option<ImapTransport>>,
    draft_id: i64,
) -> Result<(), String> {
    let fail = |store: &Store, msg: String| -> String {
        let _ = dao::set_draft_status(store.conn(), draft_id, "failed", Some(&msg));
        msg
    };
    let account_id: i64 = store
        .conn()
        .query_row(
            "SELECT account_id FROM drafts WHERE id = ?1",
            [draft_id],
            |r| r.get(0),
        )
        .map_err(|_| format!("draft {draft_id} vanished"))?;
    let acc = accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| fail(store, "account is disabled".into()))?;
    if let Err(e) = dao::set_draft_status(store.conn(), draft_id, "sending", None) {
        return Err(e.to_string());
    }
    let transport = transports.entry(acc.id).or_insert(None);
    if transport.is_none() {
        *transport = Some(connect(acc).map_err(|e| fail(store, e))?);
    }
    let password = providers::get_account_secret(&acc.credential_ref)
        .map_err(|e| fail(store, format!("credential read failed: {e}")))?;
    crate::mail::smtp::send_draft(
        transport.as_mut().expect("just connected"),
        store,
        draft_id,
        &password,
    )
    .map_err(|e| fail(store, e.to_string()))
}
/// Returns when the command side drops (app teardown) — process exit is
/// owned by tauri; this thread never races it with `process::exit`.
/// `app` is `None` in unit tests.
/// The driver thread body: blocking IMAP + SQLite, woken by channel.
/// Returns when the command side drops (app teardown) — process exit is
/// owned by tauri; this thread never races it with `process::exit`.
/// `app` is `None` in unit tests.
pub fn driver_loop(ch: DriverChannels, app: Option<AppHandle>) {
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
        let accounts = enabled_accounts(store.conn());
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
                if !ok {
                    crate::platform::applog::record(
                        "error",
                        format!("sync failed for account {} (attempt {attempt})", acc.id),
                    );
                }
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
            Ok(DriverMsg::SyncNow(id)) => {
                next_due.remove(&id);
            }
            Ok(DriverMsg::Action(MessageAction::RestoreArchived {
                account_id,
                from_folder,
                uid,
                to_folder,
            })) => {
                // Account-addressed (the local row is already gone).
                let acc = accounts.iter().find(|a| a.id == account_id);
                match acc {
                    Some(acc) => run_action(
                        app,
                        &store,
                        &statuses,
                        acc,
                        transports.entry(acc.id).or_insert(None),
                        MessageAction::RestoreArchived {
                            account_id,
                            from_folder,
                            uid,
                            to_folder,
                        },
                    ),
                    None => {
                        if let Some(app) = app {
                            let _ = app.emit("action-failed", "unknown account");
                        }
                    }
                }
            }
            Ok(DriverMsg::Action(action)) => {
                let message_id = match action {
                    MessageAction::MarkRead { message_id, .. }
                    | MessageAction::SetStarred { message_id, .. }
                    | MessageAction::Archive { message_id } => message_id,
                    MessageAction::RestoreArchived { .. } => unreachable!("handled above"),
                };
                let acc = account_of_message(store.conn(), message_id);
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
            Ok(DriverMsg::BulkArchive { sender, reply }) => {
                let _ = reply.send(bulk_archive(&store, &mut transports, &sender));
                if let Some(app) = app {
                    let _ = app.emit("store-changed", ());
                }
            }
            Ok(DriverMsg::SendDraft { draft_id, reply }) => {
                let result = send_draft_via_driver(&store, &accounts, &mut transports, draft_id);
                if result.is_ok() {
                    if let Some(app) = app {
                        let _ = app.emit("store-changed", ());
                    }
                }
                let _ = reply.send(result);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
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

/// Settings read/write over the encrypted store's `settings` table
/// (spec §3 — Credential-Manager-free app state only; no secrets here).
fn get_setting_value(state: &State<'_, AppState>, key: &str) -> Option<serde_json::Value> {
    let st = lock_store(state).ok()?;
    dao::get_setting(st.conn(), key).ok().flatten()
}

fn set_setting_value(
    state: &State<'_, AppState>,
    key: &str,
    value: &serde_json::Value,
) -> CmdResult<()> {
    let st = lock_store(state)?;
    dao::set_setting(st.conn(), key, value).map_err(|e| e.to_string())
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

/// One message's body: cached text + the sanitized render (spec §8 v2).
/// `show_images` overrides the per-sender allowlist for this render only.
#[derive(serde::Serialize)]
pub struct OpenedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub blocked_images: usize,
    pub tracking_count: usize,
    pub snippet: String,
}

#[tauri::command]
pub async fn message_body(
    state: State<'_, AppState>,
    message_id: i64,
    show_images: Option<bool>,
) -> CmdResult<OpenedBody> {
    let (body, allow, proxy_origin) = {
        let st = lock_store(&state)?;
        let body = queries::message_body(st.conn(), message_id)
            .map_err(|e| e.to_string())?
            .ok_or("message not found")?;
        // First open latches and credits the sender's engagement counter (FR-14).
        let _ = queries::mark_opened(st.conn(), message_id);
        let allow = show_images.unwrap_or_else(|| {
            queries::sender_show_images(st.conn(), message_id)
                .ok()
                .flatten()
                .unwrap_or(false)
        });
        (body, allow, proxy_origin_of(&state))
    };
    let opts = crate::render::RenderOptions {
        allow_remote_images: allow,
        proxy_origin,
    };
    let rendered = body
        .html
        .as_deref()
        .map(|h| crate::render::render_document_opts(h, &opts));
    if let Some(r) = &rendered {
        let st = lock_store(&state)?;
        let _ = queries::set_tracking_count(st.conn(), message_id, r.tracking_count);
    }
    Ok(OpenedBody {
        text: body.text,
        html: rendered.as_ref().map(|r| r.html.clone()),
        blocked_images: rendered.as_ref().map_or(0, |r| r.blocked_images),
        tracking_count: rendered.as_ref().map_or(0, |r| r.tracking_count),
        snippet: body.snippet,
    })
}

fn proxy_origin_of(state: &State<'_, AppState>) -> Option<String> {
    state.inner().proxy.as_ref().map(|p| p.origin.clone())
}

/// Per-sender image allowlist ("Always for this sender", FR-19 AC).
#[tauri::command]
pub async fn set_sender_images(
    state: State<'_, AppState>,
    message_id: i64,
    allow: bool,
) -> CmdResult<()> {
    let st = lock_store(&state)?;
    let addr = queries::message_address(st.conn(), message_id)
        .map_err(|e| e.to_string())?
        .ok_or("message not found")?;
    let sender = addr.sender_email.ok_or("message has no sender")?;
    queries::set_sender_show_images(st.conn(), addr.account_id, &sender, allow)
        .map_err(|e| e.to_string())
}

/// Local FTS search, optionally extended with server-side IMAP SEARCH over
/// uncached ranges (FR-9 completion). Server rows are ingested into the
/// cache (headers-only) and show up with `has_body = false`.
#[tauri::command]
pub async fn search_messages(
    state: State<'_, AppState>,
    query: String,
    also_server: Option<bool>,
) -> CmdResult<Vec<queries::SearchRow>> {
    let local = {
        let st = lock_store(&state)?;
        queries::search(st.conn(), &query, 200).map_err(|e| e.to_string())?
    };
    let want_server = also_server.unwrap_or(false)
        && get_setting_value(&state, "search.also_server")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    if !want_server || query.trim().is_empty() {
        return Ok(local);
    }
    // Server expansion runs off the async runtime (blocking IMAP).
    let st_state = state.inner().clone();
    let query_owned = query.clone();
    tauri::async_runtime::spawn_blocking(move || server_search_expand(&st_state, &query_owned))
        .await
        .map_err(|e| e.to_string())??;
    let st = lock_store(&state)?;
    queries::search(st.conn(), &query, 200).map_err(|e| e.to_string())
}

/// Connects to each enabled account, runs IMAP SEARCH, and ingests the
/// matches (headers-only) so the local FTS view merges them marked uncached.
/// Individual account failures are skipped — server search is best-effort.
fn server_search_expand(state: &AppState, query: &str) -> Result<(), String> {
    use crate::mail::transport::SearchField;
    let accounts: Vec<AccountRow> = {
        let st = state.store.lock().map_err(|_| "store poisoned")?;
        enabled_accounts(st.conn())
    };
    for acc in accounts {
        let metas = (|| -> Result<Vec<_>, String> {
            let mut t = connect(&acc)?;
            crate::mail::sync::server_search_fetch(&mut t, SearchField::Text, query)
                .map_err(|e| e.to_string())
        })();
        let Ok(metas) = metas else { continue };
        if metas.is_empty() {
            continue;
        }
        let st = state.store.lock().map_err(|_| "store poisoned")?;
        let folder = dao::folder_state_by_name(st.conn(), acc.id, "INBOX")
            .map_err(|e| e.to_string())?
            .ok_or("no inbox folder")?;
        crate::mail::sync::ingest_headers(&st, acc.id, folder.id, &metas)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Follow-up threshold in days (FR-13: default 3, user-set 1/2/3/5/7).
const FOLLOWUP_DAYS_KEY: &str = "followup.days";

#[tauri::command]
pub async fn followups_list(
    state: State<'_, AppState>,
    account_id: Option<i64>,
) -> CmdResult<Vec<queries::FollowupRow>> {
    let days = get_setting_value(&state, FOLLOWUP_DAYS_KEY)
        .and_then(|v| v.as_i64())
        .unwrap_or(3);
    let now = dao::now_ms();
    let st = lock_store(&state)?;
    queries::waiting_for_reply(st.conn(), scope_of(account_id), now, days.clamp(1, 7), 100)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_followup_days(state: State<'_, AppState>, days: i64) -> CmdResult<()> {
    set_setting_value(
        &state,
        FOLLOWUP_DAYS_KEY,
        &serde_json::json!(days.clamp(1, 7)),
    )
}

/// The subscription-cleanup ranking (FR-14).
#[tauri::command]
pub async fn cleanup_list(state: State<'_, AppState>) -> CmdResult<Vec<queries::SubscriptionRow>> {
    let st = lock_store(&state)?;
    queries::subscription_senders(st.conn()).map_err(|e| e.to_string())
}

/// One-click unsubscribe — invoked ONLY by the user's click (FR-14 AC). The
/// RFC 8058 POST carries no credentials and refuses redirects.
#[tauri::command]
pub async fn unsubscribe_sender(
    state: State<'_, AppState>,
    sender: String,
    list_unsubscribe: String,
    list_unsub_post: Option<String>,
) -> CmdResult<u16> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::mail::unsubscribe::one_click(&list_unsubscribe, list_unsub_post.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?;
    let status = result.map_err(|e| e.to_string())?;
    let st = lock_store(&state)?;
    dao::audit(
        st.conn(),
        "user",
        "unsubscribe",
        serde_json::json!({ "sender": sender, "status": status }),
    )
    .map_err(|e| e.to_string())?;
    Ok(status)
}

#[derive(serde::Serialize)]
pub struct BulkArchiveResult {
    pub archived: usize,
    pub undo: Vec<mail_actions::ArchivedRef>,
}

/// Bulk archive of one sender's inbox mail (FR-14). The driver performs the
/// server MOVEs; the undo references live in the frontend session.
#[tauri::command]
pub async fn bulk_archive_sender(
    state: State<'_, AppState>,
    sender: String,
) -> CmdResult<BulkArchiveResult> {
    let (tx, rx) = mpsc::channel();
    state
        .inner()
        .send(DriverMsg::BulkArchive { sender, reply: tx });
    let undo = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(120))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(BulkArchiveResult {
        archived: undo.len(),
        undo,
    })
}

/// Session Undo (FR-14 AC: bulk archive is reversible within the session):
/// MOVEs every reference back on the server; sync re-fetches the messages.
#[tauri::command]
pub async fn undo_bulk_archive(
    state: State<'_, AppState>,
    undo: Vec<mail_actions::ArchivedRef>,
) -> CmdResult<()> {
    // A server without UIDPLUS reports no COPYUID (new_uid 0): there is no
    // addressable location to restore from — skip rather than MOVE uid 0.
    for r in undo.into_iter().filter(|r| r.new_uid > 0) {
        state
            .inner()
            .send(DriverMsg::Action(MessageAction::RestoreArchived {
                account_id: r.account_id,
                from_folder: r.from_folder,
                uid: r.new_uid,
                to_folder: r.to_folder,
            }));
    }
    Ok(())
}

/// Marks a message opened (reader latch; FR-14 counters) — the reader calls
/// this after `message_body` when it renders text-only bodies too.
#[tauri::command]
pub async fn message_opened(state: State<'_, AppState>, message_id: i64) -> CmdResult<()> {
    let st = lock_store(&state)?;
    queries::mark_opened(st.conn(), message_id).map_err(|e| e.to_string())?;
    Ok(())
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
#[allow(clippy::too_many_arguments)]
pub async fn add_account(
    state: State<'_, AppState>,
    provider_id: String,
    email: String,
    password: String,
    imap_host: Option<String>,
    imap_port: Option<u16>,
    smtp_host: Option<String>,
    smtp_port: Option<u16>,
) -> CmdResult<queries::AccountSummary> {
    let email_clean = email.trim().to_string();
    if !email_clean.contains('@') {
        return Err("enter a valid email address".into());
    }
    let normalized_id = if provider_id == "imap" {
        "generic"
    } else {
        provider_id.as_str()
    };
    let preset = providers::ALL
        .iter()
        .find(|p| p.id == normalized_id)
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
    // Manual accounts must be able to send (FR-18): require their SMTP host.
    let smtp_host = smtp_host.filter(|h| !h.trim().is_empty());
    if preset.id == "generic" && smtp_host.is_none() {
        return Err("manual accounts need the SMTP server host".into());
    }
    // The SQLite accounts.provider CHECK constraint allows ('gmail','outlook','icloud','zoho','yahoo','imap').
    let db_provider = if preset.id == "generic" {
        "imap"
    } else {
        preset.id
    };

    let account_id = {
        let st = lock_store(&state)?;
        // Insert first (the Credential Manager target is id-keyed), store the
        // secret, then link the row; a secret failure removes the half-made row.
        let id = dao::add_account(
            st.conn(),
            &dao::NewAccount {
                provider: db_provider,
                email: &email_clean,
                display_name: None,
                auth_type: "app_password",
                credential_ref: "",
                imap_host: imap_host.as_deref().filter(|h| !h.trim().is_empty()),
                imap_port,
                smtp_host: smtp_host.as_deref(),
                smtp_port,
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

// ------------------------------------------------------------------ drafts

/// Creates or autosaves a draft (FR-17). `draft_id` present → update (only
/// `draft`-status rows are editable); absent → create.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn save_draft(
    state: State<'_, AppState>,
    account_id: i64,
    to: Vec<dao::Addr>,
    cc: Vec<dao::Addr>,
    subject: String,
    body_text: String,
    in_reply_to: Option<String>,
    thread_id: Option<i64>,
    draft_id: Option<i64>,
) -> CmdResult<i64> {
    let st = lock_store(&state)?;
    match draft_id {
        Some(id) => {
            dao::update_draft(st.conn(), id, &to, &cc, &subject, &body_text)
                .map_err(|e| e.to_string())?;
            Ok(id)
        }
        None => dao::add_draft(
            st.conn(),
            &dao::NewDraft {
                account_id,
                thread_id,
                to: &to,
                cc: &cc,
                subject: &subject,
                body_text: &body_text,
                in_reply_to: in_reply_to.as_deref(),
            },
        )
        .map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub async fn list_drafts(
    state: State<'_, AppState>,
    only_pending: Option<bool>,
) -> CmdResult<Vec<dao::DraftRow>> {
    let st = lock_store(&state)?;
    dao::list_drafts(st.conn(), only_pending.unwrap_or(false)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_draft(state: State<'_, AppState>, draft_id: i64) -> CmdResult<()> {
    let st = lock_store(&state)?;
    dao::delete_draft(st.conn(), draft_id).map_err(|e| e.to_string())
}

/// Queues a draft for sending (FR-18). The driver performs SMTP + Sent
/// APPEND; the reply reports the outcome, and a failure keeps the row in the
/// outbox with its reason.
#[tauri::command]
pub async fn send_draft(state: State<'_, AppState>, draft_id: i64) -> CmdResult<()> {
    {
        let st = lock_store(&state)?;
        dao::set_draft_status(st.conn(), draft_id, "queued", None).map_err(|e| e.to_string())?;
    }
    let (tx, rx) = mpsc::channel();
    state.inner().send(DriverMsg::SendDraft {
        draft_id,
        reply: tx,
    });
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(120))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?
    .map_err(|e| {
        crate::platform::applog::record("error", format!("draft {draft_id} send failed: {e}"));
        e
    })
}

/// Opens the OS mail composer for a mailto: unsubscribe fallback (FR-14).
/// Only the mailto scheme is accepted.
#[tauri::command]
pub async fn open_mailto(app: AppHandle, mailto: String) -> CmdResult<()> {
    if !mailto.to_lowercase().starts_with("mailto:") {
        return Err("only mailto: links are accepted".into());
    }
    crate::platform::shell::open_url(&app, &mailto).map_err(|e| e.to_string())
}

/// Opens an external `https://` or `mailto:` URL in the OS default browser or
/// mail client via `ShellExecuteW` (fixes WebView2 blocked target="_blank"
/// redirects for provider app-password pages, API key dashboards, and docs).
#[tauri::command]
pub async fn open_external_url(app: AppHandle, url: String) -> CmdResult<()> {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("mailto:") {
        return Err("only https:// and mailto: URLs are accepted".into());
    }
    crate::platform::shell::open_url(&app, trimmed).map_err(|e| e.to_string())
}

fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Spawns a standalone floating composer window initialized with the current
/// draft state so users can write or reply in a separate window while browsing
/// other threads in the main window.
#[tauri::command]
pub async fn open_composer_window(app: AppHandle, state_json: String) -> CmdResult<()> {
    let label = format!(
        "composer-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(1)
    );
    let encoded = percent_encode_query(&state_json);
    let url = format!("index.html?window=composer&state={encoded}");
    tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::App(url.into()))
        .title("Compose — cracktivity")
        .inner_size(740.0, 620.0)
        .min_inner_size(520.0, 440.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- settings

/// The whole settings surface for the UI (Phase 5): every key the app owns.
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let st = lock_store(&state)?;
    let mut out = serde_json::Map::new();
    for key in SETTINGS_KEYS {
        if let Some(v) = dao::get_setting(st.conn(), key).ok().flatten() {
            out.insert((*key).to_string(), v);
        }
    }
    Ok(serde_json::Value::Object(out))
}

#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> CmdResult<()> {
    if !SETTINGS_KEYS.contains(&key.as_str()) {
        return Err(format!("unknown setting {key}"));
    }
    set_setting_value(&state, &key, &value)
}

/// Keys the UI may write (allowlist: no secrets ever live in `settings`).
const SETTINGS_KEYS: [&str; 19] = [
    "ai.enabled",
    "ai.tier_override",
    "ai.engine",
    "ai.byo_model",
    "ai.model_override",
    "ai.providers_config",
    "ai.writing_engine",
    "ai.writing_model",
    "ai.fallback_engine",
    "mcp.allow_send",
    "mcp.rate_limit_per_hour",
    "search.also_server",
    "followup.days",
    "updates.check_enabled",
    "privacy.blocklist_updates",
    "onboarding.completed",
    "appearance.keymap",
    "appearance.theme",
    "panel.hotkey",
];

/// Sets the panel hotkey (FR-28 configurability): validates, swaps the
/// registration live, then persists. An empty value resets to the default.
#[tauri::command]
pub async fn set_panel_hotkey(
    state: State<'_, AppState>,
    app: AppHandle,
    hotkey: String,
) -> CmdResult<()> {
    let wanted = if hotkey.trim().is_empty() {
        None
    } else {
        Some(hotkey.trim())
    };
    // Validate before touching the live registration.
    crate::platform::shell::validate_hotkey(
        wanted.unwrap_or(crate::platform::shell::DEFAULT_HOTKEY),
    )?;
    let old = get_setting_value(&state, "panel.hotkey")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| crate::platform::shell::DEFAULT_HOTKEY.to_string());
    crate::platform::shell::unregister_hotkey(&app, &old);
    if let Err(e) = crate::platform::shell::register_hotkey(&app, wanted) {
        // Restore the previous hotkey so the panel is never left dead.
        let _ = crate::platform::shell::register_hotkey(&app, Some(&old));
        return Err(e);
    }
    set_setting_value(
        &state,
        "panel.hotkey",
        &serde_json::json!(wanted.unwrap_or(crate::platform::shell::DEFAULT_HOTKEY)),
    )?;
    Ok(())
}

// ----------------------------------------------------------------------- AI

/// Streaming events for AI tasks (FR-21/22: incremental output).
#[derive(serde::Serialize, Clone)]
#[serde(tag = "type")]
pub enum AiEvent {
    Delta { text: String },
    Notice { text: String },
    Done,
}

/// Settings snapshot for AI commands.
pub(crate) struct AiSettings {
    pub(crate) enabled: bool,
    pub(crate) engine_id: String,
    pub(crate) model: Option<String>,
    pub(crate) writing_engine_id: Option<String>,
    pub(crate) writing_model: Option<String>,
    pub(crate) fallback_engine_id: String,
    pub(crate) tier_override: Option<String>,
    pub(crate) providers_config: serde_json::Value,
}

fn default_provider_base_url(id: &str) -> &'static str {
    match id {
        "openai" => "https://api.openai.com",
        "anthropic" => "https://api.anthropic.com",
        "xai" => "https://api.x.ai",
        "openrouter" => "https://openrouter.ai/api/v1",
        "mistral" => "https://api.mistral.ai/v1",
        "groq" => "https://api.groq.com/openai/v1",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta/openai",
        "huggingface" => "https://router.huggingface.co/v1",
        _ => "",
    }
}

fn default_provider_model(id: &str) -> &'static str {
    match id {
        "local" => "local",
        "openai" => "gpt-4o-mini",
        "anthropic" => "claude-3-5-haiku-latest",
        "xai" => "grok-2-latest",
        "openrouter" => "openai/gpt-4o-mini",
        "mistral" => "mistral-small-latest",
        "groq" => "llama-3.3-70b-versatile",
        "gemini" => "gemini-2.5-flash",
        "huggingface" => "Qwen/Qwen2.5-72B-Instruct",
        "chatgpt" => "gpt-4.1-mini",
        _ => "gpt-4o-mini",
    }
}

pub(crate) fn ai_settings_from_conn(conn: &rusqlite::Connection) -> AiSettings {
    let get = |key: &str| dao::get_setting(conn, key).ok().flatten();
    AiSettings {
        enabled: get("ai.enabled").and_then(|v| v.as_bool()).unwrap_or(false),
        engine_id: get("ai.engine")
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "local".to_string()),
        model: get("ai.byo_model")
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| !s.trim().is_empty()),
        writing_engine_id: get("ai.writing_engine")
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| !s.trim().is_empty() && s != "same"),
        writing_model: get("ai.writing_model")
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| !s.trim().is_empty()),
        fallback_engine_id: get("ai.fallback_engine")
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "local".to_string()),
        tier_override: get("ai.tier_override").and_then(|v| v.as_str().map(String::from)),
        providers_config: get("ai.providers_config").unwrap_or_else(|| serde_json::json!({})),
    }
}

fn ai_settings_of(state: &State<'_, AppState>) -> AiSettings {
    state
        .inner()
        .store
        .lock()
        .ok()
        .map(|st| ai_settings_from_conn(st.conn()))
        .unwrap_or(AiSettings {
            enabled: false,
            engine_id: "local".into(),
            model: None,
            writing_engine_id: None,
            writing_model: None,
            fallback_engine_id: "local".into(),
            tier_override: None,
            providers_config: serde_json::json!({}),
        })
}

/// AI status for Settings + onboarding (FR-20/23/25 surface).
#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let s = ai_settings_of(&state);
    let mut ai = state.inner().ai.lock().map_err(|_| "ai poisoned")?;
    let tier = ai.effective_tier(s.tier_override.as_deref());
    let hw = crate::ai::models::detect();
    let key_set_id = |id: &str| {
        crate::ai::byo_key_target_for_id(id)
            .map(|t| crate::platform::providers::get_account_secret(&t).is_ok())
            .unwrap_or(false)
    };
    let mut byo_map = serde_json::Map::new();
    for id in [
        "openai",
        "anthropic",
        "xai",
        "openrouter",
        "mistral",
        "groq",
        "gemini",
        "huggingface",
    ] {
        byo_map.insert(id.to_string(), serde_json::Value::Bool(key_set_id(id)));
    }
    if let Some(obj) = s.providers_config.as_object() {
        for k in obj.keys() {
            if !byo_map.contains_key(k) {
                byo_map.insert(k.clone(), serde_json::Value::Bool(key_set_id(k)));
            }
        }
    }
    Ok(serde_json::json!({
        "enabled": s.enabled,
        "engine": s.engine_id,
        "chatgpt_email": crate::ai::siwc::load_stored().map(|c| c.email),
        "tier": tier.as_str(),
        "tier_reason": format!(
            "{} GB RAM, {} cores → {} tier",
            hw.total_ram_gb, hw.logical_cores, tier.as_str()
        ),
        "tier_is_override": s.tier_override.is_some(),
        "model_downloaded": ai.model_downloaded(tier),
        "sidecar_running": ai
            .sidecar
            .as_mut()
            .map(|s| s.is_alive())
            .unwrap_or(false),
        "byo_configured": serde_json::Value::Object(byo_map),
        "model_disk_bytes": crate::ai::manager::disk_usage(&ai.model_dir),
    }))
}

/// Master switch (FR-25): off kills every model process immediately.
#[tauri::command]
pub async fn ai_set_enabled(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    set_setting_value(&state, "ai.enabled", &serde_json::json!(enabled))?;
    if !enabled {
        let mut ai = state.inner().ai.lock().map_err(|_| "ai poisoned")?;
        ai.shutdown();
    }
    Ok(())
}

/// Deletes the tier's model file, reclaiming disk (FR-20 AC).
#[tauri::command]
pub async fn ai_delete_model(state: State<'_, AppState>) -> CmdResult<()> {
    let s = ai_settings_of(&state);
    let mut ai = state.inner().ai.lock().map_err(|_| "ai poisoned")?;
    ai.shutdown();
    let tier = ai.effective_tier(s.tier_override.as_deref());
    crate::ai::manager::delete(&ai.model_dir, crate::ai::models::write_model_for(tier))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Model download with progress events (FR-20 AC: resumable, verified).
#[tauri::command]
pub async fn ai_download_model(
    state: State<'_, AppState>,
    on_event: tauri::ipc::Channel<AiEvent>,
) -> CmdResult<()> {
    let s = ai_settings_of(&state);
    let ai = Arc::clone(state.inner().ai_ref());
    let channel = on_event.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Lock only to resolve tier + model dir; the download itself runs
        // lock-free so ai_status / the FR-25 master switch stay responsive
        // during the multi-minute fetch.
        let (model_dir, pin) = {
            let ai = ai.lock().map_err(|_| "ai poisoned")?;
            let tier = ai.effective_tier(s.tier_override.as_deref());
            (
                ai.model_dir.clone(),
                crate::ai::models::write_model_for(tier),
            )
        };
        crate::ai::manager::download(&model_dir, pin, &mut |received, total| {
            let _ = channel.send(AiEvent::Notice {
                text: format!("{} / {} bytes", received, total),
            });
        })
        .map_err(|e| e.to_string())?;
        let _ = on_event.send(AiEvent::Done);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}

/// Stores (or clears) a BYO API key in Credential Manager (FR-24).
#[tauri::command]
pub async fn ai_set_key(
    _state: State<'_, AppState>,
    kind: String,
    key: Option<String>,
) -> CmdResult<()> {
    let target = crate::ai::byo_key_target_for_id(&kind).ok_or("unknown engine kind")?;
    match key {
        Some(k) if !k.trim().is_empty() => {
            crate::platform::providers::set_account_secret(&target, k.trim())
        }
        _ => crate::platform::providers::delete_account_secret(&target),
    }
    .map_err(|e| e.to_string())
}

/// Validates an AI provider endpoint URL:
/// - Requires `https://` with a non-empty host, OR
/// - Allows `http://` ONLY when the parsed host is strictly a loopback address
///   (`localhost`, `127.0.0.1`, `::1`, or `[::1]`). Rejects prefix tricks like
///   `http://localhost.attacker.example`.
pub(crate) fn validate_ai_endpoint_url(raw: &str) -> Result<(Url, bool), String> {
    let parsed = Url::parse(raw.trim()).map_err(|e| format!("invalid endpoint URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "endpoint URL must include a host".to_string())?;
    let is_loopback = matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1" | "[::1]"
    );
    match parsed.scheme() {
        "https" => Ok((parsed, false)),
        "http" if is_loopback => Ok((parsed, true)),
        "http" => Err(
            "Base URL must use https:// (or http://localhost / http://127.0.0.1 for local endpoints)"
                .into(),
        ),
        other => Err(format!("unsupported URL scheme '{other}': use https://")),
    }
}

/// Queries a provider's `/models` endpoint to test connectivity and return
/// discovered model IDs (Jan-style Refresh Models & Test Connection).
#[tauri::command]
pub async fn ai_fetch_models(
    kind: String,
    base_url: String,
    api_key: Option<String>,
    api_format: Option<String>,
) -> CmdResult<Vec<String>> {
    let trimmed_url = base_url.trim().to_string();
    validate_ai_endpoint_url(&trimmed_url)?;
    let resolved_key = match api_key.filter(|k| !k.trim().is_empty()) {
        Some(k) => k,
        None => crate::ai::byo_key_target_for_id(&kind)
            .and_then(|t| crate::platform::providers::get_account_secret(&t).ok())
            .unwrap_or_default(),
    };
    let is_anthropic = api_format.as_deref() == Some("anthropic") || kind == "anthropic";
    tauri::async_runtime::spawn_blocking(move || {
        crate::ai::chat::fetch_provider_models(&trimmed_url, &resolved_key, is_anthropic)
            .map_err(|e| e.0)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs the Sign in with ChatGPT browser flow (D-047): dynamic client
/// registration on first use, plan-usage scopes, loopback callback. Blocking
/// and user-paced, so it runs on a worker thread and stores the connection
/// in Credential Manager. Returns the connected account email.
#[tauri::command]
pub async fn ai_chatgpt_connect(state: State<'_, AppState>) -> CmdResult<String> {
    let _ = state;
    let email = tauri::async_runtime::spawn_blocking(crate::ai::siwc::connect)
        .await
        .map_err(|e| e.to_string())?
        .map(|c| c.email)
        .map_err(|e| e.to_string())?;
    Ok(email)
}

/// Removes the stored ChatGPT connection (tokens + registration).
#[tauri::command]
pub async fn ai_chatgpt_disconnect() -> CmdResult<()> {
    crate::ai::siwc::disconnect();
    Ok(())
}

/// Builds the plain-text transcript for a thread (summarize + draft input).
fn thread_transcript(store: &Store, thrid: &str) -> Result<String, String> {
    let messages = queries::thread_messages(store.conn(), thrid).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for m in messages {
        let body = queries::message_body(store.conn(), m.id)
            .ok()
            .flatten()
            .map(|b| b.text.or(b.html).unwrap_or_default())
            .unwrap_or_default();
        let from = m
            .from
            .first()
            .map(|a| a.email.as_str())
            .unwrap_or("unknown");
        out.push_str(&format!("From {from}:\n{body}\n\n"));
    }
    Ok(out)
}

pub(crate) fn build_byo_provider_engine(
    settings: &AiSettings,
    engine_id: &str,
    override_model: Option<&str>,
) -> Result<Box<dyn crate::ai::ChatEngine>, String> {
    let provider_cfg = &settings.providers_config[engine_id];
    if provider_cfg["enabled"].as_bool() == Some(false) {
        return Err(format!(
            "provider '{engine_id}' is disabled in Settings → AI"
        ));
    }
    let configured_base = provider_cfg["baseUrl"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_provider_base_url(engine_id));
    if configured_base.is_empty() {
        return Err(format!(
            "No base URL configured for provider '{engine_id}' (Settings → AI)"
        ));
    }
    let (_parsed_url, is_local_http) = validate_ai_endpoint_url(configured_base)?;
    let target = crate::ai::byo_key_target_for_id(engine_id).ok_or("unknown engine kind")?;
    let key = match crate::platform::providers::get_account_secret(&target) {
        Ok(k) => k,
        Err(_) if is_local_http => String::new(),
        Err(_) => return Err(format!("no {engine_id} API key configured (Settings → AI)")),
    };
    let configured_default_model = provider_cfg["defaultModel"]
        .as_str()
        .or_else(|| {
            provider_cfg["models"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str().or_else(|| v["id"].as_str()))
        })
        .unwrap_or_else(|| default_provider_model(engine_id));
    let model = override_model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(configured_default_model);
    let api_format = provider_cfg["apiFormat"]
        .as_str()
        .unwrap_or(if engine_id == "anthropic" {
            "anthropic"
        } else {
            "openai"
        });
    if api_format == "anthropic" {
        if configured_base == "https://api.anthropic.com" {
            Ok(Box::new(crate::ai::chat::Anthropic::new(&key, model)))
        } else {
            Ok(Box::new(crate::ai::chat::Anthropic::with_base_url(
                configured_base,
                &key,
                model,
            )))
        }
    } else if engine_id == "openai" && configured_base == "https://api.openai.com" {
        Ok(Box::new(crate::ai::chat::OpenAiCompat::openai(&key, model)))
    } else if engine_id == "xai" && configured_base == "https://api.x.ai" {
        Ok(Box::new(crate::ai::chat::OpenAiCompat::xai(&key, model)))
    } else {
        Ok(Box::new(crate::ai::chat::OpenAiCompat::custom(
            configured_base,
            &key,
            model,
        )))
    }
}

fn build_provider_engine(
    ai: &Arc<Mutex<crate::ai::AiState>>,
    settings: &AiSettings,
    engine_id: &str,
    override_model: Option<&str>,
) -> Result<Box<dyn crate::ai::ChatEngine>, String> {
    if engine_id == "local" {
        let mut ai = ai.lock().map_err(|_| "ai poisoned")?;
        let tier = ai.effective_tier(settings.tier_override.as_deref());
        let engine = ai.local_engine(tier).map_err(|e| e.to_string())?;
        return Ok(Box::new(engine));
    }
    if engine_id == "chatgpt" {
        let stored =
            crate::ai::siwc::load_stored().ok_or("not signed in to ChatGPT (Settings, AI)")?;
        let access = if crate::ai::siwc::needs_refresh(&stored) {
            crate::ai::siwc::refresh(&stored).map_err(|e| e.to_string())?;
            crate::ai::siwc::load_stored()
                .ok_or("ChatGPT connection vanished")?
                .access
        } else {
            stored.access
        };
        let model = override_model
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(default_provider_model("chatgpt"));
        return Ok(Box::new(crate::ai::chat::Responses::new(access, model)));
    }
    build_byo_provider_engine(settings, engine_id, override_model)
}

/// Runs the streaming task with the FR-24 fallback chain: chosen engine →
/// configured fallback engine (default: local model) with a visible notice.
fn run_ai_task(
    ai: &Arc<Mutex<crate::ai::AiState>>,
    settings: &AiSettings,
    is_writing_task: bool,
    passes: &[Vec<crate::ai::Msg>],
    reduce: bool,
    on_event: &tauri::ipc::Channel<AiEvent>,
) -> Result<String, String> {
    if !settings.enabled {
        return Err("AI is disabled".into());
    }

    fn run_passes(
        engine: &dyn crate::ai::ChatEngine,
        passes: &[Vec<crate::ai::Msg>],
        reduce: bool,
        on_event: &tauri::ipc::Channel<AiEvent>,
    ) -> Result<String, String> {
        let mut parts = Vec::new();
        for pass in passes {
            parts.push(
                engine
                    .stream_chat(pass, &mut |d| {
                        let _ = on_event.send(AiEvent::Delta {
                            text: d.to_string(),
                        });
                    })
                    .map_err(|e| e.to_string())?,
            );
        }
        if reduce && parts.len() > 1 {
            engine
                .stream_chat(
                    &crate::ai::prompts::summarize_reduce_messages(&parts),
                    &mut |d| {
                        let _ = on_event.send(AiEvent::Delta {
                            text: d.to_string(),
                        });
                    },
                )
                .map_err(|e| e.to_string())
        } else {
            Ok(parts.join("\n\n"))
        }
    }

    let chosen_engine_id = if is_writing_task {
        settings
            .writing_engine_id
            .as_deref()
            .unwrap_or(&settings.engine_id)
    } else {
        &settings.engine_id
    };
    let chosen_model = if is_writing_task {
        settings
            .writing_model
            .as_deref()
            .or(settings.model.as_deref())
    } else {
        settings.model.as_deref()
    };

    if chosen_engine_id == "local" {
        let engine = build_provider_engine(ai, settings, "local", None)?;
        return run_passes(engine.as_ref(), passes, reduce, on_event);
    }

    let primary_res = build_provider_engine(ai, settings, chosen_engine_id, chosen_model)
        .and_then(|engine| run_passes(engine.as_ref(), passes, reduce, on_event));

    match primary_res {
        Ok(text) => Ok(text),
        Err(byo_err) => {
            let fb_id = settings.fallback_engine_id.as_str();
            if fb_id == "none" || fb_id == chosen_engine_id {
                return Err(byo_err);
            }
            let fb_engine = build_provider_engine(ai, settings, fb_id, None)
                .map_err(|fb_err| format!("{byo_err}; fallback ({fb_id}) unavailable: {fb_err}"))?;
            let fb_label = if fb_id == "local" {
                "the local model".to_string()
            } else {
                format!("fallback provider '{fb_id}'")
            };
            let _ = on_event.send(AiEvent::Notice {
                text: format!("{chosen_engine_id} API unreachable — used {fb_label} instead"),
            });
            run_passes(fb_engine.as_ref(), passes, reduce, on_event)
        }
    }
}

/// Summarizes a thread on-device (FR-21): streams incrementally, works
/// offline, respects the context budget (chunked map-reduce).
#[tauri::command]
pub async fn ai_summarize(
    state: State<'_, AppState>,
    thread_id: String,
    on_event: tauri::ipc::Channel<AiEvent>,
) -> CmdResult<()> {
    let (transcript, settings) = {
        let st = lock_store(&state)?;
        let transcript = thread_transcript(&st, &thread_id)?;
        (transcript, ai_settings_of(&state))
    };
    let ai = Arc::clone(state.inner().ai_ref());
    let passes = crate::ai::prompts::summarize_messages(&transcript);
    tauri::async_runtime::spawn_blocking(move || {
        run_ai_task(&ai, &settings, false, &passes, true, &on_event)?;
        let _ = on_event.send(AiEvent::Done);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}

/// Drafts a reply from thread context + instruction (FR-22): streams into
/// the composer as editable text; nothing is ever sent automatically.
#[tauri::command]
pub async fn ai_draft_reply(
    state: State<'_, AppState>,
    thread_id: String,
    instruction: String,
    on_event: tauri::ipc::Channel<AiEvent>,
) -> CmdResult<()> {
    let (transcript, account_email, settings) = {
        let st = lock_store(&state)?;
        let transcript = thread_transcript(&st, &thread_id)?;
        let account_email = queries::thread_messages(st.conn(), &thread_id)
            .ok()
            .and_then(|m| m.first().map(|m| m.account_id))
            .and_then(|id| {
                st.conn()
                    .query_row("SELECT email FROM accounts WHERE id = ?1", [id], |r| {
                        r.get::<_, String>(0)
                    })
                    .ok()
            })
            .unwrap_or_default();
        (transcript, account_email, ai_settings_of(&state))
    };
    let ai = Arc::clone(state.inner().ai_ref());
    let msgs = crate::ai::prompts::draft_messages(&transcript, &instruction, &account_email);
    tauri::async_runtime::spawn_blocking(move || {
        run_ai_task(&ai, &settings, true, &[msgs], false, &on_event)?;
        let _ = on_event.send(AiEvent::Done);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}

// --------------------------------------------------------------------- MCP

/// One-click registration into an MCP client's config (FR-26): .bak backup,
/// unknown keys preserved. The client needs a restart to pick it up.
#[tauri::command]
pub async fn mcp_register(state: State<'_, AppState>, client: String) -> CmdResult<String> {
    let client = crate::mcp::register::Client::parse(&client)
        .ok_or_else(|| format!("unknown client {client}"))?;
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string();
    let path = crate::mcp::register::register(client, &exe)?;
    let st = lock_store(&state)?;
    dao::audit(
        st.conn(),
        "user",
        "mcp_register",
        serde_json::json!({ "client": client.label(), "path": path }),
    )
    .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// One audit trail row: (id, at, actor, action, detail json).
pub type AuditTailRow = (i64, i64, String, String, String);

/// Audit tail for Settings → MCP (FR-27 AC).
#[tauri::command]
pub async fn mcp_audit_tail(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> CmdResult<Vec<AuditTailRow>> {
    let st = lock_store(&state)?;
    dao::audit_tail(st.conn(), limit.unwrap_or(100)).map_err(|e| e.to_string())
}

// -------------------------------------------------------------- diagnostics

/// Session host list (FR-32 evidence): every remote host this process
/// contacted, in-process only (D-015: never leaves the machine).
#[tauri::command]
pub async fn diagnostics_hosts(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let _ = state;
    Ok(crate::platform::netlog::hosts())
}

/// Storage usage (Phase-5 diagnostics): DB + models, in bytes.
#[derive(serde::Serialize)]
pub struct StorageUsage {
    pub db_bytes: u64,
    pub models_bytes: u64,
}

#[tauri::command]
pub async fn storage_usage(state: State<'_, AppState>) -> CmdResult<StorageUsage> {
    let dir = state.inner().data_dir.clone();
    let db = std::fs::metadata(dir.join("profile").join(crate::store::DB_FILE))
        .map(|m| m.len())
        .unwrap_or(0);
    let models = {
        let ai = state.inner().ai.lock().map_err(|_| "ai poisoned")?;
        crate::ai::manager::disk_usage(&ai.model_dir)
    };
    Ok(StorageUsage {
        db_bytes: db,
        models_bytes: models,
    })
}

/// One log-buffer row for the Diagnostics viewer (Phase 5, D-042).
#[derive(serde::Serialize)]
pub struct LogLine {
    pub ts: u64,
    pub level: &'static str,
    pub msg: String,
}

/// The in-app log ring buffer, oldest first (in-memory only, D-042).
#[tauri::command]
pub async fn diagnostics_logs() -> CmdResult<Vec<LogLine>> {
    Ok(crate::platform::applog::tail()
        .into_iter()
        .map(|(ts, level, msg)| LogLine { ts, level, msg })
        .collect())
}

/// Clears the in-app log buffer (user action from the Diagnostics viewer).
#[tauri::command]
pub async fn diagnostics_clear_logs() -> CmdResult<()> {
    crate::platform::applog::clear();
    Ok(())
}

/// Cache prune (Phase 5): deletes partial model downloads (`.part` — the
/// resumable prefixes of interrupted downloads, spec §11). Returns bytes
/// reclaimed. The encrypted mail DB is the offline cache and is never
/// pruned from the UI (D-043).
#[tauri::command]
pub async fn prune_cache(state: State<'_, AppState>) -> CmdResult<u64> {
    let dir = {
        let ai = state.inner().ai.lock().map_err(|_| "ai poisoned")?;
        ai.model_dir.clone()
    };
    let freed = crate::ai::manager::prune_partials(&dir);
    if freed > 0 {
        crate::platform::applog::record(
            "info",
            format!("pruned {freed} bytes of partial downloads"),
        );
    }
    Ok(freed)
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
            smtp_host: None,
            smtp_port: None,
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

/// Removes any synthetic `--e2e-seed` demo account (`credential_ref = 'cracktivity.e2e.seed'`)
/// and its cascaded folders/messages/threads so normal production runs never show
/// stock demo data even if `--e2e-seed` was previously run against the profile.
pub fn purge_demo(store: &Store) -> Result<usize, crate::store::StoreError> {
    let conn = store.conn();
    let deleted = conn.execute(
        "DELETE FROM accounts WHERE credential_ref = 'cracktivity.e2e.seed'",
        [],
    )?;
    Ok(deleted)
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
        assert!(
            !rendered.html.contains(" src="),
            "no live remote src: {}",
            rendered.html
        );
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

    #[test]
    fn purge_demo_removes_seeded_account_and_messages() {
        let (_dir, state) = seeded_state();
        let st = state.store.lock().unwrap();
        assert_eq!(queries::list_accounts(st.conn()).unwrap().len(), 1);
        let deleted = purge_demo(&st).expect("purge_demo must succeed");
        assert_eq!(deleted, 1);
        assert!(
            queries::list_accounts(st.conn()).unwrap().is_empty(),
            "demo account and cascaded messages must be removed on normal startup"
        );
        let all =
            queries::thread_list(st.conn(), Scope::Unified, ImportanceFilter::All, 50, 0).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn validate_ai_endpoint_url_rejects_localhost_prefix_spoofing() {
        assert!(validate_ai_endpoint_url("http://localhost.attacker.example/v1").is_err());
        assert!(validate_ai_endpoint_url("http://127.0.0.1.attacker.example/v1").is_err());
        assert!(validate_ai_endpoint_url("http://remote.example.com/v1").is_err());
        assert_eq!(
            validate_ai_endpoint_url("http://localhost:11434/v1")
                .unwrap()
                .1,
            true
        );
        assert_eq!(
            validate_ai_endpoint_url("http://127.0.0.1:1234/v1")
                .unwrap()
                .1,
            true
        );
        assert_eq!(
            validate_ai_endpoint_url("https://api.openai.com/v1")
                .unwrap()
                .1,
            false
        );
    }

    #[test]
    fn generic_account_maps_to_imap_schema_and_resolves_custom_endpoints() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        use tauri::webview::InvokeRequest;
        use tauri::Manager;

        let (_dir, state) = seeded_state();
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![super::add_account])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        app.manage(state);
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let res = tauri::test::get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "add_account".into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(serde_json::json!({
                    "providerId": "generic",
                    "email": "iqbalmaarif@invosa.com",
                    "password": "test-app-password",
                    "imapHost": "imap.invosa.com",
                    "imapPort": 993,
                    "smtpHost": "smtp.invosa.com",
                    "smtpPort": 587
                })),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .expect("add_account generic must succeed against SQLCipher CHECK constraint")
        .deserialize::<serde_json::Value>()
        .unwrap();

        let acc_id = res["id"].as_i64().unwrap();
        assert_eq!(res["provider"].as_str(), Some("imap"));
        assert_eq!(res["email"].as_str(), Some("iqbalmaarif@invosa.com"));

        let app_state = app.state::<AppState>();
        let st = app_state.store.lock().unwrap();
        let smtp_ep = crate::mail::smtp::smtp_endpoint_of(st.conn(), acc_id).unwrap();
        assert_eq!(smtp_ep, ("smtp.invosa.com".to_string(), 587));
        let _ = providers::delete_account_secret(&providers::app_password_target(acc_id));
    }
}
