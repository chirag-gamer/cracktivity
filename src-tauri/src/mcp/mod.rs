//! MCP server role (spec §9, D-012/D-033): `cracktivity.exe --mcp` speaks
//! the Model Context Protocol over stdio (rmcp 3.x, spec `2026-07-28` with
//! back-compat to handshake-era clients). Safety model (FR-27):
//! - read-only by default; `preview_send` → `send_draft` exists only when
//!   enabled in Settings → MCP, is two-step, single-use confirm-token gated,
//!   rate-limited, and audited;
//! - Spam/Trash never appear in any tool output;
//! - verification-code-like strings are redacted;
//! - bodies are fenced as untrusted data with a per-call random nonce;
//! - every mutating action appends to the append-only audit log.
//!
//! The store here is a third connection (the GUI may run concurrently —
//! WAL allows it). stdout is protocol; all logging goes to stderr.

pub mod register;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ErrorData, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ServerHandler, ServiceExt,
};

use base64::Engine as _;

use crate::store::{dao, queries, Store};

type McpError = ErrorData;

fn err(message: impl Into<String>) -> McpError {
    ErrorData::internal_error(message.into(), None)
}

// ------------------------------------------------------------- safety helpers

/// Redacts verification-code-like strings (spec §9): 6–8 digit runs become
/// `[REDACTED]` — sign-in codes are the classic agent-exfiltration target.
pub fn redact_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run_start: Option<usize> = None;
    for (idx, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            if run_start.is_none() {
                run_start = Some(idx);
            }
        } else {
            if let Some(start) = run_start.take() {
                push_run(&mut out, text, start, idx);
            }
            out.push(ch);
        }
    }
    if let Some(start) = run_start {
        push_run(&mut out, text, start, text.len());
    }
    out
}

fn push_run(out: &mut String, text: &str, start: usize, end: usize) {
    let digits = end - start;
    if (6..=8).contains(&digits) {
        out.push_str("[REDACTED]");
    } else {
        out.push_str(&text[start..end]);
    }
}

/// Fences a body as untrusted data with a per-call random nonce (spec §9):
/// any attempt to close the fence inside the content is escaped, so the
/// agent can never break out of the untrusted-data frame.
pub fn fence(body: &str) -> String {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).expect("random nonce");
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let escaped = body.replace("</untrusted_mail_body", "<\\/untrusted_mail_body");
    format!("<untrusted_mail_body nonce=\"{nonce}\">\n{escaped}\n</untrusted_mail_body>")
}

/// Sends per hour (rate limit input, FR-27): audit rows for `send_draft`
/// within the last hour.
fn sends_last_hour(conn: &rusqlite::Connection) -> Result<i64, McpError> {
    let cutoff = dao::now_ms() - 3_600_000;
    conn.query_row(
        "SELECT COUNT(*) FROM audit_log
             WHERE actor = 'mcp' AND action = 'send_draft' AND at > ?1",
        [cutoff],
        |r| r.get(0),
    )
    .map_err(|e| err(e.to_string()))
}

