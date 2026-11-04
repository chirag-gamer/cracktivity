# spec.md — cracktivity Technical Specification

Source of truth for **how** cracktivity is built. Requirements live in [prd.md](prd.md), rationale in [decisions.md](decisions.md), sequencing in [plan.md](plan.md).

Version: 1.0 (Phase 0) · Date: 2026-11-04 · Stack pins: Tauri 2.12.x · Rust stable (MSRV 1.90) · Node 24 / pnpm

---

## 1. System overview & process model

One binary, multiple roles:

```
cracktivity.exe (Tauri app)
├── GUI role (default): main window + notch panel + tray
│     └── WebView2 (React + Fluent UI v9) ── IPC (commands/Channel) ──┐
├── MCP role (--mcp): rmcp stdio server, no GUI                       │
└── llama-server.exe (bundled resource, child process, AI on)         │
      └── OpenAI-compatible HTTP on 127.0.0.1:<ephemeral> + api-key   │
                                                                     │
Rust core (in-process, tokio) ◄──────────────────────────────────────┘
├── mail engine: IMAP/SMTP per account (io-imap · lettre)
├── store: SQLCipher (rusqlite bundled) + FTS5
├── ai engine: ChatEngine trait → {LlamaCppSidecar, OpenAICompat, Anthropic}
├── classify: rule engine + counters (+ optional logistic model)
├── render: ammonia + lol_html sanitize/neutralize pipeline
└── platform: keyring(CredManager) · DPAPI · tray · shortcuts · notifications
```

Rules: only the mail engine and the AI engine may open outbound sockets (allowlist, D-015); `llama-server` binds loopback only with a random per-run key; the MCP role opens **no** sockets (stdio pipes only).

## 2. Repository layout

```
cracktivity/
├── README.md · prd.md · spec.md · plan.md · decisions.md · AGENTS.md
├── src/                      # React + TS (main window + panel pages)
│   ├── main/                 # three-pane app
│   ├── panel/                # notch dropdown app
│   ├── shared/               # components, ipc client, i18n strings
│   └── tests/                # vitest (mockIPC)
├── src-tauri/
│   ├── src/
│   │   ├── main.rs           # role dispatch (GUI vs --mcp)
│   │   ├── lib.rs            # builder: plugins, windows, command registry
│   │   ├── app.rs            # AppState, IPC commands, sync-driver thread, e2e seed
│   │   ├── accounts/         # provider presets, auth flows
│   │   ├── mail/             # imap adapter, sync engine, threading, actions; smtp (Phase-3 tail)
│   │   ├── store/            # sqlcipher schema, migrations, fts, ui queries
│   │   ├── ai/               # model manager, chat engines, prompts
│   │   ├── classify/         # rules, counters, optional learned layer
│   │   ├── render/           # sanitize pipeline (v1: ammonia pass, D-026)
│   │   ├── mcp/              # rmcp tools (used by --mcp role)
│   │   └── platform/         # keyring, dpapi, oauth; shell.rs = panel/tray/hotkey (D-011)
│   ├── capabilities/*.json
│   ├── resources/llama-server.exe   # pinned llama.cpp build
│   └── tauri.conf.json
├── e2e/                      # wdio config + launcher + specs (@wdio/tauri-service, D-027)
└── .github/workflows/        # ci.yml, nightly.yml, release.yml
```

## 3. Data model (SQLCipher / SQLite, WAL)

All tables have `created_at`/`updated_at`. FKs enforced. Migrations versioned by the `user_version` pragma (the implemented mechanism — no separate table).

