# decisions.md — append-only decision ledger

Rules: this file is append-only. Never edit or delete past entries. To change a decision, append a new entry that supersedes the old one and reference its ID. Each entry: ID, date, status, context, decision, alternatives, consequences. Statuses: `accepted`, `provisional` (pending local verification), `superseded`.

---

## D-001 — 2026-11-04 — Framework: Tauri 2 over Electron — accepted

- **Context:** Need a Windows 10/11 desktop app with a borderless top-edge dropdown panel, tray, low memory, and a Rust core (mail engine, crypto, MCP). Verified: tauri 2.12.5 (2026-10-21) is current; `noRedirectionBitmap` window option landed in 2.12.0.
- **Decision:** Tauri 2.12.x + WebView2. UI in React + TypeScript + Fluent UI React v9 (native-feeling, Windows-native theming, keyboard focus control we own in a borderless window).
- **Alternatives:** Electron (348 MB more on disk, bundles Node.js = extra attack surface for a privacy app; transparent/alwaysOnTop works but heavier); WinUI 3 (native but poor fit for HTML mail rendering + web-tech UI velocity).
- **Consequences:** Transparent WebView2 costs perf (confirmed by shipped apps) — the always-open main window stays opaque; only the notch panel may be transparent, with an opaque fallback if it measures badly. `skipTaskbar` alone is unreliable on Windows; we must re-assert `WS_EX_TOOLWINDOW` on the raw HWND after every `show()`.

## D-002 — 2026-11-04 — Design reference: Bloop.email is the direct analog — accepted

- **Context:** Owner asked to "scrape ideas from bloop email." Verified bloop.email exists: native macOS client, $4.99 one-time, notch dropdown for new mail, Important/Other, Waiting for Reply, subscription cleanup, on-device AI (Apple Intelligence or BYO OpenAI/Claude key), built-in MCP (one-click for Claude/Codex/Cursor; no Spam/Trash visibility; no permanent delete; send opt-in), no middleman servers, Keychain storage, offline. macOS-only.
- **Decision:** Treat Bloop.email as the primary external validation + feature checklist for the Windows port. Parity targets: its triage trio (Important/Other, follow-ups, subscription cleanup), notch dropdown, MCP safety model. Do not copy name/branding; note bloop.ai (code search) is unrelated.
- **Consequences:** PRD feature set mirrors the Bloop pillar list, adapted to Windows conventions and a CPU-only local model.

## D-003 — 2026-11-04 — Local AI: llama.cpp `llama-server` prebuilt sidecar, not FFI — accepted

- **Context:** Summarization/drafting/classification on CPU only. Options: link llama.cpp via `llama-cpp-2` FFI (crate itself warns "not safe… do not use for tasks where UB is not acceptable"), WASM (wllama, slower), or run `llama-server.exe` as a child process speaking the OpenAI-compatible HTTP API on loopback.
- **Decision:** Ship the official llama.cpp prebuilt `win-cpu-x64` zip's `llama-server.exe` as a bundled resource, spawned via plain `std::process::Command` (no shell plugin/capability needed), bound to `127.0.0.1:<ephemeral port>` with a per-run random API key. Pin a stable `vX.Y.Z` release tag; never pin a nightly `bNNNNN`. MIT license permits bundling.
- **Alternatives:** FFI (crash/OOM isolation lost, build chain burden, UB warning); wllama WASM (slow); Ollama (requires separate user-installed daemon — unacceptable for a consumer installer).
- **Consequences:** Process isolation for a 4B prefill spike; independent llama.cpp upgrade cadence; one OpenAI-wire client covers llama-server AND BYO keys. Version-numbering sources disagreed (v0.2.0–v0.8.0 claims) — the build resolves the current stable tag at build time and records it in the release notes.

## D-004 — 2026-11-04 — Model tiers: Qwen family + Phi-4-mini, Apache/MIT only bundled — accepted

