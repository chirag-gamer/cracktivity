/**
 * Notch dropdown panel (FR-28, D-011): compact unread triage list. The
 * window itself is created/positioned/animated from Rust; this is only the
 * content: newest unread across accounts, j/k + quick actions, Esc hides.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Text } from "@fluentui/react-components";
import { CheckCircle2, ExternalLink, Inbox } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { Avatar, BadgePill } from "../shared/ui";
import { relativeTime, senderName } from "../shared/format";
import type { MessageRow } from "../shared/types";
import { useKeyboard } from "../main/keyboard";
import {
  onKeymapChange,
  readKeymapScheme,
  syncKeymapScheme,
  type KeymapScheme,
} from "../main/keymap";

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
    .filter((m, i, arr) => arr.findIndex((x) => x.id === m.id) === i)
    .sort((a, b) => b.date - a.date)
    .slice(0, 50);
}

export function PanelApp() {
  const [queue, setQueue] = useState<MessageRow[]>([]);
  const [idx, setIdx] = useState(0);
  const [keymap, setKeymap] = useState<KeymapScheme>(() => readKeymapScheme());
  const queueRef = useRef(queue);
  queueRef.current = queue;

  useEffect(() => {
    syncKeymapScheme(setKeymap);
    return onKeymapChange(setKeymap);
  }, []);

  const refresh = useCallback(() => {
    loadQueue().then(setQueue).catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);

  useEffect(() => {
    document
      .querySelectorAll('[data-testid="panel-row"]')
      .item(idx)
      ?.scrollIntoView({ block: "nearest" });
  }, [idx]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const un1 = await listen("store-changed", refresh);
        const un2 = await listen("panel-visibility", (e) => {
          document.body.classList.toggle("collapsed", e.payload !== true);
          if (e.payload === true) refresh();
        });
        const safe = () => {
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
      compose: () => {},
      reply: () => {},
      focusSearch: () => {},
      toggleOverlay: () => {},
      togglePalette: () => void openMain(),
      accountSlot: () => {},
      escape: () => void ipc.hidePanel().catch(() => {}),
    }),
    [idx, refresh],
  );
  useKeyboard(actions, true, keymap);

  return (
    <div className="panel-shell" data-testid="panel-app">
      <header className="panel-header">
        <div className="panel-header-title">
          <span className="panel-brand-dot">
            <Inbox size={14} />
          </span>
          <span>{S.panelTitle}</span>
        </div>
        <button
          type="button"
          className="link-button panel-open-btn"
          onClick={() => void openMain()}
          data-testid="panel-open-main"
        >
          <span>{S.openInApp}</span>
          <ExternalLink size={12} />
        </button>
      </header>

      {queue.length === 0 ? (
        <div className="empty panel-empty" role="status">
          <CheckCircle2 size={20} style={{ color: "var(--status-success)" }} />
          <span>{S.panelEmpty}</span>
        </div>
      ) : (
        <div role="listbox" aria-label={S.panelTitle} className="panel-list">
          {queue.map((m, i) => {
            const sender = senderName(m.from[0] ?? null);
            return (
              <button
                type="button"
                key={m.id}
                role="option"
                aria-selected={i === idx}
                className={`panel-row${i === idx ? " selected" : ""}`}
                data-testid="panel-row"
                onMouseEnter={() => setIdx(i)}
                onClick={() => setIdx(i)}
              >
                <Avatar
                  name={sender}
                  seed={m.from[0]?.email ?? sender}
                  size="sm"
                />
                <div className="row-body-col">
                  <div className="row-main">
                    <span className="row-sender strong">{sender}</span>
                    <span className="row-time">{relativeTime(m.date)}</span>
                  </div>
                  <div className="row-subject">{m.subject}</div>
                  <div className="row-snippet">{m.snippet}</div>
                </div>
              </button>
            );
          })}
        </div>
      )}

      <footer className="panel-footer">
        <Text size={100}>{S.panelHints}</Text>
        {queue.length > 0 && (
          <BadgePill variant="brand" size="xs">
            {queue.length}
          </BadgePill>
        )}
      </footer>
    </div>
  );
}
