# prd.md — cracktivity Product Requirements

Owner-facing source of truth for **what** we build and **how we know it works**. The *how it's built* lives in [spec.md](spec.md); *why choices were made* in [decisions.md](decisions.md); *when* in [plan.md](plan.md).

Version: 1.0 (Phase 0) · Date: 2026-11-04 · Platform: Windows 10 (1809+) and Windows 11

---

## 1. Product overview

cracktivity is a native, privacy-first desktop email client for Windows. It brings multiple inboxes (Gmail, Outlook.com, iCloud, Zoho, Yahoo, any IMAP) into one keyboard-driven environment. It connects **directly** to providers — there is no cracktivity server, account, or relay. AI features (thread summaries, reply drafting) run on a small open-weights model on the user's own CPU, opt-in with the recommendation ON. A macOS-style "notch dropdown" panel drops from the top of the screen for fast triage. Developer AI agents get a built-in, local MCP server.

Primary design reference: [Bloop.email](https://bloop.email/) (macOS) — the closest existing product; cracktivity is its Windows counterpart with a CPU-only local model (see D-002).

## 2. Goals

1. One app where all mail accounts live together, sorted so the important stuff is seen first — computed on-device.
2. Zero middlemen: no telemetry, no analytics, no cloud sync, no proxy for mail content.
3. Full offline capability for everything already synced: read, search, draft.
4. Keyboard-first triage fast enough to process a full inbox without touching the mouse.
5. On-device AI that respects low-end hardware (no GPU, ≥4 GB RAM machines must not be excluded).
6. A developer story (MCP) that never gives an agent more mail power than the user granted.

## 3. Non-goals (v1)

- **Gmail OAuth** (Google restricted-scope verification + CASA assessment) — deferred; Gmail works via app password in v1 (D-008).
- Google Workspace accounts and Google Advanced Protection accounts.
- Snooze, send-later, undo-send (candidate for post-1.0).
- Calendar and contacts sync; PGP/S-MIME; mobile apps; macOS/Linux builds.
- Team/shared inboxes; plugin ecosystem; theme marketplace.
- Any server-side component, ever (including "anonymous telemetry").

## 4. Target users

- **Primary:** privacy-conscious professionals with 2–5 mail accounts who live in their inbox on Windows and resent being forced into webmail or cloud-reliant clients.
- **Secondary:** developers who want their AI coding agents to read/draft mail locally and safely.
- **Hardware floor we must serve:** 4 GB RAM / 2-core Windows 10 laptop (reduced-feature AI tier, everything else identical).

## 5. User stories

**Triage flow**
- As a user with 3 accounts, I open the dropdown with a hotkey, see the day's new mail across accounts, and archive/reply to two of them — in under 30 seconds, mouse-free.
- As a user, I want newsletters and receipts out of my face: they land in Other automatically, and if the app guesses wrong, moving one message teaches it.
- As a user, when someone hasn't answered my sent mail for 3 days, I want it back on my radar with a one-click follow-up draft.
- As a user drowning in newsletters, I want a ranked list of senders I never open and a one-click unsubscribe that actually works (one-click POST where the sender supports it).

**Reading & writing flow**
- As a user on a plane, I can read, search, and draft replies offline; sends queue and flush on reconnect.
- As a user, I compose with AI help only when I ask; the draft always starts as something I can edit before sending.

**Setup flow**
- As a user, I add a Gmail account by following a guided app-password flow and see my inbox within a minute.
- As a privacy-minded user, at first run I get one clear choice about the on-device AI model (recommended ON), with its disk/RAM cost stated plainly.
- As a user, I can see exactly which hosts my mail client has talked to.

**Developer flow**
- As a developer, I run one command and my AI agent can list recent mail, search, read threads, and summarize — but it cannot send or delete anything unless I turned that on in settings, and every action is logged locally.

## 6. Functional requirements

Acceptance criteria (AC) are observable behaviors; each FR maps to a spec section and a planned verification (matrix in [plan.md](plan.md)).

### A. Accounts & connections

- **FR-1 Add account.** Users add accounts via provider presets (Gmail, Outlook.com, iCloud, Zoho, Yahoo) or manual IMAP/SMTP entry.
  - AC: Adding Gmail with an app password completes a guided flow (with a link to Google's app-password page) and the inbox appears with a visible sync progress state; the account is listed in Settings → Accounts and can be removed with an explicit "also delete local data" choice.
- **FR-2 Credential storage.** All passwords, app passwords, and OAuth refresh tokens are stored in Windows Credential Manager; the SQLCipher DEK is DPAPI-wrapped.
  - AC: After adding an account, `cmdkey /list` shows a cracktivity generic credential; grepping the app data directory for the secret finds nothing; on a different Windows user account, the mail DB does not open.
- **FR-3 Outlook.com OAuth.** Outlook.com connects via OAuth2 (XOAUTH2) with loopback redirect; refresh is automatic.
  - AC: The connect flow opens the system browser, returns via 127.0.0.1, and completes without the user copying tokens; with the app running, an expired access token refreshes without user interaction; a failed refresh shows a re-auth prompt, not a silent error.
- **FR-4 Multiple accounts & Ctrl+1..9.** Accounts are ordered; Ctrl+1..9 jumps to the corresponding account/folder view in-app.
  - AC: With 3 accounts configured, Ctrl+2 focuses account 2's view; reordering accounts in settings is reflected immediately.
- **FR-5 Connection state.** Connection failures surface as a non-blocking banner with retry; recovery is automatic.
  - AC: Disabling the network mid-session shows "Offline — will retry" within seconds; reconnecting resumes sync and clears the banner without app restart; a wrong password shows an actionable message linking to the account settings.

### B. Sync & offline

- **FR-6 Initial backfill.** New accounts backfill the last 90 days (configurable) headers-first; bodies fetch lazily on open.
  - AC: During backfill the list is usable within 30 s on a normal connection; opening any listed message fetches and caches its body; total disk use for a 90-day, ~15k-message mailbox stays under ~2 GB.
- **FR-7 Incremental sync.** New mail arrives via IMAP IDLE where supported, polling fallback otherwise; flags and expunges reconcile in both paths.
  - AC: A message sent to the account from another client appears in the app within 60 s (IDLE provider) or ≤ 5 min (polling provider); marking read on the web reflects in the app within one sync cycle and vice versa.
- **FR-8 Full offline mode.** Cached mail is readable and searchable offline; sends queue.
  - AC: With networking disabled, opening cached threads, searching, and composing works; queued mail sends automatically on reconnect; killing the app mid-sync never corrupts the store (next start resumes without user action).
- **FR-9 Local + server search.** FTS5-backed local search over cached mail; optional server-side IMAP SEARCH extends results to uncached ranges.
  - AC: A search over a 100k-message cache returns results in < 1 s; toggling "also search server" adds uncached results merged into the same list, clearly marked.
- **FR-10 Attachments.** Attachment lists show name/type/size; saving works.
  - AC: A message with 3 attachments lists them; Save As works offline if the part is cached, otherwise downloads on demand with progress.

### C. Triage

- **FR-11 Unified inbox & Important/Other.** A unified view and per-account views exist; incoming mail is auto-classified Important or Other on-device.
  - AC: A seeded newsletter (with List-Unsubscribe, not addressed To: me) lands in Other; a personal reply to my thread lands in Important; moving a message to the other bucket updates the model for that sender/pattern, and a similar later message lands in the corrected bucket.
- **FR-12 On-device classification.** Classification uses only local signals (headers, history, user corrections).
  - AC: With a network monitor active, classifying a batch of new mail produces zero external network calls.
- **FR-13 Automated follow-ups.** Sent mail with no reply after N days (default 3, user-set 1/2/3/5/7) appears in a "Waiting for Reply" view with a one-click follow-up draft.
  - AC: Seeding a sent message with no reply and winding the clock forward places the thread in the view; the follow-up action opens a pre-filled in-thread draft.
- **FR-14 Subscription cleanup.** A cleanup view ranks senders by engagement (never-opened first), supports one-click unsubscribe (RFC 8058 POST where `List-Unsubscribe-Post` is present, mailto fallback otherwise), optional bulk-archive of that sender's mail with Undo.
  - AC: The view sorts senders by open rate; clicking Unsubscribe performs the POST only on that explicit click and reports success/failure; bulk archive is reversible via Undo within the session; no automatic unsubscribe ever happens without a click.
- **FR-15 Read receipts & tracking badges.** When tracker pixels are detected/blocked, the reader shows a badge naming the count.
  - AC: Opening a seeded tracked email shows "Blocked N trackers"; allowing images for that sender updates future messages per allowlist rules.

### D. Reading & composing

- **FR-16 Threaded reading.** Mail groups into threads (JWZ algorithm); quotes are collapsed by default.
  - AC: A 6-message reply chain shows as one thread with 6 entries; "show quoted text" expands; opening a thread from the list preserves scroll position when returning.
- **FR-17 Compose, reply, forward, drafts.** Composing supports reply/forward with quoting; drafts persist locally.
  - AC: Killing the app mid-compose and restarting restores the draft; drafts survive offline; sending a draft removes it from Drafts.
- **FR-18 Send via SMTP.** Sending uses the account's SMTP submission; a copy is saved to the provider's Sent folder via IMAP APPEND.
  - AC: A sent message appears in the provider's own Sent folder on the web; a send failure (bad password, offline) leaves the message in a retryable Outbox with the reason shown.
- **FR-19 Remote content blocked by default.** HTML mail renders sanitized, with remote images blocked and links cleaned.
  - AC: A seeded HTML mail with scripts/iframes/tracking pixels renders with no script execution and no external requests (verified by render log); "Show images" (per message) and "Always for this sender" both work; clicked links have `utm_*` parameters stripped.

### E. On-device AI

- **FR-20 First-run opt-in.** First app start shows the on-device AI opt-in dialog (recommended, checkbox ON by default) stating what it does, the model size, and disk/RAM cost.
  - AC: Fresh install shows the dialog exactly once; declining leaves zero model files on disk and no model processes; accepting starts the pinned-model download with progress, resumable after interruption, SHA-256-verified.
- **FR-21 Thread summarization.** With AI enabled, any thread can be summarized on-device.
  - AC: The summary streams in incrementally; it works with networking fully disabled; it respects a ≤ 8k context budget (long threads are chunked); a regenerate action exists.
- **FR-22 Reply drafting.** With AI enabled, the user can generate a reply draft from thread context plus a short instruction.
  - AC: The draft streams into the composer as editable text; nothing is ever sent without the user's send action; with AI disabled the button is hidden, not merely erroring.
- **FR-23 Hardware tiering.** The app detects RAM/CPU and selects the model tier (LOW: Qwen2.5-1.5B, MID: Qwen3-1.7B, HIGH: Qwen3-4B-2507/Phi-4-mini); the user can override.
  - AC: Simulating an 8 GB machine selects Qwen3-1.7B with a Settings line explaining why; manual override persists and takes effect without app restart (model swap at next task).
- **FR-24 BYO API keys.** OpenAI, Anthropic, and xAI keys can be configured as the engine for summarize/draft; keys live in Credential Manager.
  - AC: With a key set, drafting calls the chosen provider (verified via its request log in Diagnostics) and the app falls back to the local model (if enabled) with a visible notice when the API is unreachable; keys never appear in config files (FR-2 check).
- **FR-25 AI master switch.** One toggle disables all AI: processes killed, features hidden.
  - AC: Toggling off terminates llama-server processes (Task Manager verifies zero) and hides AI affordances; toggling on restores without app restart.

### F. MCP server

- **FR-26 Built-in MCP server.** The app binary doubles as an MCP stdio server (`cracktivity.exe --mcp`), with one-click registration into Claude Desktop, Claude Code, Codex, and Cursor configs.
  - AC: Clicking "Add to Claude Desktop" updates that app's config (with a backup) and Claude Desktop lists cracktivity's tools; `cracktivity.exe --mcp` speaks the handshake with an MCP client while the GUI may run concurrently.
- **FR-27 MCP safety.** Toolset is read-only by default; sending is an explicit, two-step, opt-in flow; a local audit log records every mutating action.
  - AC: With defaults, no tool can send/delete/modify; enabling "Allow agents to send email" makes `preview_send` → `send_draft(confirm_token)` available, where the token is single-use and bound to the previewed content; Settings → MCP shows the audit log; Spam/Trash content and verification-code-like strings are excluded from tool output.

### G. Windows experience & UI

- **FR-28 Notch dropdown panel.** A borderless top-center panel drops from the top edge: opens via tray left-click or global hotkey (default Ctrl+Shift+Space, configurable); hides on blur/Esc; absent from taskbar and Alt-Tab; usable during normal app usage.
  - AC: With a browser focused, the hotkey opens the panel above it; clicking elsewhere hides it; Alt-Tab list does not contain the panel; panel re-opens correctly after monitor resolution/DPI changes.
- **FR-29 Main window.** Three-pane layout (accounts/folders · list · reader), system-following light/dark theme.
  - AC: Switching Windows personalization to dark reflects in-app within 1 s; panes resize with sane minimums; last layout (sizes, selected view) restores on relaunch.
- **FR-30 Keyboard-first navigation.** j/k move, Enter open, e archive, r reply, c compose, u unread, s star, / search, ? shortcuts overlay, Ctrl+K command palette, Ctrl+1..9 accounts.
  - AC: Every listed action works in the main window and dropdown without mouse use; ? overlays the full cheat sheet with the active keymap label (cracktivity/Gmail-style selectable in Settings).
- **FR-31 Tray.** Tray icon shows unread count badge; context menu: Open, Compose, Check all accounts, Settings, Quit.
  - AC: Badge count equals unread across accounts after sync; "Compose" opens the composer focused; Quit fully exits (no background process remains).

### H. Privacy & security

- **FR-32 Network allowlist & zero telemetry.** The app contacts only: configured mail providers; HuggingFace (during opt-in model download); update endpoint (check OFF by default); BYO AI endpoint (only when triggered); optional blocklist update (OFF by default). No analytics exist in code.
  - AC: A Diagnostics panel lists every remote host contacted this session; an automated egress test on a seeded setup observes zero connections outside the allowlist; dependency audit contains no analytics packages.
- **FR-33 Encrypted local cache.** The SQLite mail store is SQLCipher-encrypted with a DPAPI-wrapped key.
  - AC: Opening the DB file in a sqlite CLI without the key fails; deleting the Credential Manager entry makes the DB undecryptable and the app offers a clean re-sync rather than crashing.
- **FR-34 Manual-first updates.** Update checking is OFF by default.
  - AC: Fresh install makes no update-related network calls; enabling "Check for updates" performs a check on demand; an available update shows release notes and applies via the signed updater flow.

## 7. Non-functional requirements

- **NFR-1 Performance:** app interactive ≤ 3 s after launch (warm) on an SSD 8 GB machine; list scroll 60 fps at 10k visible-index messages; search < 1 s at 100k messages (FR-9).
- **NFR-2 Memory:** main app + UI ≤ 500 MB typical (excluding model RAM); AI adds ≤ ~2–4 GB per active model tier.
- **NFR-3 Reliability:** no user data loss on crash/kill at any sync step (WAL + idempotent sync writes); auto-reconnect with exponential backoff, capped.
- **NFR-4 Accessibility:** 100% keyboard operability (FR-30); UIA-labeled controls; visible focus rings; respects OS text scaling.
- **NFR-5 Windows support:** Windows 10 1809+ and Windows 11, x64 (ARM64 best-effort via llama.cpp ARM build — explicitly stretch, not a promise).
- **NFR-6 Installer:** per-user, no admin, ≤ 40 MB before optional model download.
- **NFR-7 Privacy:** zero telemetry in code (D-015); secrets never in webview storage; mail HTML always sanitized + sandboxed (D-014).
- **NFR-8 Maintainability:** every FR traceable to spec section, task, and test (matrix in plan.md); decisions.md append-only.
- **NFR-9 Localization:** English v1; all strings externalized from day one so translations don't require refactors.
- **NFR-10 Honest limits:** the UI never overstates privacy (e.g., the local image proxy's IP caveat is stated in-product, D-014).

## 8. Success criteria

Because there is zero telemetry (D-015), success is measured locally and by direct user feedback:
1. A new user reaches a fully synced unified inbox within 5 minutes of first launch (manual QA script).
2. Triage benchmark: the seeded QA scenario (50 mixed messages) is classified ≥ 90% correctly by rules alone; corrections are one keypress.
3. AI benchmark: summaries for the 20-thread golden set are judged useful (rubric ≥ 4/5) on MID tier hardware; drafting completes in ≤ 20 s warm.
4. Crash-free local sessions across the Phase 6 QA matrix (Win10 1809, Win10 22H2, Win11 23H2/24H2).
5. The MCP toolset passes the safety test suite (no mutation without explicit enable + confirm).

## 9. Open questions

1. **Branding/name collision** with bloop.email awareness in the market — marketing decision, not engineering; tracked for Phase 6.
2. **Outlook.com app registration** publisher-verification burden for an individual dev account — verify during Phase 2 with a real registration.
3. **Blocklist sourcing/license** (UglyEmail license terms; EasyPrivacy compatibility) — legal check before bundling (D-014).
4. **rmcp backward-compat in practice** against current Claude Desktop/Cursor builds — verify in Phase 4 with real clients.
