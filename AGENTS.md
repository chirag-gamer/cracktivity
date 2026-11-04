# AGENTS.md — Operating contract for agents working on cracktivity

This file governs human and AI contributors alike. If an instruction elsewhere conflicts with this file, this file wins; if it conflicts with a user instruction in-session, surface the conflict explicitly before acting.

## 1. Source of truth

| Question | Authoritative doc |
|---|---|
| What are we building, and what counts as done? | [prd.md](prd.md) (FRs + acceptance criteria) |
| How is it built (architecture, data, security)? | [spec.md](spec.md) |
| Why was each choice made? | [decisions.md](decisions.md) (append-only) |
| What order, what's next, what's verified? | [plan.md](plan.md) (living) |
| What does the user see first? | [README.md](README.md) |

Precedence on conflict: prd.md > spec.md > plan.md. Code comments never override docs; if code and spec disagree, one of them is wrong — fix the doc or the code, never silently drift.

## 2. Safety parameters (hard invariants — never violated)

1. **Privacy is load-bearing.** No telemetry, analytics, or crash-upload code paths — not "disabled by default", *absent*. Outbound network is allowlist-only (D-015). Any PR introducing an unlisted network host fails review.
2. **Secrets discipline.** Passwords/tokens only in Windows Credential Manager; DEK only DPAPI-wrapped. Never in config, webview storage, logs, or test fixtures. Never commit `.env`-style secrets; the repo must stay clean by construction.
3. **Mail HTML is attacker-controlled.** It never renders unsanitized and never reaches the MCP layer as raw HTML (spec §8, §9).
4. **Read-only defaults everywhere power is granted** (MCP tools, remote images). Every escalation needs an explicit, logged user action.
5. **No destructive commands without confirmation** in agent sessions: `rm -rf` outside `target/`/`node_modules/`, `git reset --hard`, `git push --force`, registry edits, deleting user data directories.
6. **Scope discipline (surgical changes).** Touch only what the current task requires; do not refactor unbroken code, reformat unrelated lines, or "improve" adjacent code. Every changed line must trace to a PRD FR or a plan task. Noticed dead code gets *mentioned*, not deleted.
7. **Simplicity gate.** Minimum viable code. No speculative abstractions, no config for single-use values, no error handling for impossible states. If a change can be written in 50 lines instead of 200, write 50.
8. **decisions.md is append-only.** Never rewrite history; supersede with a new entry referencing the old ID.

## 3. Workflow handoffs

The standard loop for any non-trivial task:

1. **Research** — spawn read-only general subagents (Exa MCP + context7) for anything requiring external facts. Subagents must never write files or run state-changing commands. Batch up to 5 concurrent; give each a bounded scope, a word budget, and a CONFIRMED/LIKELY labeling requirement.
2. **Verify** — the implementing agent spot-checks load-bearing subagent claims against primary sources before coding. A claim marked LIKELY must be either verified or encoded as provisional (test-gated) in spec/decisions.
3. **Plan** — record the task in plan.md with its FR mapping and verification method *before* implementing.
4. **Implement** — smallest change that satisfies the FR acceptance criteria.
5. **Verify** — run the verification gates (§5); update the plan.md matrix row with evidence; only then mark complete.
6. **Record** — new decisions (even provisional ones) go to decisions.md the same session.

Handoff artifacts between phases: a phase is done when every plan.md task for it is verified; the next phase's first task is to re-read prd.md + spec.md diffs and confirm no drift.

## 4. Build, run, test

```bash
pnpm install            # frontend deps
pnpm tauri dev          # run app (GUI role)
pnpm tauri dev -- --mcp # run MCP role over stdio (for manual agent testing)
pnpm vitest run         # L1 frontend tests
cargo test              # L2/L3 Rust tests (src-tauri)
cargo test -- --ignored # L4 gated live-IMAP tests (needs Dovecot container)
pnpm wdio run           # L5 e2e (tauri-driver / CDP)
pnpm tauri build        # NSIS installer
```

Style: match surrounding code; Rust `cargo fmt` + `clippy -D warnings`; TS eslint + prettier + `tsc --noEmit`. Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:`) referencing FR IDs.

## 5. Verification gates (definition of done for any task)

A task is complete when **all** hold:
1. Acceptance criteria from prd.md are demonstrated (automated test or recorded manual QA with evidence in the PR description).
2. `cargo fmt --check`, `cargo clippy -- -D warnings`, `pnpm lint`, `pnpm tsc --noEmit`, `pnpm vitest run`, `cargo test` all green locally.
3. New behavior has a test at the lowest capable level (prefer L1–L3 over L5).
4. plan.md matrix row updated; decisions.md appended if a choice was made.
5. No scope creep beyond the task's FR/task mapping.

## 6. When uncertain

- Multiple plausible interpretations → present them, recommend one, and proceed with the recommended one unless it is destructive or irreversible (then ask).
- Missing external fact → spawn a read-only subagent; do not guess API names or versions.
- Contradiction between sources → record it in decisions.md as provisional with a Phase-1-style verification task (see D-006 FTS5 pattern).
- Blocked on a user decision → stop, name the exact decision and options in one message; do not silently pick on irreversible matters.
