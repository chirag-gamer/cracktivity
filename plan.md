# plan.md — cracktivity living plan

Consolidates the document inventory and the phased task list. Updated continuously; statuses: ☐ todo · ◐ in progress · ☑ done (with verification evidence noted). FR IDs reference [prd.md](prd.md); § references [spec.md](spec.md).

---

## 1. Document inventory

| Doc | Role | Status |
|---|---|---|
| README.md | User-facing front page (structure mirrors chirag-gamer/button) | ☑ Phase 0 |
| prd.md | Goals, user stories, FR-1..34 + AC, NFRs, non-goals | ☑ Phase 0 |
| spec.md | Architecture, data model, sync/AI/MCP/security, test strategy | ☑ Phase 0 |
| decisions.md | Append-only decision ledger (living; D-001..D-034) | ☑ Phase 0 (living) |
| AGENTS.md | Agent contract: source of truth, safety, workflow, gates | ☑ Phase 0 |
| plan.md | This file — tasks, phases, traceability | ☑ Phase 0 (living) |

## 2. Phases

### Phase 0 — Discovery, research, docs, decisions — ☑ done 2026-11-04
- ☑ 10 read-only research subagents in 2 batches (AI models · UI/framework · email protocols · Windows platform · UX/Bloop/README · MCP · Tauri facts · AI integration · tracker blocking · testing/CI).
- ☑ Main-agent verification pass: Bloop.email, MCP spec 2026-07-28 + rmcp 3.8.0, tauri 2.12.5, keyring/windows-native-keyring-store, rusqlite-bundled features, MCP versioning page.
- ☑ Canonical docs written (above). Key decisions D-001..D-018 recorded, incl. explicitly provisional items (D-009 io-imap, D-006 FTS5 build-flag note, llama.cpp release-tag resolution).