- **Context:** Must run on 4 GB RAM laptops without a GPU. KV cache, not weights, is the real RAM driver (~0.9 GB at 8k ctx FP16 for 1.7B; halve with q8_0 KV). Sub-1B models are unreliable for classification-style JSON output; 3–4B models are the writing sweet spot.
- **Decision (two-model strategy, never both large resident):**
  - Classify tier (resident when AI on): **Qwen3-0.6B** Q4_K_M, ctx 2048 — assisted labeling only; deterministic rules do the heavy lifting (D-013).
  - Write tier (loaded on demand): LOW hardware (<6 GB RAM): **Qwen2.5-1.5B**; MID (8 GB): **Qwen3-1.7B**; HIGH (16 GB+): **Qwen3-4B-Instruct-2507**, fallback **Phi-4-mini** (MIT).
  - Bundled-by-download only Apache-2.0/MIT models (Qwen, Phi, SmolLM). Gemma/Llama models offered as optional downloads with their own license terms surfaced at download.
- **Alternatives:** single 4B model everywhere (fails 4–8 GB RAM budget); Gemma default (license pass-through obligations).
- **Consequences:** Models are downloaded at first run from pinned HuggingFace commits with SHA-256 verification, stored under app local-data dir. Latency budget: drafting ~9–19 s, summarizing ~14–30 s (warm prefix) — streaming UX required (see spec §6.5).

## D-005 — 2026-11-04 — AI opt-in at first run, ON by default — accepted

- **Context:** Owner requirement: "prompt asking the user to opt-in for this local AI model upon installation (on by default)."
- **Decision:** A first-run dialog (post-install, first app start) presents on-device AI with a pre-checked enable box, plain-language description, model size and disk/RAM footprint. Declining skips model download entirely and is changeable later in Settings. No network call happens before the user finishes this dialog except provider connections the user initiates.
- **Consequences:** Cannot ship model weights inside the installer unconditionally; download manager (resume, SHA-256 pin, delete) is an MVP component.

## D-006 — 2026-11-04 — Storage: rusqlite (bundled SQLite) + WAL + FTS5; split data dirs — accepted

- **Context:** Offline cache must scale to hundreds of thousands of messages with sub-second search. `tauri-plugin-sql` (sqlx) adds async machinery we don't need from Rust.
- **Decision:** Rust-side `rusqlite` with `features = ["bundled"]` (SQLite 3.54.x). WAL journal mode. FTS5 external-content virtual table over message text. **Status note:** bundled-FTS5 is per libsqlite3-sys build script (high confidence, subagent-verified); Phase 1 adds a startup smoke test (`CREATE VIRTUAL TABLE … USING fts5`) and fails fast if absent. If absent, switch to `bundled-sqlcipher-vendored-openssl` which we need anyway for D-007 encryption.
- **Alternatives:** sqlx/tauri-plugin-sql; LMDB; custom index.
- **Consequences:** Settings/small state → `%APPDATA%` (roaming); DB, attachments, models → `%LOCALAPPDATA%` (never roam multi-GB data).

## D-007 — 2026-11-04 — Secrets: Windows Credential Manager + DPAPI-encrypted SQLCipher DB — accepted

- **Context:** Verified `keyring` 4.2.0 / `windows-native-keyring-store` 1.1.0 maps to Credential Manager generic credentials (CredWriteW/CredReadW). Credential Manager caps the blob at **2560 bytes** (CRED_MAX_CREDENTIAL_BLOB_SIZE) — fine for passwords/app-passwords and OAuth refresh tokens, not for big material. Thunderbird's blank-primary-password storage is decryptable by anyone with file access — explicitly rejected as a pattern.
- **Decision:** Per-secret storage in Credential Manager (persist = Local). The SQLite mail cache is encrypted with SQLCipher; a random 32-byte DEK is generated per profile, wrapped with DPAPI (`CryptProtectData`, user scope) and the wrapped key stored in Credential Manager. Tokens never enter webview localStorage; the mail-rendering context has no IPC access.
- **Alternatives:** plain SQLite + filesystem ACLs (insufficient); Credential Manager for everything (2560-byte cap + multi-thread caveats).
- **Consequences:** Admin password *reset* (not user password change) can orphan DPAPI-wrapped keys → recovery path documented in spec §11 (re-sync from server; credentials survive in Credential Manager). Do not share one Credential Manager entry across threads.