```sql
accounts(id PK, provider TEXT CHECK(provider IN('gmail','outlook','icloud','zoho','yahoo','imap')),
         email, display_name, color, ord INTEGER, auth_type TEXT,      -- app_password|oauth
         credential_ref TEXT,          -- Credential Manager entry name (secret NOT here)
         sync_state TEXT, backfill_days INTEGER DEFAULT 90, enabled INT DEFAULT 1,
         last_full_sync_at, last_idle_at)

folders(id PK, account_id FK, name, role TEXT,        -- special-use: inbox|sent|drafts|trash|all|archive|junk|NULL
        uidvalidity INT, uidnext INT, highestmodseq TEXT NULL, highest_uid INT, subscribed INT)

messages(id PK, account_id FK, folder_id FK, uid INT,
         message_id TEXT, in_reply_to TEXT NULL, references_raw TEXT NULL,
         subject, from_json, to_json, cc_json, reply_to_json,
         date_sent INT, date_recv INT, size INT, flags TEXT,           -- JSON array of IMAP flags
         snippet TEXT, body_text TEXT NULL, body_html TEXT NULL,       -- html stored post-sanitization? NO — stored raw, sanitized at render (3 passes cached)
         headers_json, list_unsubscribe TEXT NULL, list_unsub_post TEXT NULL,
         thrid TEXT NULL,                                              -- X-GM-THRID or local JWZ id
         unread INT, starred INT, importance TEXT,                     -- 'important'|'other'
         importance_reasons_json, tracking_count INT DEFAULT 0,
         UNIQUE(account_id, folder_id, uid));

threads(id PK, account_id FK, subject_norm, participants_json, last_activity INT,
        importance TEXT, needs_followup INT DEFAULT 0, followup_since INT NULL,
        summary TEXT NULL, summary_model TEXT NULL, summary_at INT NULL);
thread_msgs(thread_id FK, message_id FK, PRIMARY KEY(thread_id, message_id));

attachments(id PK, message_id FK, filename, mime, size INT, cid TEXT NULL, part_path TEXT, cached INT);
contacts(id PK, account_id FK NULL, email UNIQUE-ish, name, sent_to INT DEFAULT 0, recv_from INT DEFAULT 0,
         is_vip INT DEFAULT 0, last_seen INT);
sender_stats(account_id FK, sender_email, opened INT, deleted_unread INT, received INT,
             classification_overrides TEXT, PRIMARY KEY(account_id, sender_email));
drafts(id PK, account_id FK, thread_id FK NULL, to_json, cc_json, subject, body_text,
       in_reply_to TEXT NULL, status TEXT,          -- draft|queued|sending|failed
       error TEXT NULL, updated_at INT);
settings(key PK, value_json);                       -- app settings (mirrors Credential-Manager-free state)
blocklist(id PK, pattern TEXT, kind TEXT,            -- domain|url_param|pixel_regex
          source TEXT, added_at);                    -- bundled + optional updates
audit_log(id PK, at INT, actor TEXT,                 -- 'mcp'|'user'
          action TEXT, detail_json);                 -- append-only (FR-27)
messages_fts(fts5 external content on messages(body_text, subject, snippet, from_json));
```

Indexing policy: `messages(account_id, folder_id, date_recv DESC)`, `(thread_id)`, `(unread)`; FTS kept in sync by triggers on messages insert/update of text columns. DB size guard: bodies beyond backfill window are pruned on a background task unless starred.

## 4. Sync engine

Per account, per folder, one tokio task. Transport behind the `MailTransport` trait (D-009).

**Required server capabilities:** IDLE, UIDPLUS, MOVE, SPECIAL-USE, LIST-EXTENDED, NAMESPACE, SASL-IR, ENABLE. **Used if present:** QRESYNC/CONDSTORE, SORT, THREAD, UTF8=ACCEPT, COMPRESS=DEFLATE. Capability probe result cached per account.

