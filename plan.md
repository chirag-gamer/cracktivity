# plan.md — cracktivity living plan

Consolidates the document inventory and the phased task list. Updated continuously; statuses: ☐ todo · ◐ in progress · ☑ done (with verification evidence noted). FR IDs reference [prd.md](prd.md); § references [spec.md](spec.md).

---

## 1. Document inventory

| Doc | Role | Status |
|---|---|---|
| README.md | User-facing front page (structure mirrors chirag-gamer/button) | ☑ Phase 0 |
| prd.md | Goals, user stories, FR-1..34 + AC, NFRs, non-goals | ☑ Phase 0 |
| spec.md | Architecture, data model, sync/AI/MCP/security, test strategy | ☑ Phase 0 |
| decisions.md | Append-only decision ledger D-001..D-018 | ☑ Phase 0 (living) |
| AGENTS.md | Agent contract: source of truth, safety, workflow, gates | ☑ Phase 0 |
| plan.md | This file — tasks, phases, traceability | ☑ Phase 0 (living) |

## 2. Phases

### Phase 0 — Discovery, research, docs, decisions — ☑ done 2026-11-04
- ☑ 10 read-only research subagents in 2 batches (AI models · UI/framework · email protocols · Windows platform · UX/Bloop/README · MCP · Tauri facts · AI integration · tracker blocking · testing/CI).
- ☑ Main-agent verification pass: Bloop.email, MCP spec 2026-07-28 + rmcp 3.8.0, tauri 2.12.5, keyring/windows-native-keyring-store, rusqlite-bundled features, MCP versioning page.
- ☑ Canonical docs written (above). Key decisions D-001..D-018 recorded, incl. explicitly provisional items (D-009 io-imap, D-006 FTS5 build-flag note, llama.cpp release-tag resolution).

### Phase 1 — Repo, tooling, scaffold, tests & CI — ☐
1. `git init`; commit Phase 0 docs; `.gitignore` (target/, node_modules/, dist/, models/, secrets).
2. Scaffold Tauri 2.12 + React + TS + Vite + pnpm workspace; `tauri.conf.json` v2 skeleton (NSIS target, capabilities files); Fluent UI v9 + theme provider (light/dark follows system).
3. Rust workspace layout per spec §2; empty modules compile; `tauri::test` harness wiring (feature `test`).
4. Toolchain gates: cargo fmt/clippy, eslint+prettier+tsc, vitest hello-test, CI `ci.yml` on `windows-latest` (lint + L1 + L2 jobs, rust-cache + pnpm cache).
5. Verify: **V-1.1** CI green on windows-latest; **V-1.2** `pnpm tauri dev` opens main window on local Win11; **V-1.3** FTS5 startup smoke test passes (D-006 note); **V-1.4** keyring write/read round-trip test passes on Windows (D-007 byte-encoding open question resolved here).

### Phase 2 — Core model, boundaries, persistence & recovery — ☐
1. SQLCipher schema + migrations (spec §3) with DPAPI-wrapped DEK; store module + tests.
2. `MailTransport` trait; io-imap adapter; scripted-socket IMAP mock harness (L3) incl. IDLE/QRESYNC/reconnect/UIDVALIDITY scenarios.
3. Sync engine per spec §4 (backfill, steady-state, flag/expunge reconcile, body lazy-fetch, batching, WAL kill-safety).
4. Provider auth: app-password flows (Gmail/iCloud/Zoho/Yahoo) with guided UX data; Outlook OAuth loopback flow + refresh; Credential Manager integration.
5. JWZ threading; contacts/sender_stats bookkeeping; settings store.
6. Verify: **V-2.1** L3 suite green (delta/flag/expunge/reconnect/UIDVALIDITY cases); **V-2.2** nightly Dovecot job green (V-2.2 runs first time here, then stays nightly); **V-2.3** kill-app-mid-backfill recovery test; **V-2.4** real-account manual connect matrix (Gmail app-pw, iCloud app-pw, Zoho app-pw, Outlook OAuth) with evidence; **V-2.5** secrets-not-on-disk test (FR-2).