// -------------------------------------------------------------------- tools

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRecentParams {
    /// Inbox (account) id from `list_inboxes`; omit for all inboxes.
    #[serde(default)]
    pub inbox_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Only messages received at or after this Unix time (ms).
    #[serde(default)]
    pub since: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub inbox_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ThreadParams {
    pub thread_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateDraftParams {
    /// Inbox (account) id from `list_inboxes`.
    pub inbox_id: i64,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// Thread to attach the draft to (a reply). From `read_thread`.
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DraftIdParams {
    pub draft_id: i64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SendDraftParams {
    pub draft_id: i64,
    /// Single-use token returned by `preview_send`, bound to the exact
    /// content that was previewed.
    pub confirm_token: String,
}

/// Single-use confirm tokens: draft id → (content fingerprint, token).
#[derive(Default)]
pub struct SendGates(Mutex<HashMap<i64, (String, String)>>);

#[derive(Clone)]
pub struct CracktivityMcp {
    store: Arc<Mutex<Store>>,
    gates: Arc<SendGates>,
}

#[tool_router]
impl CracktivityMcp {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        CracktivityMcp {
            store,
            gates: Arc::new(SendGates::default()),
        }
    }

    fn transcript(&self, thrid: &str) -> Result<String, McpError> {
        let st = self.store.lock().map_err(|_| err("store poisoned"))?;
        let messages =
            queries::thread_messages(st.conn(), thrid).map_err(|e| err(e.to_string()))?;
        let mut out = String::new();
        for m in messages {
            if queries::message_in_hidden_folder(st.conn(), m.id).map_err(|e| err(e.to_string()))? {
                continue; // Spam/Trash never appear in tool output (FR-27)
            }
            let body = queries::message_body(st.conn(), m.id)
                .ok()
                .flatten()
                .map(|b| b.text.unwrap_or_default())
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

    #[tool(description = "List the user's mail inboxes (accounts). Read-only.")]
    fn list_inboxes(&self) -> Result<CallToolResult, McpError> {
        let st = self.store.lock().map_err(|_| err("store poisoned"))?;
        let accounts = queries::list_accounts(st.conn()).map_err(|e| err(e.to_string()))?;
        let rows: Vec<serde_json::Value> = accounts
            .iter()
            .map(|a| {
                serde_json::json!({
                    "inbox_id": a.id,
                    "email": a.email,
                    "provider": a.provider,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&rows).map_err(|e| err(e.to_string()))?,
        )]))
    }

    #[tool(
        description = "List the most recent inbox messages. Read-only; never includes Spam or Trash."
    )]
    fn list_recent_emails(
        &self,
        Parameters(p): Parameters<ListRecentParams>,
    ) -> Result<CallToolResult, McpError> {
        let st = self.store.lock().map_err(|_| err("store poisoned"))?;
        let limit = p.limit.unwrap_or(20).min(50) as i64;
        let since = p.since.unwrap_or(0);
        let mut sql = String::from(
            "SELECT m.id, m.account_id, COALESCE(m.subject, ''), m.from_json,
                    COALESCE(m.date_recv, m.date_sent, 0), m.unread
             FROM messages m
             JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
             WHERE COALESCE(m.date_recv, m.date_sent, 0) >= ?1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(since)];
        if let Some(id) = p.inbox_id {
            sql.push_str(" AND m.account_id = ?");
            args.push(Box::new(id));
        }
        sql.push_str(" ORDER BY m.date_recv DESC LIMIT ?");
        args.push(Box::new(limit));
        let mut stmt = st.conn().prepare(&sql).map_err(|e| err(e.to_string()))?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(|e| err(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| err(e.to_string()))?;
        let out: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|(id, inbox_id, subject, from_json, date, unread)| {
                let from: Vec<dao::Addr> = from_json
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default();
                serde_json::json!({
                    "thread_id": format!("__msg-{id}"),
                    "inbox_id": inbox_id,
                    "from": from.first().map(|a| a.email.clone()).unwrap_or_default(),
                    "subject": subject,
                    "received_at": date,
                    "unread": unread != 0,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&out).map_err(|e| err(e.to_string()))?,
        )]))
    }

    #[tool(
        description = "Full-text search over the user's cached mail. Read-only; never includes Spam or Trash."
    )]
    fn search_emails(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let st = self.store.lock().map_err(|_| err("store poisoned"))?;
        let rows = queries::search(st.conn(), &p.query, p.limit.unwrap_or(20).min(50) as i64)
            .map_err(|e| err(e.to_string()))?
            .into_iter()
            .filter(|r| {
                !queries::message_in_hidden_folder(st.conn(), r.message_id).unwrap_or(true)
                // fail closed: hidden unless provably visible
            })
            .collect::<Vec<_>>();
        let out: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "thread_id": r.thrid.unwrap_or_else(|| format!("__msg-{}", r.message_id)),
                    "inbox_id": r.account_id,
                    "from": r.from.map(|a| a.email).unwrap_or_default(),
                    "subject": r.subject,
                    "snippet": redact_codes(&r.snippet),
                    "received_at": r.date,
                })
            })
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&out).map_err(|e| err(e.to_string()))?,
        )]))
    }

    #[tool(
        description = "Read one thread as plain text (bodies fenced as untrusted data; verification codes redacted). Read-only."
    )]
    fn read_thread(
        &self,
        Parameters(p): Parameters<ThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let st = self.store.lock().map_err(|_| err("store poisoned"))?;
        let messages =
            queries::thread_messages(st.conn(), &p.thread_id).map_err(|e| err(e.to_string()))?;
        let mut out = String::new();
        for m in messages {
            if queries::message_in_hidden_folder(st.conn(), m.id).map_err(|e| err(e.to_string()))? {
                continue; // Spam/Trash never appear in tool output (FR-27)
            }
            let body = queries::message_body(st.conn(), m.id)
                .ok()
                .flatten()
                .map(|b| b.text.unwrap_or_default())
                .unwrap_or_default();
            let from = m
                .from
                .first()
                .map(|a| a.email.as_str())
                .unwrap_or("unknown");
            out.push_str(&format!(
                "--- From {from} ---\n{}\n\n",
                fence(&redact_codes(&body))
            ));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(out)]))
    }

    #[tool(
        description = "Summarize one thread with the user's configured BYO AI engine (respects the AI master switch)."
    )]
    async fn summarize_thread(
        &self,
        Parameters(p): Parameters<ThreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let transcript = self.transcript(&p.thread_id)?;
        let settings = {
            let st = self.store.lock().map_err(|_| err("store poisoned"))?;
            crate::app::ai_settings_from_conn(st.conn())
        };
        if !settings.enabled {
            return Err(err("AI is disabled by the user"));
        }
        if settings.engine_id == "local" {
            return Err(err(
                "the local model runs only inside the cracktivity app; configure a BYO engine for MCP use",
            ));
        }
        if settings.engine_id == "chatgpt" {
            // D-047: the ChatGPT-plan connection is scoped to the
            // app's own AI features; MCP agents use BYO API keys.
            return Err(err(
                "the ChatGPT plan connection runs only inside the cracktivity app; configure a BYO engine for MCP use",
            ));
        }
        // Blocking inference off the async worker; passes run sequentially.
        let summary = tokio::task::spawn_blocking(move || -> Result<String, McpError> {
            let boxed = crate::app::build_byo_provider_engine(
                &settings,
                &settings.engine_id,
                settings.model.as_deref(),
            )
            .map_err(err)?;
            let passes = crate::ai::prompts::summarize_messages(&transcript);
            let mut parts = Vec::new();
            for pass in &passes {
                parts.push(
                    boxed
                        .stream_chat(pass, &mut |_| {})
                        .map_err(|e| err(e.to_string()))?,
                );
            }
            if parts.len() > 1 {
                boxed
                    .stream_chat(
                        &crate::ai::prompts::summarize_reduce_messages(&parts),
                        &mut |_| {},
                    )
                    .map_err(|e| err(e.to_string()))
            } else {
                Ok(parts.join("\n\n"))
            }
        })
        .await
        .map_err(|e| err(e.to_string()))??;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            redact_codes(&summary),
        )]))
    }

    #[tool(
        description = "Create a draft in the user's mailbox. The draft is NOT sent; use preview_send (gated) to send."
    )]
    fn create_draft(
        &self,
        Parameters(p): Parameters<CreateDraftParams>,
    ) -> Result<CallToolResult, McpError> {
        let to: Vec<dao::Addr> =
            p.to.iter()
                .map(|e| dao::Addr {
                    name: None,
                    email: e.clone(),
                })
                .collect();
        let cc: Vec<dao::Addr> =
            p.cc.iter()
                .map(|e| dao::Addr {
                    name: None,
                    email: e.clone(),
                })
                .collect();
        let draft_id = {
            let st = self.store.lock().map_err(|_| err("store poisoned"))?;
            // A reply attaches to the threads row the thrid points at.
            let thread_row: Option<i64> = p.thread_id.as_ref().and_then(|t| {
                st.conn()
                    .query_row(
                        "SELECT tm.thread_id FROM thread_msgs tm
                         JOIN messages m ON m.id = tm.message_id
                         WHERE m.thrid = ?1 LIMIT 1",
                        [t],
                        |r| r.get(0),
                    )
                    .ok()
            });
            let id = dao::add_draft(
                st.conn(),
                &dao::NewDraft {
                    account_id: p.inbox_id,
                    thread_id: thread_row,
                    to: &to,
                    cc: &cc,
                    subject: &p.subject,
                    body_text: &p.body,
                    in_reply_to: None,
                },
            )
            .map_err(|e| err(e.to_string()))?;
            dao::audit(
                st.conn(),
                "mcp",
                "create_draft",
                serde_json::json!({ "draft_id": id, "to": p.to, "subject": p.subject }),
            )
            .map_err(|e| err(e.to_string()))?;
            id
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "{{\"draft_id\": {draft_id}}}"
        ))]))
    }

    #[tool(
        description = "Step 1 of the gated send: returns a single-use confirm token bound to this draft's exact content. Only available when the user enabled agent sending in Settings."
    )]
    fn preview_send(
        &self,
        Parameters(p): Parameters<DraftIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let st = self.store.lock().map_err(|_| err("store poisoned"))?;
        let allowed = dao::get_setting(st.conn(), "mcp.allow_send")
            .ok()
            .flatten()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !allowed {
            return Err(err(
                "sending is disabled: the user has not enabled agent sending in Settings → MCP",
            ));
        }
        let draft = dao::draft_by_id(st.conn(), p.draft_id)
            .map_err(|e| err(e.to_string()))?
            .ok_or_else(|| err("draft not found"))?;
        if draft.status != "draft" {
            return Err(err("only draft-status drafts can be previewed"));
        }
        let fingerprint = content_fingerprint(&draft);
        let mut bytes = [0u8; 24];
        getrandom::fill(&mut bytes).map_err(|e| err(e.to_string()))?;
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        self.gates
            .0
            .lock()
            .map_err(|_| err("gates poisoned"))?
            .insert(p.draft_id, (fingerprint.clone(), token.clone()));
        dao::audit(
            st.conn(),
            "mcp",
            "preview_send",
            serde_json::json!({ "draft_id": p.draft_id, "fingerprint": fingerprint }),
        )
        .map_err(|e| err(e.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "{{\"confirm_token\": \"{token}\", \"note\": \"call send_draft with this token; it is single-use\"}}"
        ))]))
    }

    #[tool(
        description = "Step 2 of the gated send: sends the draft with the confirm token from preview_send. Single-use, rate-limited, audited."
    )]
    fn send_draft(
        &self,
        Parameters(p): Parameters<SendDraftParams>,
    ) -> Result<CallToolResult, McpError> {
        let (allowed, limit) = {
            let st = self.store.lock().map_err(|_| err("store poisoned"))?;
            (
                dao::get_setting(st.conn(), "mcp.allow_send")
                    .ok()
                    .flatten()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                dao::get_setting(st.conn(), "mcp.rate_limit_per_hour")
                    .ok()
                    .flatten()
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5),
            )
        };
        if !allowed {
            return Err(err("sending is disabled by the user"));
        }
        {
            let st = self.store.lock().map_err(|_| err("store poisoned"))?;
            if sends_last_hour(st.conn())? >= limit {
                return Err(err(format!("send rate limit reached ({limit}/hour)")));
            }
        }
        // Single-use token bound to the previewed content.
        let expected = {
            let mut gates = self.gates.0.lock().map_err(|_| err("gates poisoned"))?;
            gates.remove(&p.draft_id)
        };
        let Some((fingerprint, token)) = expected else {
            return Err(err("no preview for this draft — call preview_send first"));
        };
        if !constant_time_eq(token.as_bytes(), p.confirm_token.as_bytes()) {
            return Err(err("invalid confirm token"));
        }
        let fingerprint_ok = {
            let st = self.store.lock().map_err(|_| err("store poisoned"))?;
            let draft = dao::draft_by_id(st.conn(), p.draft_id)
                .map_err(|e| err(e.to_string()))?
                .ok_or_else(|| err("draft not found"))?;
            content_fingerprint(&draft) == fingerprint
        };
        if !fingerprint_ok {
            return Err(err("draft changed after preview — preview again"));
        }

        // The real send: own transport, same path as the GUI's driver.
        let result = send_draft_now(&self.store, p.draft_id);
        {
            let st = self.store.lock().map_err(|_| err("store poisoned"))?;
            dao::audit(
                st.conn(),
                "mcp",
                "send_draft",
                serde_json::json!({ "draft_id": p.draft_id, "ok": result.is_ok() }),
            )
            .map_err(|e| err(e.to_string()))?;
        }
        result?;
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "{{\"sent\": true, \"draft_id\": {}}}",
            p.draft_id
        ))]))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// SHA-256 fingerprint of the draft content the preview bound the token to.
