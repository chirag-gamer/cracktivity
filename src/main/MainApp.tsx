/**
 * Main window shell (FR-29): accounts/folders · list · reader, wired to the
 * keyboard model (FR-30), Ctrl+1..9 account switching (FR-4), Important/Other
 * filter (FR-11) and local FTS search (FR-9 v1).
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
import { S } from "../shared/strings";

export function MainApp() {
  const mail = useMail();
  const [search, setSearch] = useState<SearchRow[] | null>(null);
  const [overlay, setOverlay] = useState(false);
  const [palette, setPalette] = useState(false);
  const [adding, setAdding] = useState(false);

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

  // Opening a thread marks its unread messages read — keyed on the thread
  // identity only, so the refresh triggered by 'u' (mark unread) cannot
  // immediately re-mark it read.
  const lastMarkedThrid = useRef<string | null>(null);
  useEffect(() => {
    if (!mail.selectedThrid || lastMarkedThrid.current === mail.selectedThrid)
      return;
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

  const newestOf = (thrid: string | null): ThreadRow | null => {
    if (!thrid) return null;
    return mail.threads.find((t) => t.thrid === thrid) ?? null;
  };

  const actions = useMemo(
    () => ({
      move: (delta: number) => selectAt(cursor.current + delta),
      open: () => selectAt(cursor.current),
      archive: () => {
        // v1 triage semantics: archive the newest message of the selected
        // thread (the one being triaged); older thread messages stay.
        const t = newestOf(mail.selectedThrid);
        if (t) {
          ipc
            .threadMessages(t.thrid)
            .then((ms) => {
              const target = ms[ms.length - 1];
              if (target) return ipc.archiveMessage(target.id);
              return undefined;
            })
            .then(() => mail.refresh())
            .catch(() => {});
        }
      },
      toggleUnread: () => {
        const anyUnread = mail.messages.some((m) => m.unread);
        markThreadRead(!anyUnread);
        // Keep the open-thread effect from re-marking on the next refresh.
        lastMarkedThrid.current = mail.selectedThrid;
      },
      toggleStar: () => {
        const t = newestOf(mail.selectedThrid);
        if (!t) return;
        ipc
          .threadMessages(t.thrid)
          .then((ms) => {
            const target = ms[ms.length - 1];
            if (target) return ipc.setStarred(target.id, !target.starred);
            return undefined;
          })
          .then(() => mail.refresh())
          .catch(() => {});
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
    [mail.accounts, mail.selectedThrid, mail.threads, mail.messages, mail, selectAt, markThreadRead, search],
  );
  const modalOpen = overlay || palette || adding;
  useKeyboard(actions, !modalOpen);

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
      { id: "sync", label: S.cmdSync, run: () => void ipc.syncNow().catch(() => {}) },
      { id: "add", label: S.addAccount, run: () => setAdding(true) },
    ],
    [mail],
  );

  const onMarkRead = useCallback((m: { id: number; unread: boolean }) => {
    ipc.markRead(m.id, !m.unread).catch(() => {});
  }, []);

  return (
    <div className="app-shell" data-testid="main-app">
      <AccountPane
        accounts={mail.accounts}
        statuses={mail.statuses}
        scope={mail.scope}
        onScope={mail.setScope}
      />
      <div className="pane-middle">
        <Toolbar
          filter={mail.filter}
          onFilter={mail.setFilter}
          onSearch={setSearch}
          onAddAccount={() => setAdding(true)}
          statuses={mail.statuses}
        />
        {search ? (
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
          />
        )}
      </div>
      <Reader messages={mail.messages} onMarkRead={onMarkRead} />
      <ShortcutsOverlay open={overlay} onClose={() => setOverlay(false)} />
      <CommandPalette
        open={palette}
        onClose={() => setPalette(false)}
        commands={commands}
      />
      <AddAccountDialog
        open={adding}
        onClose={() => setAdding(false)}
        onAdded={() => void ipc.syncNow().catch(() => {})}
      />
    </div>
  );
}