## D-008 — 2026-11-04 — Provider auth: app passwords first, Outlook.com OAuth, Gmail OAuth deferred — accepted

- **Context:** Verified 2025–2026 status: Gmail basic auth off (2025-03-14); Gmail OAuth needs restricted scope `https://mail.google.com/` → Google verification + CASA Tier 2 (~$540–1,000 via lab, 4–12 weeks, annual re-verification); Gmail app passwords work for personal accounts with 2SV. Outlook.com personal: basic auth off since 2024-09-16, app passwords are NOT a workaround → OAuth2 mandatory. iCloud: app-specific passwords only (no third-party IMAP OAuth). Zoho: app passwords or XOAUTH2. Yahoo: app passwords (OAuth requires commercial agreement).
- **Decision (v1):** App-password onboarding for Gmail (personal), iCloud, Zoho, Yahoo. OAuth2 XOAUTH2 for Outlook.com via our own Entra app registration with loopback redirect (`http://127.0.0.1:<port>`), scopes `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`. Gmail OAuth + CASA deferred (Phase 6+/post-1.0, revisit when revenue/structure justifies it).
- **Alternatives:** Gmail OAuth first (blocks v1 for months + cost); supporting only IMAP-noauth providers (excludes Outlook).
- **Consequences:** Honest onboarding UX per provider (step-by-step app-password instructions with deep links to provider security pages). Workspace Gmail and Advanced Protection users are out of v1 scope — stated in README/PRD, not hidden.

## D-009 — 2026-11-04 — IMAP stack: io-imap primary, async-imap fallback — provisional

- **Context:** Verified crates: `io-imap` (pimalaya) 0.7.1 — IDLE + CONDSTORE/QRESYNC + SORT/THREAD + XOAUTH2, sans-IO core (unit-testable), but 0.x churn; `async-imap` 0.12 — most-downloaded, solid IDLE, weaker QRESYNC. Neither is battle-complete; we own reconnection/UIDVALIDITY/token-refresh logic regardless.
- **Decision:** Build the sync engine on `io-imap` 0.7.x behind a thin `MailTransport` trait so the engine never depends on crate specifics. If 0.x churn bites, swap to `async-imap` with a QRESYNC polyfill (flag-diff fallback) without touching engine logic.
- **Consequences:** Spec §4 sync algorithm must work WITHOUT QRESYNC (Outlook likely lacks it): flag/expunge reconciliation via periodic `UID SEARCH`/flag-diff snapshots is a first-class path, not an afterthought.

## D-010 — 2026-11-04 — Threading: JWZ client-side, provider thread IDs when present — accepted

- **Context:** JWZ threading (References/In-Reply-To + subject normalization incl. locale prefixes) is the field-proven algorithm; Gmail exposes `X-GM-THRID`.
- **Decision:** Implement JWZ over parsed headers in Rust; prefer provider-native thread IDs when available; subject-normalize fallback for orphans.
- **Consequences:** Thread identity is local; consistent across providers; no dependency on server threading.

## D-011 — 2026-11-04 — Notch dropdown recipe: undecorated top-center panel + WS_EX_TOOLWINDOW + CSS animation — accepted