### Phase 1 — Repo, tooling, scaffold, tests & CI — ✅ (2026-11-04)
1. `git init`; commit Phase 0 docs; `.gitignore` (target/, node_modules/, dist/, models/, secrets). **done**
2. Scaffold Tauri 2.12 + React + TS + Vite + pnpm workspace; `tauri.conf.json` v2 skeleton (NSIS target, capabilities files); Fluent UI v9 + theme provider (light/dark follows system). **done**
3. Rust workspace layout per spec §2; empty modules compile; `tauri::test` harness wiring (feature `test`). **done** — plus the comctl32 v6 manifest fix for test binaries (D-020).
4. Toolchain gates: cargo fmt/clippy, eslint+prettier+tsc, vitest hello-test, CI `ci.yml` on `windows-latest` (lint + L1 + L2 jobs, rust-cache + pnpm cache). **done**
5. Verify: **V-1.1** CI green on windows-latest; **V-1.2** `pnpm tauri dev` opens main window on local Win11 (evidence: dev binary built and ran with the window event loop up — process verified alive, 42 MB working set, no crash for the observation window; visual on-screen confirmation is the user's quick check on the next dev run); **V-1.3** FTS5 startup smoke test passes (D-006 note) — `fts5_present_and_file_is_encrypted` green; **V-1.4** keyring write/read round-trip test passes on Windows — `byte_round_trip_and_persistence_local` + `oversized_secret_is_rejected_not_truncated` green (byte-exact incl. NUL/0xFF; >2560 B rejected, not truncated).

### Phase 2 — Core model, boundaries, persistence & recovery — ✅ (2026-11-04)
1. SQLCipher schema + migrations (spec §3) with DPAPI-wrapped DEK; store module + tests. **done** (schema, DEK DPAPI wrap, `open_creates_schema_and_reopens_clean`, `concurrent_writer_lock_times_out_then_recovers`, `missing_dek_entry_yields_recoverable_error_on_create`, `reopen_after_dek_entry_deleted_is_orphan_error`)
2. `MailTransport` trait; io-imap adapter (0.8.1, D-019); scripted IMAP mock harness (L3) incl. IDLE/reconnect/UIDVALIDITY scenarios. **done**
3. Sync engine per spec §4 (backfill, steady-state, flag/expunge reconcile, body lazy-fetch, batching, WAL kill-safety). **done** (11-test L3 suite)
4. Provider auth: provider presets with verified guided-UX data (D-021); Outlook OAuth loopback + PKCE + refresh (D-022); Credential Manager integration (`providers::set/get/delete_account_secret`). **done** — Outlook OAuth pending only the project's Entra app registration (client id constant is empty and fails loudly; V-2.4 is the manual gate).
5. JWZ threading; contacts/sender_stats bookkeeping; settings store. **done**
6. Verify: **V-2.1** L3 suite green — 48 lib + 1 integration tests, 0 failures (delta/flag/expunge/reconnect/UIDVALIDITY cases covered); **V-2.2** ⚠ deferred: no Docker on the dev machine — Dovecot container + gated L4 `#[ignore]` tests + nightly workflow are Phase-3 entry work; **V-2.3** `kill_mid_backfill_resumes_without_duplicates` green (11-test sync suite); **V-2.4** code paths ready (loopback redirect + state binding fixed per D-023); manual connect matrix needs the user's real accounts (and the Entra registration for Outlook); **V-2.5** `secrets_never_reach_disk` green (FR-2).

### Phase 3 — MVP features & core user flow — ◐ (core pillars done 2026-11-04; D-025 scope split)
1. Three-pane main window (list virtualized, reader with sandboxed rendering §8 v1: sanitize+block-all-remote, no tracker list yet). **done** — Fluent v9 shell + @tanstack/react-virtual list (10k-row windowing L1-tested) + sandboxed-iframe reader w/ CSP-meta srcdoc; render pipeline = single ammonia pass (D-026): scripts/iframes/handlers stripped, all remote images blocked+counted, utm_*/click-ids stripped (4 render tests).
2. Unified inbox + per-account views; compose/reply/forward/drafts; Outbox + SMTP send + Sent APPEND (FR-16..18). **partial** — unified + per-account views done (L1+L2+L3); Important/Other triage pulled forward per D-025 (rules engine + corrections teaching, 7 unit + 1 sync-integration tests); **compose/SMTP/Sent APPEND remain open** (next session; `note_sent` still unused).
3. Offline path: read/search (FTS5) queue-send (FR-8, FR-9 v1 local-only). **done for read/search** — all UI queries answered from the encrypted cache offline (FTS5 w/ quoted prefix queries, user input never parsed as FTS syntax); queue-send lands with compose (offline flag edits are honestly rejected, not queued — D-026).
4. Notch panel v1: window recipe (D-011), tray toggle + hotkey + hide-on-blur/Esc; triage list with archive/reply quick actions (FR-28 partial). **done** — full D-011 recipe (no_redirection_bitmap, WS_EX_TOOLWINDOW re-assert on show, CSS-only slide, 150 ms blur-debounce hide), tray (menu + left-click toggle, tooltip badge deferred), Ctrl+Shift+Space hotkey, panel triage list w/ e/u/Enter/Esc; panel hides fully (no resting strip — D-026).
5. Keyboard navigation + command palette + shortcut overlay (FR-30); tray + badge (FR-31). **done** — j/k/Enter/e/u/s///?/Ctrl+K/Ctrl+1..9 over window-capture keydown (typing-guard); palette + cheat sheet; tray badge overlay deferred (tooltip carries count; Windows tray overlays need icon generation — Phase 4/5 polish).
6. First-run AI opt-in dialog + model manager download/resume/verify (FR-20) — download only, no inference yet. **open** (deferred with compose per D-025).
7. Verify: **V-3.1** L5 e2e **seeded legs GREEN 4/4** (2026-11-05 local Win11: hydration from encrypted cache → j/keyboard open reader → Ctrl+1 account switch → `/`+FTS5 search; WDIO + @wdio/tauri-service 1.5.1, v9-pinned, external tauri-driver 2.1.0, `--e2e-seed` idempotent); onboarding-dialog/add-account-UI and send/reply legs deferred with compose (D-025); **V-3.2** FlaUI Alt-Tab/hotkey — manual gate, recipe implemented; **V-3.3** low-end transparent-panel perf — open (D-011 gate); **V-3.4** FTS < 1 s @ 100k — open (benchmark harness TODO; L1/L3 cover correctness); **V-3.5** real Gmail/Outlook round-trip — open (needs user accounts; Outlook also blocked on the Entra registration, V-2.4).

### Phase 4 — Remaining approved features — ✅ core done 2026-11-05 (V-4.5 golden-file + V-4.7 real-client evidence outstanding)
0. Phase-3 carry-in prerequisites: compose/reply/drafts + SMTP send + Sent APPEND (FR-17/18) and first-run AI opt-in + model manager (FR-20). **done** — lettre submission (465 implicit TLS / 587 STARTTLS), Sent APPEND + local mirror, outbox statuses with retryable failures; onboarding dialog (checkbox ON) streams the pinned-model download.
1. Follow-ups (FR-13). **done** — driver syncs the Sent folder (headers-only), `waiting_for_reply` is a pure `now`-injected query (V-4.2 covered by `followups_respect_injected_clock_and_clear_on_reply` + the L3 `sent_sync_feeds_the_followup_view`), the view + one-click pre-filled draft are live; threshold is user-set 1/2/3/5/7, default 3.
2. Subscription cleanup (FR-14). **done** — `messages.opened_at` latch (V3) credits `sender_stats.opened` exactly once; ranking never-opened-first; RFC 8058 POST fires ONLY on click (`one_click` refuses non-POST-advertised, insecure, redirecting endpoints; **V-4.3 wire contract pinned against a real TLS fixture server**); mailto fallback via ShellExecuteW; bulk archive returns undo refs and Undo MOVEs back server-side (L1 click-isolation test).
3. Tracker blocking full pipeline (FR-15/19). **done** — ammonia → lol_html (`data-blocked-src` placeholder, 1×1-pixel + curated-domain heuristics → `tracking_count`, srcset always dropped) → sandboxed iframe with per-render CSP; per-sender allowlist (V5 `sender_stats.show_images`) + per-message override route allowed images through the loopback fetch proxy (no referer/cookies, token-gated, 10 MB cap); tracker pixels stay blocked even when allowed. **V-4.4** covered by 9 render tests.
4. AI inference (FR-21..25). **done** — model manager with frozen rev+size+sha256 pins (D-031), resumable Range downloads with checksum enforcement (fixture-tested); tiering from RAM/cores with override; llama-server sidecar (b12110 flags verified live, D-030); ChatEngine over llama-server + OpenAI-compat + Anthropic native (L7 wire-contract tests for both SSE shapes); summarize (map-reduce within the 8k budget, quote stripping) and draft streaming through Tauri Channels; BYO keys in Credential Manager with the FR-24 local-fallback notice; master switch kills the sidecar. **V-4.5 golden-file run with a real tiny GGUF + latency budgets remain open** (sidecar binary is not committed; `scripts/fetch-llama.mjs` fetches it).
5. MCP server (FR-26/27). **done** — rmcp 3.9 stdio server (`cracktivity.exe --mcp`, stdout protocol-only), 8 tools; read-only default (**V-4.6: handshake + toolset + gate tests green over an in-process duplex**), two-step gated send with a single-use confirm token bound to the content fingerprint, per-hour rate limit, append-only audit rows, code redaction (6–8 digit runs), nonce-fenced bodies with escape-proofing, Spam/Trash excluded structurally (inbox-role joins); one-click registration for Claude Desktop (incl. MSIX LocalCache probe), Claude Code, Codex (TOML), Cursor with .bak backups and unknown-key preservation (L1-tested).
6. Server-side search (FR-9 completion). **done** — `uid_search` on the transport (mock + io-imap), off-runtime expansion ingesting headers under a short lock, `search.also_server` setting, results marked uncached (SearchRow.has_body → "on server — not cached" in the list).
7. Importance counters + V-4.1. **done** — 50-message seeded batch classifies 100% by rules alone (`seeded_batch_classifies_at_least_90_percent`, with the ≥90% gate as the AC); zero egress is structural (pure classify function, no network path).
8. Frontend. **done** — Follow-ups view, Cleanup view (unsubscribe/archive/undo), Composer (autosave, Ctrl+Enter, streaming AI draft, failure reason), Settings (AI master switch/tier/engine/keys/download, MCP gate + registrations + audit, follow-up days, server search), Onboarding dialog (once, checkbox ON), reader AI bar + tracking badge + Show images; `c` compose / `r` reply keys; 37 L1 tests green.
9. Verify: **V-4.1 ✅** · **V-4.2 ✅** · **V-4.3 ✅** · **V-4.4 ✅** · **V-4.6 ✅** (all automated, evidence in test names) · **V-4.5** open (golden-file + latency on real hardware — needs the fetched sidecar + a tiny GGUF) · **V-4.7** open (manual: register into Claude Desktop/Cursor and exercise the tools).

### Phase 5 — Settings, onboarding, usability & accessibility — ✅ done 2026-11-06 (V-5.3 fresh-VM manual run remains user-side; review loop CONVERGED-CLEAN: 2 reviewers round 1 → fixes D-046 → final verifier round 2)
1. Settings surface: accounts, AI (tier/override/keys/master switch), MCP, privacy defaults, appearance, shortcuts/keymap themes, storage. **done** — AI/MCP/general sections (master switch, tier+engine selects, BYO keys, model download/delete, mcp.allow_send gate + registrations + audit tail, follow-up days, server-search toggle) + Appearance section (theme choice system/light/dark, `appearance.theme` + localStorage mirror, D-039; keymap scheme cracktivity/Gmail-style per FR-30 AC, D-041-adjacent `appearance.keymap`; panel hotkey config with validation + IME note, FR-28/D-045) + Diagnostics section (hosts, storage, log viewer, prune). Signatures: not an FR in prd.md — deliberately absent.
2. Onboarding polish: first-run sequence (AI opt-in → add first account → done state); provider help deep-links. **done (core)** — dialog shows once (onboarding.completed), checkbox ON streams the model download, flows into add-account; provider deep-links already in the add-account dialog (D-021); dialog now carries `aria-label` (a11y pass).
3. Accessibility pass: full keyboard map audit, UIA labels, focus rings, OS text scaling (NFR-4); hotkey configurability incl. IME-conflict note (FR-28). **done (2026-11-06)** — label associations fixed (settings selects), onboarding dialog + star toggle `aria-label`s, search-results `role="listbox"`, log viewer `role="log"`; theme surface repaints via `data-theme` + `color-scheme` with no flash (visible:false + Rust background_color before show, D-040); OS text scaling is automatic via tao PerMonitorV2, screen-reader support automatic via WebView2 — semantics-only work needed (D-041); live OS-scaling reflow + NVDA walkthrough are recorded manual checks.
4. Diagnostics: log viewer, session host list (FR-32 evidence), storage usage + cache prune controls. **done (2026-11-06)** — in-memory log ring buffer (platform/applog, D-042) wired at AI-download/sidecar/SMTP-send/sync failure points + `diagnostics_logs`/`diagnostics_clear_logs`; `prune_cache` deletes partial model downloads (mail DB intentionally not prunable — D-043).
5. Verify: **V-5.1 ✅** e2e keyboard-first legs green (palette→settings, label association, Tab focus travel into appearance controls; j/k/e/u/s/c/r keymap contract at L1 incl. the new Gmail-style `y` alias); **V-5.2 ✅** `static_inventory_covers_all_runtime_egress` asserts exact two-sided set equality between `netlog::STATIC_EGRESS_HOSTS` and runtime-derived sources (parsed endpoint URLs, `providers::ALL` presets) — one-sided version corrected in round 1 (D-046); **V-5.3** open (fresh-VM manual onboarding ≤ 5 min — user-side).

### Phase 6 — Stabilization & release — ☐ (packaging prep done: NSIS per-user + embedBootstrapper pinned in tauri.conf.json; llama fetch script added)
   - OPEN (review round 1, P2): `bundle.resources` lists only `resources/README.txt` — the fetched `llama-server.exe` is NOT bundled, so installers have no sidecar. Adding a missing-resource entry fails every build, so the line lands with the Phase-6 key/binary ceremony: fetch script → add `"resources/llama-server.exe"` (+ the DLL closure it needs — b12110 splits the server into a stub exe + `llama-server-impl.dll` + ggml/libomp DLLs, ~50 MB total, D-037) → build.
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
| FR-8 Offline | §3,§4,§11 | 3.3 | V-3.1, V-2.3 — **read/search offline verified** (L1/L2, all queries cache-only); queue-send lands with compose |
| FR-9 Search | §3,§4 | 3.3, 4.7 | V-3.4 (open); **done local + server toggle** (FTS5 L1–L3; server expansion off-runtime, uncached rows marked) |
| FR-10 Attachments | §4.5 | 3.2 | V-3.1 |
| FR-11 Important/Other | §4,D-013 | 3.2 (pulled fwd, D-025), 4.1 | V-4.1 (batch ≥90%); **rules + corrections live** (7 unit + sync-integration tests; AC cases 1–3 covered) |
| FR-12 On-device classify | D-013 | 3.2 (pulled fwd), 4.1 | V-4.1 (zero-egress audit open); rules-only, no network path exists in classify |
| FR-13 Follow-ups | §3 | 4.2 | **V-4.2 ✅** (clock-injected query + L3 sent-sync test); view + pre-filled draft live |
| FR-14 Subscription cleanup | §4 | 4.3 | **V-4.3 ✅** (TLS fixture wire contract; POST only on click — L1; Undo via server MOVE-back) |
| FR-15 Tracking badges | §8 | 4.4 | **V-4.4 ✅** (pixel+domain heuristics; badge; allowlist keeps trackers blocked) |
| FR-16 Threads | §4 | 3.1 | V-3.1 — **threaded list/reader done** (JWZ from Phase 2; list+reader L1/L2 tested) |
| FR-17 Compose/drafts | §4 | 3.2, 4.0 | **done** (drafts CRUD + autosave + outbox; L1 composer tests; kill-safe restore via drafts table) |
| FR-18 SMTP send | §4.6 | 4.0 | **done** (lettre 465/587, Sent APPEND + local mirror; live submit is V-3.5/L4) |
| FR-19 Remote content | §8 | 4.4 | V-4.4 |
| FR-20 AI opt-in | §6.5 | 4.0 | **done** (once-only dialog, checkbox ON; resumable+verified download, fixture-tested) |
| FR-21 Summarize | §6 | 4.5 | streaming + map-reduce + budgets (unit-tested); **V-4.5 golden-file run open** |
| FR-22 Draft assist | §6 | 4.5 | streams into the composer, never auto-sends (L1); **V-4.5 golden-file run open** |
| FR-23 Tiering | §6.1 | 4.5 | **done** (RAM/cores → LOW/MID/HIGH, override, tested boundaries) |
| FR-24 BYO keys | §6.3 | 4.5 | **done** (keys in Credential Manager; fallback→local with visible notice; SSE contract tests) |
| FR-25 AI switch | §6.6 | 4.5 | **done** (off → sidecar killed + affordances hidden, L1-tested) |
| FR-26 MCP server | §9 | 4.6 | **V-4.6 ✅** (duplex handshake + toolset); **V-4.7 real-client check open** |
| FR-27 MCP safety | §9,§10 | 4.6 | **V-4.6 ✅** (read-only default, two-step single-use token gate, rate limit, audit, redaction, fencing) |
| FR-28 Notch panel | §7 | 3.4, 5.1 | V-3.2 (FlaUI manual gate — recipe implemented), V-3.3 (open); **panel v1 done** (D-011 recipe + triage UI, L1-tested); **hotkey configurable** (D-045/D-046: validation against the pinned parser's token set + IME note, `set_panel_hotkey`, L1+startup registration) |
| FR-29 Main window | §2,G | 3.1, 5.1 | V-3.1 — **three-pane shell done** (virtualized list, sandboxed reader); **theme user-selectable** system/light/dark, no-flash startup + instant switch (D-039/D-040, L1 + e2e) |
| FR-30 Keyboard | G | 3.5, 5.1 | **V-5.1 ✅** (e2e keyboard-first legs); full keymap live + **keymap scheme selectable** (cracktivity/Gmail-style `y` alias, L1 + overlay labels) |
| FR-31 Tray | G | 3.5 | V-3.1 — **tray done** (menu: Open/Check/Quit; left-click toggles panel); badge overlay deferred (tooltip carries count) |
| FR-32 Allowlist/no-telemetry | §10,D-015 | 5.4 | **V-5.2 ✅** (static inventory pinned by tests, D-044); hosts list + log ring buffer + prune live (D-042/D-043) |
| FR-33 Encrypted cache | §5,§10 | 2.1 | V-2.5, Phase-6 task-4 failure drills |
| FR-34 Updates | §10,D-016 | 6.2 | V-6.2 |

## 4. Open verification queue (carried from Phase 0)

1. rusqlite `bundled` FTS5 presence → **resolved** V-1.3: FTS5 present and on-disk file non-plaintext in the bundled SQLCipher build.
2. `windows-native-keyring-store` secret byte-encoding + >2.5 KB behavior → **resolved** V-1.4: byte-exact round-trip; >2560-byte secrets rejected (never truncated).
3. llama.cpp stable release tag + asset inventory at build time (recorded per release).
4. io-imap 0.x stability vs async-imap fallback trigger → **resolved for Phases 2–3** (D-019/D-026): io-imap 0.8.1 chosen, blocking client over rustls; Phase 3 shipped on the 90-s polling fallback (`idle_once` = NOOP poll). The eventful `watch_mailbox` IDLE driver remains future work — re-review when it lands.
5. rmcp backward-compat against real Claude Desktop/Cursor builds → V-4.7.
6. UglyEmail/EasyPrivacy blocklist licensing → legal check before bundling (D-014).
7. Outlook.com app-registration publisher-verification burden → **partially resolved** (D-022): research confirms a registered client id works (Thunderbird precedent); the project must create its own Entra registration — user action, tracked for V-2.4.
