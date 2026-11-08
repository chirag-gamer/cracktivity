**Status:** in development. Phases 0-4 are built (encrypted local store, IMAP sync, Important/Other triage, notch panel, keyboard-first UI, compose/send via SMTP with a Sent copy, "Waiting for Reply" follow-ups, subscription cleanup with one-click RFC 8058 unsubscribe + Undo, tracker blocking with a per-sender image allowlist, the opt-in on-device AI with model download/tiering/BYO keys, and the built-in MCP server with its safety gates, see [plan.md](plan.md)). Phase 5 and 6 polish (release signing, the signed updater, the QA matrix) is the remaining tail. Development builds only, no signed installer yet.

To build with the AI sidecar, fetch the pinned llama.cpp build once (`node scripts/fetch-llama.mjs`; the tag is recorded in [decisions.md](decisions.md) D-030). Without it, everything except AI inference works.

# cracktivity

All your email in one Windows app that never sends your mail through someone else's server.

## What it does

cracktivity puts your Gmail, Outlook, iCloud, Zoho and Yahoo accounts into a single inbox. The app talks to your mail providers directly, over IMAP and OAuth. There is no cracktivity account, no cracktivity server, and no copy of your mail passing through us, because there is no us in the middle.

Summaries and reply drafts come from our own fine-tuned small model (cracktivity-email-qwen3-0.6b) that runs on your own CPU. If you would rather use a bigger model, you can connect your ChatGPT Plus or Pro plan with Sign in with ChatGPT (no API key needed), or plug in your own OpenAI, Claude or Grok key; that part talks to whichever provider you chose, and only when you ask it to.

## Features

**One inbox, sorted for you**

- All accounts in one list, or one list per account. Ctrl+1 through Ctrl+9 jumps between them.
- Important vs. Other sorting, computed on your machine from your own habits. not from a cloud profile.
- Waiting-for-reply: if you sent something and nobody answered, it comes back to you.
- Subscription cleanup: see every newsletter you receive, rank the ones you never open, unsubscribe in one click, archive what they already sent, with Undo.

**A dropdown from the top of the screen**

- A panel drops down from the top edge, like the notch dropdowns on Mac. Open it from the tray or a global hotkey, scan what's new, act, done. It hides when you click away. The full app handles the rest.

**On-device AI, by default and optional**

- Thread summaries and reply drafting run on our fine-tuned small model on your CPU. No GPU needed. You opt in during setup (it is the recommended default), and turning it off removes nothing else.
- Have a ChatGPT Plus or Pro plan? Sign in with ChatGPT and your plan powers the AI features in the app. No API key, no new billing.
- Developers get a built-in MCP server: point Claude Code, Codex or Cursor at your mail. Read-only by default; sending stays off until you switch it on.

**Built for Windows, and for being offline**

- Native tray icon, notifications, optional auto-start, and full-text search that works with no internet. Everything you have already read stays readable on a plane.

## Download

Not yet. When the first build ships it will land on the [Releases page](../../releases) as a per-user installer that needs no admin rights.

### "Windows protected your PC"

Early builds are not code-signed, so SmartScreen will show that warning. Choose "More info" → "Run anyway". This goes away once signing is in place. it is tracked in [plan.md](plan.md).

### Uninstall

Uninstall it from Windows Settings → Apps, like any other app. Uninstalling does not delete your mail cache; the folder it lives in is shown in Settings → Storage, and you delete it yourself if you want everything gone.

### Windows 10

cracktivity targets Windows 10 and Windows 11. On Windows 10 a few visual touches (rounded panel corners) fall back to simpler styles. The dropdown panel works on both.

## How your mail stays private

- Passwords and tokens are stored in Windows Credential Manager. Your mail cache is an encrypted local database under your own user profile.
- The app keeps a network allowlist: your mail providers, plus optional things you explicitly turn on (the AI model download, app updates, a custom AI key). Nothing else. No telemetry, no analytics, no crash uploads. crash logs stay on your disk, and you can read them before you share them.

## Found a bug?

Open an issue and attach the log from Settings → Diagnostics → Open log folder. The log stays on your disk; read it before you attach it.

## Build it yourself

You need Rust, Node.js and pnpm.

    pnpm install
    pnpm tauri dev

To build the installer:

    pnpm tauri build

The architecture lives in [spec.md](spec.md). If you are an AI agent working in this repo, read [AGENTS.md](AGENTS.md) first.

## License

MIT. Third-party assets that ship inside the binary (Lucide icons) and the
on-demand AI models carry their own licenses, listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