### Phase 3 — MVP features & core user flow — ☐
1. Three-pane main window (list virtualized, reader with sandboxed rendering §8 v1: sanitize+block-all-remote, no tracker list yet).
2. Unified inbox + per-account views; compose/reply/forward/drafts; Outbox + SMTP send + Sent APPEND (FR-16..18).
3. Offline path: read/search (FTS5) queue-send (FR-8, FR-9 v1 local-only).
4. Notch panel v1: window recipe (D-011), tray toggle + hotkey + hide-on-blur/Esc; triage list with archive/reply quick actions (FR-28 partial).
5. Keyboard navigation + command palette + shortcut overlay (FR-30); tray + badge (FR-31).
6. First-run AI opt-in dialog + model manager download/resume/verify (FR-20) — download only, no inference yet.
7. Verify: **V-3.1** L5 e2e: onboarding dialog → add mocked account → list → open thread → reply → send (mock SMTP) → offline toggle scenarios; **V-3.2** panel Alt-Tab/taskbar exclusion + hotkey toggle (FlaUI, manual evidence); **V-3.3** transparent-panel perf measurement on low-end hardware (D-011 gate — record decision outcome); **V-3.4** FTS search < 1 s @ seeded 100k benchmark; **V-3.5** real Gmail + Outlook manual send/receive round-trip.

### Phase 4 — Remaining approved features — ☐
1. Importance/Other: rule engine + counters + correction learning; split views (FR-11..12).
2. Follow-ups ("Waiting for Reply") + one-click follow-up draft (FR-13).
3. Subscription cleanup view: sender ranking, RFC 8058 POST (with DKIM-header caveat handling) / mailto fallback, bulk archive + Undo (FR-14).
4. Tracker blocking full pipeline (ammonia→lol_html→link-clean→sandbox iframe) + per-sender allowlist + badges (FR-15, FR-19).
5. AI inference: ChatEngine over llama-server sidecar; summarize + draft streaming; hardware tiering + override (FR-21..23); BYO keys (OpenAI/Anthropic/xAI) with fallback chain (FR-24); master switch (FR-25).
6. MCP server: rmcp tools, safety model, one-click client registration, audit log (FR-26..27).
7. Server-side search toggle merging uncached results (FR-9 completion).
8. Verify: **V-4.1** classification seeded-batch ≥ 90% rules-only + zero-egress test (FR-12); **V-4.2** follow-up clock-injection test; **V-4.3** unsubscribe flow against fixture server (POST only on click; Undo restores); **V-4.4** render pipeline: no script exec/no external request on tracked fixture, allowlist overrides; **V-4.5** AI golden-file suite (tiny GGUF) + latency budgets on MID hardware; **V-4.6** MCP suite: handshake (2026-07-28 + handshake-era fixture), read-only default, send-gate two-step, audit log; **V-4.7** real-client MCP check (Claude Desktop/Cursor) manual evidence.

### Phase 5 — Settings, onboarding, usability & accessibility — ☐
1. Settings surface: accounts, AI (tier/override/keys/master switch), MCP, privacy defaults, appearance, shortcuts/keymap themes, storage.
2. Onboarding polish: first-run sequence (AI opt-in → add first account → done state); provider help deep-links.
3. Accessibility pass: full keyboard map audit, UIA labels, focus rings, OS text scaling (NFR-4); hotkey configurability incl. IME-conflict note (FR-28).
4. Diagnostics: log viewer, session host list (FR-32 evidence), storage usage + cache prune controls.
5. Verify: **V-5.1** keyboard-only walkthrough of every FR flow; **V-5.2** diagnostics egress audit matches allowlist exactly; **V-5.3** fresh-VM manual onboarding ≤ 5 min (success criterion 1).