1. **Connect & auth** (XOAUTH2 or LOGIN with app password), TLS via rustls.
2. **Folder discovery:** `LIST` with SPECIAL-USE; map roles; detect UIDVALIDITY changes → on change: drop UID-keyed rows for that folder, re-baseline (FR-8 no-data-loss rule: messages are re-fetched, not lost).
3. **Initial backfill:** fetch newest N (backfill_days) in batches of 500: `UID FETCH <range> (UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID IN-REPLY-TO REFERENCES REPLY-TO LIST-UNSUBSCRIBE LIST-UNSUBSCRIBE-POST)])`. Always `.PEEK` (never set `\Seen` implicitly). `X-GM-THRID` is not a header — it is a Gmail FETCH item that generic HEADER.FIELDS cannot retrieve; fetch it extension-side when the server models it (D-019), else JWZ covers Gmail correctly. Write in transactions of 500; UI shows progress; kill-safe (resumable via highest_uid).
4. **Steady state:** if IDLE: idle with 25-min self-renew; on untagged EXISTS → fetch new range (same header-only fetch); on FLAGS/FETCH events → update rows. If no IDLE (Outlook fallback): poll every 90 s. *(v1 driver ships the 90-s polling path only — io-imap's eventful `watch_mailbox` IDLE remains future work, D-019/D-026.)* **Expunge/flag reconciliation without QRESYNC:** every 5 min (and after each IDLE burst), `UID SEARCH ALL` diff vs. local highest set + `FETCH (UID FLAGS)` snapshot diff. With QRESYNC: `SELECT ... (QRESYNC (last_uidvalidity highestmodseq))` and apply vanished/changed sets.
5. **Body fetch:** on open + prefetch of text parts for the 10 newest unread in the active view. Attachment blobs only on explicit save (FR-10).
6. **Send path:** SMTP submission (STARTTLS 587 or implicit TLS 465 per provider matrix); on success, IMAP APPEND to the Sent folder with `\Seen` + date (FR-18); Outbox rows are retried with backoff; failure reasons surfaced (FR-5).

Threading: JWZ over `message_id`/`in_reply_to`/`references` + subject normalization (`re:|fwd:|fw:|aw:|sv:|odp:` etc., case/punct-insensitive); `X-GM-THRID` short-circuits when present (D-010).

## 5. Auth & credentials

| Provider | v1 path | Storage |
|---|---|---|
| Gmail (personal) | App password (2SV required; guided flow links google.com/apppasswords) | Credential Manager |
| Outlook.com | OAuth2 XOAUTH2; scopes `IMAP.AccessAsUser.All SMTP.Send offline_access`; loopback redirect; Entra app registration owned by project | Credential Manager (refresh token ≤ 2.5 KB — fits blob cap) |
| iCloud | App-specific password | Credential Manager |
| Zoho | App password (IMAP enabled in Zoho settings) | Credential Manager |
| Yahoo | App password | Credential Manager |
| Generic IMAP | Password | Credential Manager |

Implementation: `keyring-core` + `windows-native-keyring-store` 1.1 (CredWriteW/CredReadW generic credentials, persist=Local, one entry per secret; never share entries across threads). SQLCipher DEK: random 32 bytes → `CryptProtectData` (user scope) → wrapped blob in Credential Manager → decrypted in memory at DB open. DPAPI orphan recovery in §11.

## 6. AI subsystem

### 6.1 Models (D-004)

| Tier (auto-detect: RAM+cores) | Classify (resident) | Write (on demand) |
|---|---|---|
| LOW (< 6 GB RAM / 2 cores) | Qwen3-0.6B Q4_K_M, ctx 2048 | Qwen2.5-1.5B Q4_K_M |
| MID (6–12 GB / 4 cores) | Qwen3-0.6B | Qwen3-1.7B Q4_K_M |
| HIGH (≥ 16 GB / 8 cores) | Qwen3-0.6B | Qwen3-4B-Instruct-2507 (fallback Phi-4-mini) |

Only one write-tier model loaded at a time; classify stays resident; write process spawns per task burst and exits after idle timeout (2 min) to free RAM.

### 6.2 Process & transport

`llama-server.exe` (pinned stable release zip, MIT) spawned via `std::process::Command` from resources dir:
`-m <model> -c <4096|8192> -t <P-core-count> -tb <P-core-count> -np 1 --host 127.0.0.1 --port <ephemeral> --api-key <random-per-run> --cache-prompt --cache-reuse 256 --cache-ram 2048 -lm mmap --no-webui`
Readiness: poll `GET /health` until 200. Requests: `POST /v1/chat/completions` (SSE stream). Structured output (classification JSON): `response_format: {"type":"json_schema","json_schema":{...strict...}}`.

### 6.3 ChatEngine trait (covers BYO keys)

```rust
trait ChatEngine { async fn stream_chat(&self, msgs: Vec<Msg>, schema: Option<JsonSchema>) -> TokenStream; }
```
Implementations: `LlamaCpp` (llama-server), `OpenAiCompat` (OpenAI `api.openai.com/v1`, xAI `api.x.ai/v1`, Anthropic-compat `/v1/chat/completions` — marked not-production-grade by Anthropic), `Anthropic` (native `/v1/messages` SSE — preferred for Anthropic keys). Selection: user's per-task engine choice; fallback chain local→none (never silently cloud, FR-24).

### 6.4 Prompts & budgets

Short, task-specific system prompts (SLM quality degrades with complex prompts — verified). Quote/signature stripping before summarization (threads shrink ~3–5×). Context budget 8k default; long threads chunked and map-reduced. Expected latency (warm prefix, MID): classify ~0.5 s, draft ~10 s, summarize ~16 s (HIGH ~30 s) — UX streams tokens with staged status lines.

### 6.5 Model manager (FR-20)

First-run opt-in dialog (checkbox ON, plain-language cost statement) → download from pinned HF commit URLs (`https://huggingface.co/<repo>/resolve/<commit>/<file>.gguf`) with SHA-256 verify + resumable Range requests → `%LOCALAPPDATA%\cracktivity\models\`. Disk budget: LOW ~1.1 GB · MID ~1.7 GB · HIGH ~3 GB (+0.4 GB classify). Deletion reclaims space. llama-server binary ships in the installer (≤ 40 MB target preserved).

### 6.6 Master switch (FR-25)

Settings toggle → kill child processes, clear engines, hide UI affordances. No model process ever runs with AI disabled.

## 7. Notch dropdown panel (D-011)

- Window flags: `decorations(false), transparent(true), always_on_top(true), skip_taskbar(true), shadow(false), resizable(false), visible(false), no_redirection_bitmap(true)`; logical size 480×640.
- Show: resolve cursor monitor → top-center (`set_position`), re-assert `WS_EX_TOOLWINDOW` via `SetWindowLongPtrW(GWL_EXSTYLE, ex|0x80)` (must repeat after every show / ignore-cursor change), `set_focus()`, CSS slide-down (200 ms, `will-change: transform`).
- Hide: `WindowEvent::Focused(false)` with 150 ms debounce excluding own windows; Esc in webview; hotkey toggles.
- Triggers: tray left-click toggle; global shortcut plugin (`tauri-plugin-global-shortcut` builder pattern, `with_handler` → emit to panel).
- Content: new-mail triage list (unread across accounts), quick actions (archive/reply/mark-read), deep-link into main window.
- Known accepted limitations: elevated-window focus steals hotkey (needs UIAccess+signing — deferred); exclusive-fullscreen apps can cover the panel; Win10 falls back to squared corners.
- v1 scope (D-026/D-025): the panel hides fully instead of resting as a notch strip (`set_ignore_cursor_events` unused); quick actions ship archive/mark-read — "reply" deep-links into the main window until compose lands.
- Verification gate (Phase 3): transparent-panel perf on low-end hardware; if bad, opaque panel with themed background (decision recorded then).

## 8. Mail rendering & tracker blocking (D-014)

Pipeline (Rust, on body open; result cached per message). *(v1 scope, D-026/D-028: a single ammonia pass implements steps 1+2's blocking and step 3 — block ALL remote images with a count, strip `utm_*`/click-ids — and wraps the output with a CSP meta plus `<base target="_blank">` (sandbox makes links inert). The `lol_html` `data-blocked-src` placeholder, tracker-list matching, per-sender image allowlist and the local image proxy arrive with "Show images" in Phase 4; the v1 render is computed on open, not cached.)*
1. `ammonia` whitelist: strip `script, style, iframe, form, object, embed, meta, link, base`; drop all `on*` attributes, `javascript:`/`data:` hrefs; force `rel="noopener noreferrer"`.
2. `lol_html` pass: remote `img[src|srcset]` → `data-blocked-src` placeholder + increment `tracking_count` (pixel heuristics: 1×1 dimensions, known tracker domains from bundled list); `cid:` → local attachment scheme.
3. Link cleaning: strip `utm_*`, `fbclid`, `gclid`, `mc_eid`, `msclkid`, known redirect-tracker prefixes (bundled `blocklist` table; update channel opt-in).
4. Render: sandboxed iframe in the reader — `sandbox=""` (no scripts, no same-origin), CSP `default-src 'none'; img-src data: <app-scheme>`.
5. Overrides: per-message "Show images" (fetches remote images through the app's HTTP client with no referer; honest IP note in tooltip); per-sender allowlist persisted in `sender_stats`.

Known limits (documented in-product, NFR-10): allowed images leak IP; click-tracker redirects fire in the external browser for unknown trackers; novel pixels pass heuristics.

## 9. MCP server (D-012)

- Runtime: `rmcp` 3.8.x, `transport-io` (stdio), tool macros + schemars. Role dispatch in `main.rs`: `--mcp` → skip all GUI/window/tray init; log to stderr only.
- Tools: `list_inboxes`, `list_recent_emails{inbox_id?,limit?,since?}`, `search_emails{query,inbox_id?,after?,before?,limit?}`, `read_thread{thread_id}` (plain text only), `summarize_thread{thread_id}`, `create_draft{...}` → `draft_id`, gated pair `preview_send{draft_id}` → `send_draft{draft_id, confirm_token}`.
- Safety: read-only default; Settings → MCP gates sends; single-use confirm token bound to a SHA-256 fingerprint of the previewed content; per-hour send rate limit; audit_log rows for every mutation; Spam/Trash excluded from all tool output; verification-code-like strings redacted; bodies fenced `<untrusted_mail_body nonce=…>` with escaped closing tags.
- One-click registration: writes/merges `claude_desktop_config.json`, `~/.claude.json` (Claude Code), `~/.codex/config.toml`, `~/.cursor/mcp.json` — always with a `.bak` backup and a UI diff preview.
- Test: handshake against rmcp client (2026-07-28) AND a handshake-era client fixture; GUI + MCP concurrently.

## 10. Security constraints

1. Secrets only in Credential Manager / DPAPI-wrapped DEK; never in config files, webview storage, or logs (FR-2 AC enforced by test).
2. Mail HTML: sanitize → neutralize → sandbox (§8); the mail-rendering iframe has no Tauri IPC exposure.
3. Network allowlist enforced in the egress layer (HTTP client wrapper + IMAP/SMTP hosts from account config); Diagnostics lists session hosts (FR-32).
4. MCP: least power by default; fenced untrusted content; no shell/exec tools ever.
5. Updates: minisign-signed artifacts; verification not disableable; check OFF by default (D-015/D-016).
6. Windows bits: no `UIAccess`; no services; no drivers; per-user install; no registry persistence beyond autostart (opt-in, `tauri-plugin-autostart`).
7. Dependency policy: no analytics crates/npm packages (CI audit job); Rust deps pinned via `Cargo.lock`; npm via lockfile + `pnpm audit` gate.

## 11. Failure & recovery

| Failure | Behavior |
|---|---|
| IMAP disconnect / timeout | Reconnect w/ exponential backoff (1s→60s cap); IDLE re-established; UI banner (FR-5) |
| UIDVALIDITY change | Drop folder's UID-keyed rows, re-baseline; user mail untouched |
| Token refresh failure | Re-auth prompt (browser flow re-run); queued sends preserved |
| Kill mid-backfill / mid-send | WAL recovery; backfill resumes from `highest_uid`; Outbox rows marked `queued` retry |
| DPAPI orphan (admin password reset) | Detect unwrap failure → offer "re-sync from server" (credentials intact in Credential Manager); local bodies rebuild from headers+lazy fetch |
| DB corruption | `.backup`-based restore from automatic rolling snapshot (last 3, in local dir) else clean re-sync offer |
| Model download interrupted | Resume via Range; SHA-256 mismatch → re-download |
| llama-server crash | Restart on next task; AI features degrade to disabled-with-notice, never crash the app |

## 12. Performance budgets

Cold start → interactive ≤ 3 s warm / ≤ 6 s cold (SSD, 8 GB box). List render: virtualized, 60 fps @ 10k. Search < 1 s @ 100k msgs (FTS5). Sync: 15k-message backfill ≤ 10 min @ 50 Mbps; steady-state CPU ≤ 2% idle (IDLE wait is evented, not polled). Memory: app ≤ 500 MB excl. models; models per D-004 tier table. Backfill writes batched (500/txn) with prepared statements.

## 13. Test strategy (D-018)

| Level | Tooling | Covers | CI cadence |
|---|---|---|---|
| L1 Frontend unit | Vitest + Testing Library + `@tauri-apps/api/mocks` (`mockIPC`, `clearMocks`) | components, state, IPC contract | every PR |
| L2 Rust unit / commands | `cargo test` + `tauri::test` MockRuntime (`get_ipc_response`), feature `test` | commands incl. State, rules engine, threading, classify | every PR |
| L3 Sync engine integration | scripted-socket IMAP mock (IDLE + QRESYNC scenarios, reconnect/UIDVALIDITY cases) + in-memory store | delta sync, flag diff, expunge, resumable backfill | every PR |
| L4 Live IMAP E2E | Dovecot container (CONDSTORE/QRESYNC + IDLE) gated `#[ignore]`; GreenMail for SMTP submit | full sync cycle | nightly |
| L5 UI E2E | WebdriverIO + `@wdio/tauri-service` (external `tauri-driver` provider in CI) ; Playwright `connectOverCDP` fallback (unique port + `WEBVIEW2_USER_DATA_FOLDER` per run) | real user flows: onboarding dialog, add account (mocked server), triage actions, panel toggle, MCP settings | smoke each PR; full on main |
| L6 Native UI | FlaUI.UIA3 (local, interactive) | tray, Alt-Tab exclusion, hotkey | manual per release |
| L7 AI | tiny GGUF (SmolLM2-135M / Qwen3-0.6B), greedy decode, golden files; contract tests for ChatEngine wire formats | prompts, schema outputs, adapter contracts | golden each PR; live nightly |

E2E scope note (FR coverage): L5 scripts assert FR-20 dialog, FR-28 panel behavior (DOM side), FR-30 keyboard actions, FR-31 tray actions via exposed commands, FR-27 MCP settings flow. Native-level assertions (Alt-Tab absence, hotkey under focus) are L6.

## 14. Traceability (summary; full matrix in plan.md)

Every FR-N maps to: spec §(listed in plan matrix) · phase task · test level(s) L1–L7. PRs must update the matrix row status. No FR ships without its acceptance criteria demonstrated by an automated test or a recorded manual QA run with evidence in the PR description.