fn content_fingerprint(draft: &dao::DraftRow) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    if let Ok(bytes) = serde_json::to_vec(&draft.to) {
        h.update(&bytes);
    }
    h.update(draft.subject.as_deref().unwrap_or_default().as_bytes());
    h.update(b"\x00");
    h.update(draft.body_text.as_deref().unwrap_or_default().as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Executes a real send from the MCP role: connects a fresh transport for
/// the draft's account (the driver's connections belong to the GUI).
fn send_draft_now(store: &Arc<Mutex<Store>>, draft_id: i64) -> Result<(), McpError> {
    let (account_id, credential_ref) = {
        let st = store.lock().map_err(|_| err("store poisoned"))?;
        let draft = dao::draft_by_id(st.conn(), draft_id)
            .map_err(|e| err(e.to_string()))?
            .ok_or_else(|| err("draft not found"))?;
        let cred: String = st
            .conn()
            .query_row(
                "SELECT credential_ref FROM accounts WHERE id = ?1",
                [draft.account_id],
                |r| r.get::<_, String>(0),
            )
            .map_err(|_| err("draft's account not found"))?;
        (draft.account_id, cred)
    };
    let password = crate::platform::providers::get_account_secret(&credential_ref)
        .map_err(|e| err(format!("credential read failed: {e}")))?;
    let store_clone = Arc::clone(store);
    let result = std::thread::spawn(move || {
        let st = store_clone.lock().map_err(|_| err("store poisoned"))?;
        let mut transport = connect_for_account(&st, account_id)?;
        crate::mail::smtp::send_draft(&mut transport, &st, draft_id, &password)
            .map_err(|e| err(e.to_string()))
    })
    .join()
    .map_err(|_| err("send thread panicked"))?;
    result
}

/// Connects an IMAP transport for one account (app-password path; OAuth
/// accounts route through the same refresh logic the GUI uses).
fn connect_for_account(
    st: &Store,
    account_id: i64,
) -> Result<crate::mail::imap::ImapTransport, McpError> {
    let (provider, email, auth_type, credential_ref, imap_host, imap_port): (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<u16>,
    ) = st
        .conn()
        .query_row(
            "SELECT provider, email, auth_type, credential_ref, imap_host, imap_port
             FROM accounts WHERE id = ?1",
            [account_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .map_err(|e| err(e.to_string()))?;
    let preset = crate::platform::providers::ALL
        .iter()
        .find(|p| p.id == provider);
    let host = preset
        .map(|p| p.imap_host.to_string())
        .or(imap_host)
        .filter(|h| !h.is_empty())
        .ok_or_else(|| err("account has no IMAP host"))?;
    let port = preset.map(|p| p.imap_port).or(imap_port).unwrap_or(993);
    let url: url::Url = format!("imaps://{host}:{port}")
        .parse()
        .map_err(|e| err(format!("bad endpoint: {e}")))?;
    let secret = crate::platform::providers::get_account_secret(&credential_ref)
        .map_err(|e| err(format!("credential read failed: {e}")))?;
    let auth = match auth_type.as_str() {
        "app_password" => crate::mail::imap::ImapAuth::Plain {
            user: email,
            password: secrecy::SecretString::from(secret),
        },
        "oauth" => {
            let tokens = crate::platform::oauth::refresh(&secrecy::SecretString::from(secret))
                .map_err(|e| err(format!("token refresh failed: {e}")))?;
            crate::mail::imap::ImapAuth::Xoauth2 {
                user: email,
                token: tokens.access_token,
            }
        }
        other => return Err(err(format!("unknown auth type {other}"))),
    };
    crate::platform::netlog::record(&host);
    crate::mail::imap::ImapTransport::connect(&url, auth)
        .map(|(t, _)| t)
        .map_err(|e| err(e.to_string()))
}

#[tool_handler]
impl ServerHandler for CracktivityMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                "cracktivity",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Read-only mail access by default. Sending requires the user's \
                 explicit two-step enablement in the cracktivity app.",
            )
    }
}

/// The stdio entry point (`--mcp` role): opens the encrypted store and
/// serves until the client disconnects. stdout is protocol-only.
pub fn serve_stdio() -> Result<(), String> {
    let profile = data_profile_dir()?;
    let store = Store::open(&profile, crate::store::dek::DEFAULT_DEK_TARGET)
        .map_err(|e| format!("store: {e}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("runtime: {e}"))?;
    runtime.block_on(async move {
        let service = CracktivityMcp::new(Arc::new(Mutex::new(store)))
            .serve(stdio())
            .await
            .map_err(|e| format!("serve: {e}"))?;
        service
            .waiting()
            .await
            .map(|_| ())
            .map_err(|e| format!("serve: {e}"))
    })
}

/// `%LOCALAPPDATA%\<identifier>\profile` — the same profile the GUI uses
/// (concurrent access is safe: WAL + busy timeout).
fn data_profile_dir() -> Result<std::path::PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA").map_err(|_| "no LOCALAPPDATA".to_string())?;
    Ok(std::path::Path::new(&local)
        .join("com.cracktivity.app")
        .join("profile"))
}

#[cfg(test)]
mod tests;