- **Context:** Verified prior art: Windows Terminal quake mode (top-docked, hidden from taskbar AND Alt-Tab, hotkey toggle, hide-on-focus-lost); WTQ settings; QuakeShell (Electron reference). Verified pitfalls: z-order vs fullscreen; hotkey fails when elevated windows hold focus (fixable only via UIAccess + signing — accepted edge case); Win32 window animation is broken in 3 tested ways; tao rewrites extended styles on show() wiping external bits; `skipTaskbar` alone unreliable.
- **Decision:** Panel = `WebviewWindowBuilder`: `decorations(false), transparent(true), always_on_top(true), skip_taskbar(true), shadow(false), resizable(false), visible(false), no_redirection_bitmap(true)` (tauri ≥ 2.12). On every show: compute top-center from `current_monitor()` scale-aware, `set_position`, re-assert `WS_EX_TOOLWINDOW` (0x80) via `SetWindowLongPtrW` on `window.hwnd()`, then `show()`. Show triggers: tray left-click + global hotkey (default `Ctrl+Shift+Space`, configurable). Hide: `WindowEvent::Focused(false)` (with 150 ms debounce + own-window exclusion), Esc in webview. Animation: CSS translateY slide inside a fixed-size window; never animate the HWND. Collapsed resting state uses `set_ignore_cursor_events(true)`.
- **Alternatives:** animate by moving window (rejected — WT evidence); menu-on-left-click tray only (slower than hotkey for triage).
- **Consequences:** Fullscreen-exclusive apps can cover the panel (accepted, matches WT); transparent-panel perf must be measured on a low-end Win10 box in Phase 3, else fall back to opaque panel with themed background.

## D-012 — 2026-11-04 — MCP server: rmcp official SDK, stdio transport, self-spawn `--mcp` — accepted

- **Context:** Verified: current MCP spec is **2026-07-28** (per-request version negotiation, mandatory `server/discover`, backward compatibility with handshake-era 2025-11-25 clients); `rmcp` 3.8.0 is the **official** Rust SDK (stdio via `transport-io`, `#[tool]` macros, schemars JSON Schema). Shipping pattern confirmed across desktop apps: register the app binary itself as the MCP command with a hidden flag. Bloop's safety model confirmed on its site: agents never see Spam/Trash/sign-in codes, never permanently delete, send opt-in.
- **Decision:** `cracktivity.exe --mcp` runs an rmcp stdio server (GUI init skipped; all logging to stderr; stdout is protocol). Tool set: `list_inboxes`, `list_recent_emails`, `search_emails`, `read_thread`, `summarize_thread`, `create_draft`, and a gated `preview_send` → `send_draft(confirm_token)` pair. Read-only by default; sends/deletes disabled until enabled in Settings → MCP. Bodies returned as plain text, fenced as untrusted data with a per-call random nonce. Append-only local audit log. One-click config writers for Claude Desktop / Claude Code / Codex / Cursor (mcp.json merge with backup). No HTTP transport in v1 (no ports opened).
- **Alternatives:** Streamable HTTP loopback server (network surface, OAuth burden — rejected for v1); hand-rolled JSON-RPC (wasteful given official SDK).
- **Consequences:** Test matrix must cover both spec eras (rmcp claims compat — verify against real Claude Desktop + Cursor in Phase 4). GUI-subsystem binary + redirected stdio needs an early smoke test.

## D-013 — 2026-11-04 — Importance: deterministic rules first, tiny learned layer second — accepted

- **Context:** Verified: Edison uses Gmail Primary when available + proprietary ML otherwise; Nextcloud Mail proves on-device ML works; practical blueprint (Wiggins) = embed/featurize + logistic regression on a few hundred hand-labeled mails. Sub-1B LLMs are unreliable for classification (subagent-verified practitioner reports).
- **Decision:** Rule engine first: bulk headers (`List-Unsubscribe`/`List-Id`/`Precedence: bulk`), To-vs-Cc position, prior two-way reply history, reply-to-my-thread, calendar invite (.ics), security-code patterns → Important; everything bulk-shaped → Other. User corrections (move between buckets) update per-sender/per-pattern counters locally. Optional learned layer: logistic regression trained on-device from the user's own archive + corrections. Gmail Primary/Category labels used as weak supervision when present. The 0.6B model assists only with language detection / edge-case newsletter detection; never gates delivery.
- **Alternatives:** LLM classification (slow, unreliable at this size); cloud classifier (violates core privacy).
- **Consequences:** Classification is instant, offline, and free on all hardware tiers. "Other" is review-later, never trash.

## D-014 — 2026-11-04 — Tracker blocking: default-deny remote content, local sanitization, no proxy — accepted

