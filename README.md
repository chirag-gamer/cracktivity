**Status:** in development. Phases 0–3 core are built (encrypted local store, IMAP sync, Important/Other triage, notch panel, keyboard-first UI — see [plan.md](plan.md)). Compose/Send, follow-ups, subscription cleanup, the on-device AI download and the MCP server land in later phases. Development builds only — no signed installer yet.

# cracktivity

All your email in one Windows app that never sends your mail through someone else's server.

## What it does

cracktivity puts your Gmail, Outlook, iCloud, Zoho and Yahoo accounts into a single inbox. The app talks to your mail providers directly, over IMAP and OAuth. There is no cracktivity account, no cracktivity server, and no copy of your mail passing through us, because there is no us in the middle.

Summaries and reply drafts come from a small AI model that runs on your own CPU. If you would rather use a bigger model, you can plug in your own OpenAI, Claude or Grok key — that part talks to whichever provider you chose, and only when you ask it to.

## Features

**One inbox, sorted for you**

- All accounts in one list, or one list per account. Ctrl+1 through Ctrl+9 jumps between them.
- Important vs. Other sorting, computed on your machine from your own habits — not from a cloud profile.
- Waiting-for-reply: if you sent something and nobody answered, it comes back to you.
- Subscription cleanup: see every newsletter you receive, rank the ones you never open, unsubscribe in one click, archive what they already sent, with Undo.

**A dropdown from the top of the screen**

- A panel drops down from the top edge, like the notch dropdowns on Mac. Open it from the tray or a global hotkey, scan what's new, act, done. It hides when you click away. The full app handles the rest.

**On-device AI, by default and optional**

- Thread summaries and reply drafting run on a small open-weights model on your CPU. No GPU needed. You opt in during setup — it is the recommended default — and turning it off removes nothing else.
- Developers get a built-in MCP server: point Claude Code, Codex or Cursor at your mail. Read-only by default; sending stays off until you switch it on.

**Built for Windows, and for being offline**

- Native tray icon, notifications, optional auto-start, and full-text search that works with no internet. Everything you have already read stays readable on a plane.

## Download

Not yet. When the first build ships it will land on the [Releases page](../../releases) as a per-user installer that needs no admin rights.

### "Windows protected your PC"

Early builds are not code-signed, so SmartScreen will show that warning. Choose "More info" → "Run anyway". This goes away once signing is in place — it is tracked in [plan.md](plan.md).

### Uninstall

Uninstall it from Windows Settings → Apps, like any other app. Uninstalling does not delete your mail cache; the folder it lives in is shown in Settings → Storage, and you delete it yourself if you want everything gone.

### Windows 10

cracktivity targets Windows 10 and Windows 11. On Windows 10 a few visual touches (rounded panel corners) fall back to simpler styles. The dropdown panel works on both.

## How your mail stays private

- Passwords and tokens are stored in Windows Credential Manager. Your mail cache is an encrypted local database under your own user profile.
- Remote images in email HTML are blocked by default — all of them, so tracker pixels have nothing to load — and links are cleaned of tracking parameters before you click them. (A known-tracker list with per-sender image permissions arrives in a later build.)
- The app keeps a network allowlist: your mail providers, plus optional things you explicitly turn on (the AI model download, app updates, a custom AI key). Nothing else. No telemetry, no analytics, no crash uploads — crash logs stay on your disk, and you can read them before you share them.

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

MIT.
