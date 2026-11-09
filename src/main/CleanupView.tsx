/**
 * Subscription cleanup (FR-14): senders ranked never-opened first. The
 * one-click unsubscribe POST fires ONLY on the button click (RFC 8058
 * no automatic unsubscribe ever); senders without a POST endpoint fall
 * back to opening their mailto: link. Bulk archive is reversible with
 * Undo inside the session.
 */
import { useCallback, useEffect, useState } from "react";
import { Text } from "@fluentui/react-components";
import { Archive, Brush, RotateCcw } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { Avatar, BadgePill } from "../shared/ui";
import type { ArchivedRef, SubscriptionRow } from "../shared/types";

type UnsubState = { sender: string; state: "busy" | "done" | "failed" };

export function CleanupView() {
  const [rows, setRows] = useState<SubscriptionRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [unsub, setUnsub] = useState<UnsubState | null>(null);
  const [undo, setUndo] = useState<ArchivedRef[] | null>(null);
  const [archivedCount, setArchivedCount] = useState<number | null>(null);

  const reload = useCallback(() => {
    ipc
      .cleanupList()
      .then((r) => setRows(r ?? []))
      .catch(() => setRows([]))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const doUnsubscribe = (row: SubscriptionRow) => {
    setUnsub({ sender: row.sender_email, state: "busy" });
    if (row.list_unsub_post && row.list_unsubscribe) {
      ipc
        .unsubscribeSender(
          row.sender_email,
          row.list_unsubscribe,
          row.list_unsub_post,
        )
        .then(() => setUnsub({ sender: row.sender_email, state: "done" }))
        .catch(() => setUnsub({ sender: row.sender_email, state: "failed" }));
    } else if (row.list_unsubscribe) {
      const links = parseLinks(row.list_unsubscribe);
      if (links.mailto) {
        ipc
          .openMailto(links.mailto)
          .then(() => setUnsub({ sender: row.sender_email, state: "done" }))
          .catch(() => setUnsub({ sender: row.sender_email, state: "failed" }));
      } else {
        setUnsub({ sender: row.sender_email, state: "failed" });
      }
    }
  };

  const doBulkArchive = (row: SubscriptionRow) => {
    ipc
      .bulkArchiveSender(row.sender_email)
      .then((r) => {
        setUndo(r.undo);
        setArchivedCount(r.archived);
        reload();
      })
      .catch(() => {});
  };

  const doUndo = () => {
    const refs = undo;
    if (!refs) return;
    ipc
      .undoBulkArchive(refs)
      .then(() => {
        setUndo(null);
        setArchivedCount(null);
        reload();
      })
      .catch(() => {});
  };

  return (
    <div className="pane-list" data-testid="cleanup-view" role="list">
      {archivedCount !== null && (
        <div className="undo-bar" data-testid="undo-bar">
          <Archive size={14} />
          <span>{S.archived(archivedCount)}</span>
          <button
            type="button"
            className="link-button msg-action-pill"
            onClick={doUndo}
            data-testid="undo-archive"
          >
            <RotateCcw size={13} />
            <span>{S.undo}</span>
          </button>
        </div>
      )}
      {loading && (
        <div className="pane-list empty">
          <div className="empty-state-inner">
            <div className="empty-icon-tile is-pulsing">
              <Brush size={20} />
            </div>
            <div className="empty-title">{S.loading}</div>
          </div>
        </div>
      )}
      {!loading && rows.length === 0 && (
        <div className="pane-list empty" role="status">
          <div className="empty-state-inner">
            <div className="empty-icon-tile">
              <Brush size={20} />
            </div>
            <div className="empty-title">{S.cleanupEmpty}</div>
            <div className="empty-subtitle">
              Mailing lists with RFC 8058 List-Unsubscribe headers will be
              ranked here by open rate.
            </div>
          </div>
        </div>
      )}
      {rows.map((r) => {
        const state = unsub?.sender === r.sender_email ? unsub.state : null;
        const displayName = r.sender_name ?? r.sender_email;

        return (
          <div
            key={`${r.account_id}-${r.sender_email}`}
            className="row row--tool-card"
            role="listitem"
          >
            <Avatar name={displayName} seed={r.sender_email} size="sm" />
            <div className="row-body-col">
              <div className="row-main">
                <span className="row-sender strong">{displayName}</span>
                <BadgePill
                  variant={r.opened === 0 ? "warning" : "neutral"}
                  size="xs"
                >
                  {S.cleanupOpened(r.opened, r.received)}
                </BadgePill>
                <span className="row-time">
                  {S.cleanupReceived(r.received)}
                </span>
              </div>
              <div className="row-snippet">{r.sender_email}</div>
              <div className="row-actions">
                <button
                  type="button"
                  className="link-button msg-action-pill"
                  data-testid={`unsub-${r.sender_email}`}
                  disabled={state === "busy" || !r.list_unsubscribe}
                  onClick={() => doUnsubscribe(r)}
                >
                  <Brush size={13} />
                  <span>
                    {state === "busy"
                      ? S.unsubscribing
                      : state === "done"
                        ? S.unsubscribed
                        : r.list_unsub_post
                          ? S.unsubscribe
                          : S.unsubscribeMailto}
                  </span>
                </button>
                <button
                  type="button"
                  className="link-button msg-action-pill"
                  data-testid={`archive-${r.sender_email}`}
                  onClick={() => doBulkArchive(r)}
                >
                  <Archive size={13} />
                  <span>{S.archiveAll}</span>
                </button>
              </div>
              {state === "failed" && (
                <Text size={200} className="error-hint">
                  {S.unsubscribeFailed}
                </Text>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** Splits a List-Unsubscribe header into https / mailto URIs. */
function parseLinks(header: string): { https?: string; mailto?: string } {
  let https: string | undefined;
  let mailto: string | undefined;
  for (const part of header.split(",")) {
    const uri = part.trim().replace(/^</, "").replace(/>$/, "").trim();
    const lower = uri.toLowerCase();
    if (!https && lower.startsWith("https://")) https = uri;
    else if (!mailto && lower.startsWith("mailto:")) mailto = uri;
  }
  return { https, mailto };
}
