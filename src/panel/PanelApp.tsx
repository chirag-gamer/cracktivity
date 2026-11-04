/**
 * Notch dropdown panel (FR-28, D-011): compact unread triage list. The
 * window itself is created/positioned/animated from Rust; this is only the
 * content: newest unread across accounts, j/k + quick actions, Esc hides.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge, Text } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { relativeTime, senderName } from "../shared/format";
import type { MessageRow } from "../shared/types";
import { useKeyboard } from "../main/keyboard";

/** Newest unread messages across all inboxes (the triage queue). */
async function loadQueue(): Promise<MessageRow[]> {
  const accounts = await ipc.listAccounts();
  const lists = await Promise.all(
    accounts.map((a) => ipc.threadList(a.id, "important")),
  );
  const rows = lists.flat();
  const msgs = await Promise.all(rows.map((r) => ipc.threadMessages(r.thrid)));
  return msgs
    .flat()
    .filter((m) => m.unread)
    // The same thread can surface via more than one account view; the row id
    // is globally unique, so dedupe on it before newest-first ordering.
    .filter((m, i, arr) => arr.findIndex((x) => x.id === m.id) === i)
    .sort((a, b) => b.date - a.date)
    .slice(0, 50);
}

export function PanelApp() {
  const [queue, setQueue] = useState<MessageRow[]>([]);
  const [idx, setIdx] = useState(0);
  const queueRef = useRef(queue);
  queueRef.current = queue;

  const refresh = useCallback(() => {
    loadQueue().then(setQueue).catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void (async () => {
      try {
      const { listen } = await import("@tauri-apps/api/event");
      const un1 = await listen("store-changed", refresh);
      const un2 = await listen("panel-visibility", (e) => {
        // The Rust side only shows/hides the HWND; the CSS slide-down is
        // driven here (D-011: never animate the Win32 window).
        document.body.classList.toggle("collapsed", e.payload !== true);
        if (e.payload === true) refresh();
      });
      const safe = () => {
        // _unlisten is async; a sync try/catch cannot see its rejection.
        void Promise.resolve(un1()).catch(() => {});
        void Promise.resolve(un2()).catch(() => {});
      };
      if (disposed) {
        safe();
      } else {
        unlisten = safe;
      }
      } catch {
        // No Tauri event bridge (tests): the queue stays static.
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);

  // Deep-link into the main window (FR-28: usable during normal app usage).
  const openMain = async () => {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const main = await WebviewWindow.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await ipc.hidePanel();
  };

  const actions = useMemo(
    () => ({
      move: (d: number) =>
        setIdx((i) => Math.max(0, Math.min(i + d, queueRef.current.length - 1))),
      open: () => void openMain(),
      archive: () => {
        const m = queueRef.current[idx];
        if (m) {
          void ipc.archiveMessage(m.id).then(refresh).catch(() => {});
          setIdx((i) => Math.max(0, i - 1));
        }
      },
      toggleUnread: () => {
        const m = queueRef.current[idx];
        if (m) void ipc.markRead(m.id, true).then(refresh).catch(() => {});
      },
      toggleStar: () => {
        const m = queueRef.current[idx];
        if (m) void ipc.setStarred(m.id, !m.starred).then(refresh).catch(() => {});
      },
      focusSearch: () => {},
      toggleOverlay: () => {},
      togglePalette: () => void openMain(),
      accountSlot: () => {},
      escape: () => void ipc.hidePanel().catch(() => {}),
    }),
    [idx, refresh],
  );
  useKeyboard(actions);

  return (
    <div className="panel-shell" data-testid="panel-app">
      <header className="panel-header">
        <Text weight="semibold">{S.panelTitle}</Text>
        <button className="link-button" onClick={() => void openMain()} data-testid="panel-open-main">
          {S.openInApp}
        </button>
      </header>
      {queue.length === 0 ? (
        <div className="empty panel-empty" role="status">
          {S.panelEmpty}
        </div>
      ) : (
        <div role="listbox" aria-label={S.panelTitle}>
          {queue.map((m, i) => (
            <button
              key={m.id}
              role="option"
              aria-selected={i === idx}
              className={`panel-row${i === idx ? " selected" : ""}`}
              data-testid="panel-row"
              onMouseEnter={() => setIdx(i)}
              onClick={() => setIdx(i)}
            >
              <div className="row-main">
                <span className="row-sender strong">{senderName(m.from[0] ?? null)}</span>
                <span className="row-time">{relativeTime(m.date)}</span>
              </div>
              <div className="row-subject">{m.subject}</div>
              <div className="row-snippet">{m.snippet}</div>
            </button>
          ))}
        </div>
      )}
      <footer className="panel-footer">
        <Text size={100}>
          j/k move · e archive · u read · Enter open · Esc hide
        </Text>
        {queue.length > 0 && <Badge appearance="ghost">{queue.length}</Badge>}
      </footer>
    </div>
  );
}