- **Context:** Verified: Hey and Proton block trackers via REMOTE proxies (middleman — excluded by our privacy core); Thunderbird/FairEmail prove local default-deny + per-sender allow works; UglyEmail maintains an email-tracker identifier list; EasyPrivacy is GPLv3/CC-BY-SA (license contamination risk if bundled).
- **Decision:** Pipeline (Rust): `ammonia` whitelist sanitization (strip script/iframe/form/object, on* handlers) → `lol_html` neutralization (remote `img[src]` → blocked placeholder, count for badge; `cid:` → local attachment) → link cleaning (strip `utm_*`, `fbclid`, `gclid`, `mc_eid`, known redirect trackers via a small bundled rule set derived from permissively-licensed sources — EasyPrivacy subset only if relicensed/justdomains and compatible, else UglyEmail + hand-curated list). Render in a sandboxed iframe (`sandbox=""`, no scripts, CSP `default-src 'none'`). Per-sender allowlist ("Always show images from …"), per-message override. Default OFF: local image-fetch proxy (127.0.0.1) — honest label: it strips headers but the user's IP still reaches the sender.
- **Alternatives:** remote proxy (violates no-middleman); allow-all images (privacy failure).
- **Consequences:** Unknown/novel trackers can pass (documented limitation); marketing mail may render with gaps until allowed; blocklist updates are opt-in (D-016 network policy).

## D-015 — 2026-11-04 — Zero telemetry, network allowlist, manual-first updates — accepted

- **Context:** Owner requirement: zero telemetry/analytics. Verified plugin ecosystem supports fully-offline defaults.
- **Decision:** No analytics/crash-upload code paths exist (not merely disabled). All egress is: (a) mail provider hosts (IMAP/SMTP/OAuth), (b) HuggingFace model download (only during opt-in download), (c) update check (OFF by default, manual setting), (d) BYO AI endpoint (only when user adds a key and triggers a task), (e) optional blocklist update (OFF by default). A documented allowlist is enforced at the settings layer; Diagnostics shows every host the app ever contacted this session. Crash logs are local files only.
- **Consequences:** Product analytics are impossible by design; success metrics are local + user-reported (PRD §Success).

## D-016 — 2026-11-04 — Packaging: NSIS per-user + minisign updater; signing deferred to Phase 6 — accepted

- **Context:** Verified: NSIS default install mode is per-user (no admin); updater supports NSIS with minisign signatures (`createUpdaterArtifacts`, verification cannot be disabled); Azure Artifact Signing is $9.99/mo (paid Azure sub required; individuals only in US/CA); EV certs no longer bypass SmartScreen (2024+); unsigned builds hit SmartScreen warnings for weeks.
- **Decision:** `bundle.targets = ["nsis"]`, `webviewInstallMode = embedBootstrapper` (~1.8 MB, keeps installer small; Win10 1809+ and Win11 ship WebView2 already). Ship unsigned dev builds during Phases 1–5 (documented in README), add signing in Phase 6 via Azure Artifact Signing if the maintainer is eligible, else an OV cert. Update endpoint = GitHub Releases `latest.json`, update check OFF by default (D-015).
- **Consequences:** README carries the SmartScreen expectation up front; release checklist includes key ceremony (`tauri signer generate`, private key kept off-repo).

## D-017 — 2026-11-04 — OAuth redirect: loopback, not deep-link — accepted

- **Context:** deep-link plugin requires a protocol registration (registry write, per-machine side effects); OAuth 2.0 for Native Apps (RFC 8252) explicitly supports loopback redirects.
- **Decision:** Outlook OAuth uses `http://127.0.0.1:<ephemeral port>` redirect with a short-lived local listener only during the connect flow.
- **Consequences:** One less plugin and no protocol handler; connect flow must handle the listener-fails edge (port blocked by firewall policy) with a manual-paste fallback (paste redirect URL).

## D-018 — 2026-11-04 — Test strategy: WDIO embedded primary, Playwright-CDP fallback, Dovecot nightly — accepted

