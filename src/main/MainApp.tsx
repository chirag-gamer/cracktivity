/**
 * Main window shell (FR-29): accounts/folders · list · reader, wired to the
 * keyboard model (FR-30), Ctrl+1..9 account switching (FR-4), Important/Other
 * filter (FR-11), local FTS search (FR-9), and the Phase-4 tool views
 * (follow-ups, cleanup, settings) plus the composer and first-run onboarding.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ipc } from "../shared/ipc";
import type { SearchRow, ThreadRow } from "../shared/types";
import { useMail } from "./useMail";
import { useKeyboard } from "./keyboard";
import { AccountPane } from "./AccountPane";
import { MessageList } from "./MessageList";
import { Reader } from "./Reader";
import { Toolbar, SearchResults } from "./Toolbar";
import { CommandPalette, ShortcutsOverlay, type PaletteCommand } from "./Overlays";
import { AddAccountDialog } from "./AddAccountDialog";
import { FollowupsView } from "./FollowupsView";
import { CleanupView } from "./CleanupView";
import { SettingsView } from "./SettingsView";
import { Composer, type ComposerTarget } from "./Composer";
import { OnboardingDialog } from "./Onboarding";
import {
  onKeymapChange,
  readKeymapScheme,
  syncKeymapScheme,
  type KeymapScheme,
} from "./keymap";
import { S } from "../shared/strings";

export function MainApp() {
  const mail = useMail();
  const [search, setSearch] = useState<SearchRow[] | null>(null);
  const [alsoServer, setAlsoServer] = useState(false);
  const [aiEnabled, setAiEnabled] = useState(false);
  const [overlay, setOverlay] = useState(false);
  const [palette, setPalette] = useState(false);
  const [adding, setAdding] = useState(false);
  const [composer, setComposer] = useState<ComposerTarget | null>(null);
  const [onboarding, setOnboarding] = useState(false);
  const [keymap, setKeymap] = useState<KeymapScheme>(() => readKeymapScheme());

  // Keymap scheme (FR-30 AC): DB row is canonical, mirror seeds first paint;
  // in-window changes from Settings apply immediately (D-046).
  useEffect(() => {
    syncKeymapScheme(setKeymap);
    return onKeymapChange(setKeymap);
  }, []);

  // First-run dialog (FR-20): exactly once, keyed by onboarding.completed.
  useEffect(() => {
    if (mail.loading) return;
    if (mail.accounts.length > 0) return;
    ipc
      .getSettings()
      .then((s) => {
        if (!s["onboarding.completed"]) setOnboarding(true);
      })
      .catch(() => {});
    // Only on the initial load.
  }, [mail.loading]);

  const refreshSettings = useCallback(() => {
    ipc
      .getSettings()
      .then((s) => {
        setAlsoServer(s["search.also_server"] === true);
        setAiEnabled(s["ai.enabled"] === true);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshSettings();
  }, [refreshSettings, mail.scope, onboarding]);

  // Selection cursor for j/k: index into the current thread list.
  const cursor = useRef(0);
  useEffect(() => {
    cursor.current = 0;
  }, [mail.scope, mail.filter, search]);

  // Search rows are keyboard-selectable too (FR-30/FR-9): same mapping the
  // click path uses; a null thrid (pre-threading row) selects by message id.
  const searchAsThreads: ThreadRow[] = (search ?? []).map((r) => ({
    thrid: r.thrid ?? `__msg-${r.message_id}`,
    account_id: r.account_id,
    account_email: "",
    subject: r.subject,
    snippet: r.snippet,
    from: r.from,
    last_date: r.date,
    msg_count: 1,
    unread_count: r.unread ? 1 : 0,
    starred: false,
    importance: null,
  }));
  const currentThreads = search ? searchAsThreads : mail.threads;

  const selectAt = useCallback(
    (idx: number) => {
      if (currentThreads.length === 0) return;
      const clamped = Math.max(0, Math.min(idx, currentThreads.length - 1));
      cursor.current = clamped;
      mail.select(currentThreads[clamped].thrid);
    },
    [currentThreads, mail],
  );

  // Opening a thread marks its unread messages read. keyed on the thread
  // identity, so the refresh triggered by 'u' (mark unread) cannot
  // immediately re-mark it read. The latch is set only once the thread's
  // messages have loaded (they arrive after selection), otherwise the
  // first run with an empty list would swallow the auto-read entirely.
  const lastMarkedThrid = useRef<string | null>(null);
  useEffect(() => {
    if (!mail.selectedThrid || lastMarkedThrid.current === mail.selectedThrid)
      return;
    if (mail.messages.length === 0) return; // wait for the thread contents
    lastMarkedThrid.current = mail.selectedThrid;
    mail.messages
      .filter((m) => m.unread)
      .forEach((m) => ipc.markRead(m.id, true).catch(() => {}));
  }, [mail.selectedThrid, mail.messages]);

  /** Applies a read state to every message of the selected thread. */
  const markThreadRead = useCallback(
    (read: boolean) => {
      mail.messages.forEach((m) => ipc.markRead(m.id, read).catch(() => {}));
    },
    [mail.messages],
  );

  const newestOf = useCallback(
    (thrid: string | null): ThreadRow | null => {
      if (!thrid) return null;
      return mail.threads.find((t) => t.thrid === thrid) ?? null;
    },
    [mail.threads],
  );

  const openComposer = useCallback((target: ComposerTarget) => {
    setComposer(target);
  }, []);

  const archiveThreadById = useCallback(
    (thrid: string) => {
      ipc
        .threadMessages(thrid)
        .then((ms) => {
          const target = ms[ms.length - 1];
          if (target) return ipc.archiveMessage(target.id);
          return undefined;
        })
        .then(() => mail.refresh())
        .catch(() => {});
    },
    [mail],
  );

  const toggleStarThreadById = useCallback(
    (thrid: string, nextStarred?: boolean) => {
      ipc
        .threadMessages(thrid)
        .then((ms) => {
          const target = ms[ms.length - 1];
          if (target) {
            const desired = nextStarred !== undefined ? nextStarred : !target.starred;
            return ipc.setStarred(target.id, desired);
          }
          return undefined;
        })
        .then(() => mail.refresh())
        .catch(() => {});
    },
    [mail],
  );

  const handleSyncNow = useCallback(() => {
    void ipc
      .syncNow()
      .then(() => mail.refresh())
      .catch(() => {});
  }, [mail]);

  const actions = useMemo(
    () => ({
      move: (delta: number) => selectAt(cursor.current + delta),
      open: () => selectAt(cursor.current),
      archive: () => {
        // v1 triage semantics: archive the newest message of the selected
        // thread (the one being triaged); older thread messages stay.
        const t = newestOf(mail.selectedThrid);
        if (t) {
          archiveThreadById(t.thrid);
        }
      },
      toggleUnread: () => {
        // Gmail-style toggle: unread thread → read; read thread → unread.
        // (markRead(id, read=true) means READ. polarity pinned by test.)
        const anyUnread = mail.messages.some((m) => m.unread);
        markThreadRead(anyUnread);
        // Keep the open-thread effect from re-marking on the refresh below.
        lastMarkedThrid.current = mail.selectedThrid;
        mail.refresh();
      },
      toggleStar: () => {
        const t = newestOf(mail.selectedThrid);
        if (!t) return;
        toggleStarThreadById(t.thrid);
      },
      compose: () => {
        const first = mail.accounts[0];
        if (first) {
          openComposer({
            accountId: first.id,
            to: [],
            subject: "",
          });
        } else {
          setAdding(true);
        }
      },
      reply: () => {
        const t = newestOf(mail.selectedThrid);
        if (!t || mail.messages.length === 0) return;
        const last = mail.messages[mail.messages.length - 1];
        openComposer({
          accountId: last.account_id,
          to: last.from,
          subject: t.subject.startsWith("Re:") ? t.subject : `Re: ${t.subject}`,
          inReplyTo: null,
          threadId: t.thrid,
        });
      },
      focusSearch: () => {
        (document.getElementById("global-search") as HTMLInputElement | null)?.focus();
      },
      toggleOverlay: () => setOverlay((v) => !v),
      togglePalette: () => setPalette((v) => !v),
      accountSlot: (n: number) => {
        const acc = mail.accounts[n - 1];
        if (acc) mail.setScope({ kind: "account", id: acc.id });
      },
      escape: () => {
        setOverlay(false);
        setPalette(false);
        if (search) setSearch(null);
      },
    }),
    [
      mail.accounts,
      mail.selectedThrid,
      mail.messages,
      mail,
      selectAt,
      markThreadRead,
      search,
      openComposer,
      newestOf,
      archiveThreadById,
      toggleStarThreadById,
    ],
  );
  const modalOpen = overlay || palette || adding || composer !== null;
  useKeyboard(actions, !modalOpen, keymap);

  const commands: PaletteCommand[] = useMemo(
    () => [
      {
        id: "unified",
        label: S.cmdUnified,
        run: () => mail.setScope({ kind: "unified" }),
      },
      ...mail.accounts.map((a, i) => ({
        id: `account-${a.id}`,
        label: S.cmdAccount(i + 1, a.display_name ?? a.email),
        run: () => mail.setScope({ kind: "account", id: a.id }),
      })),
      { id: "followups", label: S.followups, run: () => mail.setScope({ kind: "followups" }) },
      { id: "cleanup", label: S.cleanup, run: () => mail.setScope({ kind: "cleanup" }) },
      { id: "settings", label: S.settings, run: () => mail.setScope({ kind: "settings" }) },
      { id: "sync", label: S.cmdSync, run: handleSyncNow },
      { id: "add", label: S.addAccount, run: () => setAdding(true) },
      {
        id: "compose",
        label: S.compose,
        run: actions.compose,
      },
    ],
    [mail, handleSyncNow, actions.compose],
  );

  const onMarkRead = useCallback(
    (m: { id: number; unread: boolean }) => {
      ipc
        .markRead(m.id, m.unread)
        .then(() => mail.refresh())
        .catch(() => {});
    },
    [mail],
  );

  const openFollowUp = useCallback(
    (row: { account_id: number; to: { email: string } | null; subject: string; thrid: string }) => {
      openComposer({
        accountId: row.account_id,
        to: row.to ? [row.to] : [],
        subject: row.subject.startsWith("Re:") ? row.subject : `Re: ${row.subject}`,
        threadId: row.thrid,
      });
    },
    [openComposer],
  );

  const openAiReply = useCallback(
    (threadId: string, instruction: string) => {
      const last = mail.messages[mail.messages.length - 1];
      const t = newestOf(threadId);
      if (!last) return;
      openComposer({
        accountId: last.account_id,
        to: last.from,
        subject: t && !t.subject.startsWith("Re:") ? `Re: ${t.subject}` : (t?.subject ?? ""),
        threadId,
        initialBody: "",
        instruction,
      });
    },
    [mail.messages, newestOf, openComposer],
  );

  const activeAccount =
    mail.scope.kind === "account"
      ? mail.accounts.find((a) => a.id === (mail.scope as { id: number }).id)
      : undefined;
  const toolbarTitle =
    mail.scope.kind === "account"
      ? (activeAccount?.display_name ?? activeAccount?.email ?? "Account")
      : "Inbox";
  const scopeUnread =
    mail.scope.kind === "account"
      ? (activeAccount?.unread ?? 0)
      : mail.accounts.reduce((sum, a) => sum + a.unread, 0);

  return (
    <div className="app-shell" data-testid="main-app">
      <AccountPane
        accounts={mail.accounts}
        statuses={mail.statuses}
        scope={mail.scope}
        onScope={mail.setScope}
        onCompose={actions.compose}
        onAddAccount={() => setAdding(true)}
        onOpenPalette={() => setPalette(true)}
        onOpenShortcuts={() => setOverlay(true)}
      />
      <div className="pane-middle">
        {mail.scope.kind !== "settings" && (
          <Toolbar
            filter={mail.filter}
            onFilter={mail.setFilter}
            onSearch={setSearch}
            onAddAccount={() => setAdding(true)}
            statuses={mail.statuses}
            alsoServer={alsoServer}
            title={toolbarTitle}
            threadCount={mail.threads.length}
            unreadCount={scopeUnread}
            onSyncNow={handleSyncNow}
          />
        )}
        {mail.scope.kind === "followups" ? (
          <FollowupsView scope={mail.scope} onFollowUp={openFollowUp} />
        ) : mail.scope.kind === "cleanup" ? (
          <CleanupView />
        ) : mail.scope.kind === "settings" ? (
          <SettingsView />
        ) : search ? (
          <SearchResults
            results={search}
            onOpen={(t) => {
              setSearch(null);
              mail.select(t.thrid);
            }}
          />
        ) : (
          <MessageList
            threads={mail.threads}
            selectedThrid={mail.selectedThrid}
            onSelect={(thrid) => {
              const idx = mail.threads.findIndex((t) => t.thrid === thrid);
              if (idx >= 0) cursor.current = idx;
              mail.select(thrid);
            }}
            loading={mail.loading}
            hasAccounts={mail.accounts.length > 0}
            onAddAccount={() => setAdding(true)}
            onSyncNow={handleSyncNow}
            onShowAll={
              mail.filter !== "all" ? () => mail.setFilter("all") : undefined
            }
            onOpenShortcuts={() => setOverlay(true)}
            onArchiveThread={archiveThreadById}
            onToggleStarThread={toggleStarThreadById}
          />
        )}
      </div>
      <Reader
        messages={mail.messages}
        onMarkRead={onMarkRead}
        aiEnabled={aiEnabled}
        threadId={mail.selectedThrid}
        onDraftReply={openAiReply}
        onReply={actions.reply}
        onArchiveThread={actions.archive}
        onToggleStarThread={actions.toggleStar}
        onRefresh={() => mail.refresh()}
        onCompose={actions.compose}
        onFocusSearch={actions.focusSearch}
        onOpenPalette={() => setPalette(true)}
        onOpenShortcuts={() => setOverlay(true)}
      />
      <ShortcutsOverlay open={overlay} onClose={() => setOverlay(false)} scheme={keymap} />
      <CommandPalette
        open={palette}
        onClose={() => setPalette(false)}
        commands={commands}
      />
      <AddAccountDialog
        open={adding}
        onClose={() => setAdding(false)}
        onAdded={() => {
          void ipc.syncNow().catch(() => {});
          mail.refresh();
        }}
      />
      {composer && (
        <div className="overlay">
          <Composer
            target={composer}
            onClose={() => setComposer(null)}
            onSent={() => mail.refresh()}
          />
        </div>
      )}
      <OnboardingDialog
        open={onboarding}
        onFinish={() => setOnboarding(false)}
        onAddAccount={() => setAdding(true)}
      />
    </div>
  );
}