### Phase 6 — Stabilization & release — ☐
1. Performance & memory pass vs NFR-1/2 budgets; leak checks (long IDLE sessions, 24h soak).
2. Updater end-to-end (signed artifact, latest.json, OFF-by-default check, FR-34); NSIS installer finalization; icon/branding.
3. Code signing decision execution (Azure Artifact Signing if eligible, else OV) + SmartScreen guidance in README (D-016).
4. Crash/failure drills: DPAPI orphan, DB corruption restore, token refresh expiry (spec §11 table each demonstrated).
5. QA matrix (Win10 1809, Win10 22H2, Win11 23H2/24H2) manual pass; ARM64 stretch evaluation.
6. Docs final pass (README/FAQ/limits), 1.0 tag + release artifacts.
7. Verify: **V-6.1** all gates green on release candidate; **V-6.2** signed-update install path verified; **V-6.3** QA matrix evidence recorded.

## 3. Traceability matrix (FR → spec → phase task → verification)

| FR | Spec § | Phase.Task | Verification |
|---|---|---|---|
| FR-1 Add account | §5 | 2.4 | V-2.4, V-3.1 |
| FR-2 Credential storage | §5,§10 | 2.4 | V-2.5 |
| FR-3 Outlook OAuth | §5 | 2.4 | V-2.4 |
| FR-4 Ctrl+1..9 | §7,G | 3.5 | V-3.1 |
| FR-5 Connection state | §11 | 2.3 | V-2.1 |
| FR-6 Backfill | §4 | 2.3 | V-2.1, V-3.5 |
| FR-7 Incremental sync | §4 | 2.3 | V-2.1, V-2.2 |
| FR-8 Offline | §3,§4,§11 | 3.3 | V-3.1, V-2.3 |
| FR-9 Search | §3,§4 | 3.3, 4.7 | V-3.4 |
| FR-10 Attachments | §4.5 | 3.2 | V-3.1 |
| FR-11 Important/Other | §4,D-013 | 4.1 | V-4.1 |
| FR-12 On-device classify | D-013 | 4.1 | V-4.1 |
| FR-13 Follow-ups | §3 | 4.2 | V-4.2 |
| FR-14 Subscription cleanup | §4 | 4.3 | V-4.3 |
| FR-15 Tracking badges | §8 | 4.4 | V-4.4 |
| FR-16 Threads | §4 | 3.1 | V-3.1 |
| FR-17 Compose/drafts | §4 | 3.2 | V-3.1 |
| FR-18 SMTP send | §4.6 | 3.2 | V-3.5 |
| FR-19 Remote content | §8 | 4.4 | V-4.4 |
| FR-20 AI opt-in | §6.5 | 3.6 | V-3.1 |
| FR-21 Summarize | §6 | 4.5 | V-4.5 |
| FR-22 Draft assist | §6 | 4.5 | V-4.5 |
| FR-23 Tiering | §6.1 | 4.5 | V-4.5 |
| FR-24 BYO keys | §6.3 | 4.5 | V-4.5 |
| FR-25 AI switch | §6.6 | 4.5 | V-4.5 |
| FR-26 MCP server | §9 | 4.6 | V-4.6, V-4.7 |
| FR-27 MCP safety | §9,§10 | 4.6 | V-4.6 |
| FR-28 Notch panel | §7 | 3.4 | V-3.2, V-3.3 |
| FR-29 Main window | §2,G | 3.1 | V-3.1 |
| FR-30 Keyboard | G | 3.5 | V-3.1, V-5.1 |
| FR-31 Tray | G | 3.5 | V-3.1 |
| FR-32 Allowlist/no-telemetry | §10,D-015 | 5.4 | V-5.2 |
| FR-33 Encrypted cache | §5,§10 | 2.1 | V-2.5, Phase-6 task-4 failure drills |
| FR-34 Updates | §10,D-016 | 6.2 | V-6.2 |

## 4. Open verification queue (carried from Phase 0)

1. rusqlite `bundled` FTS5 presence → V-1.3 startup smoke test.
2. `windows-native-keyring-store` secret byte-encoding + >2.5 KB behavior → V-1.4.
3. llama.cpp stable release tag + asset inventory at build time (recorded per release).
4. io-imap 0.x stability vs async-imap fallback trigger (review at Phase 2 end).
5. rmcp backward-compat against real Claude Desktop/Cursor builds → V-4.7.
6. UglyEmail/EasyPrivacy blocklist licensing → legal check before bundling (D-014).
7. Outlook.com app-registration publisher-verification burden → during 2.4.