- **Context:** Verified: `tauri-driver` 2.1.0 maintained; docs now recommend WebdriverIO + `@wdio/tauri-service` (embedded WebDriver provider, IPC mocking via `browser.tauri.mock()`); Playwright officially supports WebView2 via `connectOverCDP` with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port`; WinAppDriver is dead (2020); `tauri::test` MockRuntime exists in v2 for headless command tests; GreenMail lacks IDLE/QRESYNC — Dovecot-in-Docker is the fidelity option; `@tauri-apps/api/mocks` (`mockIPC`, `clearMocks`) exists for Vitest.
- **Decision:** Test pyramid per spec §13. Primary E2E: WDIO + `@wdio/tauri-service` (external `tauri-driver` provider in CI to keep the production binary free of the embedded-driver plugin). Fallback/local: Playwright over CDP with unique debug port + user-data folder per run. IMAP: L3 scripted-socket mocks in PR CI; Dovecot container (CONDSTORE/QRESYNC + IDLE) as a gated nightly; GreenMail for SMTP submission. AI: golden-file tests with SmolLM2-135M/Qwen3-0.6B, greedy decoding.
- **Consequences:** Windows runners cost 2× Linux minutes — nightly-heavy jobs stay gated; no win10/win11 hosted matrix exists, so real Win10 coverage is a local QA checklist item per release.

## D-019 — 2026-11-04 — IMAP transport: io-imap 0.8.1, blocking client over rustls — accepted

- **Context:** Supersedes the 0.7.1 pin implied in D-009. Verified against vendored sources + docs.rs: io-imap 0.8.1 is sans-IO coroutines over imap-codec 2.0.0-alpha.10; `ImapClientStd` provides a blocking command surface (`ImapClient` trait: implement `run`, inherit ~40 commands), `connect(url, opts)` handles TCP/TLS/STARTTLS/greeting/SASL end-to-end; SASL via io-sasl 0.1.1 (`Sasl::Plain`/`Sasl::Xoauth2`, secrets as `SecretString`); `ImapIdle`/`watch_mailbox` declare their own yield vocabulary so they cannot pass through `client.run()` — the eventful IDLE driver is Phase-3 work. IMAP UIDs are 1-based ascending per RFC 3501 (our first mock assigned 0 and silently lost one message in every sync test).
- **Decision:** `io-imap = "0.8"` (0.8.1) as the transport; adapter `src/mail/imap.rs` implements the `MailTransport` trait 1:1 (FETCH with `BODY.PEEK[HEADER.FIELDS (…)]` exactly per spec §4.3, `SEARCH ALL`, `STORE ±`, `APPEND` with APPENDUID, select/list mapped incl. SPECIAL-USE attributes). Error mapping: `Io` → `Disconnected`, all SASL/LOGIN variants → `Auth`, rest → `Protocol`. `idle_once` is a NOOP poll returning `Quiet` (spec §4.4 sanctions the no-IDLE polling fallback; engine re-syncs each cycle). `thrid` stays `None`: X-GM-THRID is a Gmail FETCH item io-imap does not model; JWZ threading handles Gmail correctly without it (D-010 short-circuit simply never fires).
- **Consequences:** Body extraction uses mail-parser 0.11.9 (v1.0.1 needs Rust 1.98 > our 1.90 floor): raw text/html stored per spec §3, sanitized at render (Phase 3). X-GM-THRID revisit only if Gmail threading quality proves insufficient.

## D-020 — 2026-11-04 — Windows test binaries need an embedded comctl32 v6 manifest; embed via linker for every target — accepted

- **Context:** Every Rust test binary died at load with STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139): tao imports comctl32 v6-only exports (`SetWindowSubclass`, `DefSubclassProc`, `TaskDialogIndirect`), and a PE without an application manifest binds comctl32 5.82 from System32, which lacks them. tauri-build embeds its manifest into bins only — test harnesses were left bare. Same diagnosis/fix documented in multiple downstream projects (asyar#498, GroupMe-Windows, VoiceStudio#1552, cc-switch#644). Verified in cargo source: `cargo::rustc-link-arg-tests` covers only declared `tests/*` targets, never the lib unit-test harness; `[target]` rustflags in .cargo/config.toml leak into dependency build scripts (caused LNK1327 there).
- **Decision:** `build.rs` emits `cargo:rustc-link-arg=/MANIFEST:EMBED` + `/MANIFESTINPUT:app.manifest` (absolute path) on MSVC targets, paired with `tauri_build::WindowsAttributes::new_without_app_manifest()` to avoid CVT1100/LNK1123 duplicate-manifest resources. `app.manifest` carries only the Common-Controls 6.0.0.0 dependency. One linker embed per linked product (bin + test harnesses).
- **Consequences:** Any future crate adding binaries gets the manifest automatically; GNU toolchain path branches to plain `tauri_build::build()` (GNU linker has no /MANIFEST args and no such need).

## D-021 — 2026-11-04 — Windows Credential Manager wrapper serializes access; provider presets are verified constants — accepted

- **Context:** Concurrent `CredWriteW`/`CredReadW` calls — even on different entries — have no visibility ordering: measured by downstream users (memrynote/memry#3716: 67–75 missed reads of 500 with 5 threads, 0 with a mutex), and keyring-rs#163 removed multithreaded Windows tests for this reason; the crate's own cross-thread test is `#[ignore]`d. Reproduced here (~2/5 runs lost a set→get). Separately, provider connection/app-password facts (hosts, ports, guided URLs, manual IMAP toggles) were verified against official provider docs (see `src/platform/providers.rs`, checked 2026-11-04): Gmail/iCloud/Yahoo CONFIRMED; Zoho requires an IMAP enable toggle (CONFIRMED); Outlook.com is OAuth-only since 2024-09-16 and POP/IMAP off by default (CONFIRMED, Microsoft).
- **Decision:** The `platform::keyring` wrapper serializes every credential operation through a process-wide mutex (correctness no longer depends on callers; stress test `concurrent_set_get_delete_never_misses`: 8 threads × 40 set→get→delete cycles, 0 misses). Per-test Credential Manager targets are made unique (pid + counter) so parallel tests never share entries. Provider presets live as verified constants with source URLs in comments; preset routing is tested.
- **Consequences:** Strawberry Perl must be on PATH for the vendored OpenSSL build of `bundled-sqlcipher-vendored-openssl` (build env requirement, not a code decision).

## D-022 — 2026-11-04 — Outlook OAuth: consumers-tenant v2 endpoints, outlook.office.com scopes, PKCE + loopback, rotating refresh tokens — accepted

- **Context:** Verified against Microsoft Entra/Exchange documentation (2026-11-04): authorize/token endpoints are `login.microsoftonline.com/consumers/oauth2/v2.0/{authorize,token}`; scope set is `offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send` — the `outlook.office365.com` scope variant is a documented common failure; loopback redirect `http://127.0.0.1/<port>` works because Entra ignores the port when matching loopback reply URLs (must be registered via the app manifest, not the portal text box); refresh tokens rotate on every use (store the replacement, discard the old); basic auth for personal accounts died 2024-09-16 (CONFIRMED); Thunderbird's registered-client precedent shows third-party OAuth IMAP for personal accounts works (CONFIRMED endorsement by Microsoft's own support page); XOAUTH2 failure on Microsoft is a plain tagged NO (the base64-JSON challenge is Gmail's shape).
- **Decision:** `src/platform/oauth.rs`: PKCE (S256) always; system browser → loopback listener → code exchange → refresh with rotation; tokens carried as `SecretString`; refresh token persisted per account under `cracktivity.account.<id>.refresh_token` (Credential Manager, ≤ 2.5 KB cap — verified fits, V-1.4); access token stays in memory only. The Entra client id is a named constant (`OUTLOOK_CLIENT_ID`), currently empty and failing with `ClientIdMissing` until the project's app registration is created — a user action for V-2.4. Endpoints/scopes are verified constants with the source reasoning in the module doc.
- **Consequences:** D-015 allowlist gains `login.microsoftonline.com` for the token exchange (documented there); ureq 3.5.1 (blocking, rustls) is the only new egress-capable dependency and is called with hardcoded URLs only.
