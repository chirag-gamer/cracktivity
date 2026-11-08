/**
 * "Waiting for Reply" view (FR-13): threads whose latest message is the
 * user's own and older than the follow-up threshold, with a one-click
 * pre-filled follow-up draft.
 */
import { useCallback, useEffect, useState } from "react";
import { Text } from "@fluentui/react-components";
import { Clock, Reply } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { Avatar, BadgePill } from "../shared/ui";
import { relativeTime } from "../shared/format";
import type { FollowupRow, ViewScope } from "../shared/types";

export interface FollowupsViewProps {
  scope: ViewScope;
  /** Opens the composer pre-filled as an in-thread follow-up (FR-13 AC). */
  onFollowUp: (row: FollowupRow) => void;
}

export function FollowupsView({ scope, onFollowUp }: FollowupsViewProps) {
  const [rows, setRows] = useState<FollowupRow[]>([]);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(() => {
    const accountId = scope.kind === "account" ? scope.id : null;
    ipc
      .followupsList(accountId)
      .then((r) => setRows(r ?? []))
      .catch(() => setRows([]))
      .finally(() => setLoading(false));
  }, [scope]);

  useEffect(() => {
    reload();
  }, [reload]);

  return (
    <div className="pane-list" data-testid="followups-view" role="list">
      {loading && (
        <div className="pane-list empty">
          <div className="empty-state-inner">
            <div className="empty-icon-tile is-pulsing">
              <Clock size={20} />
            </div>
            <div className="empty-title">{S.loading}</div>
          </div>
        </div>
      )}
      {!loading && rows.length === 0 && (
        <div className="pane-list empty" role="status">
          <div className="empty-state-inner">
            <div className="empty-icon-tile">
              <Clock size={20} />
            </div>
            <div className="empty-title">{S.followupsEmpty}</div>
            <div className="empty-subtitle">
              Sent messages awaiting a reply past your threshold will appear here
              automatically.
            </div>
          </div>
        </div>
      )}
      {rows.map((r) => {
        const targetEmail = r.to?.email ?? r.account_email;
        return (
          <div
            key={`${r.account_id}-${r.thrid}`}
            className="row row--tool-card"
            role="listitem"
          >
            <Avatar name={targetEmail} seed={targetEmail} size="sm" />
            <div className="row-body-col">
              <div className="row-main">
                <span className="row-sender strong">{targetEmail}</span>
                <BadgePill variant="warning" size="xs">
                  {S.daysWaiting(r.days_waiting)}
                </BadgePill>
                <span className="row-time">{relativeTime(r.sent_date)}</span>
              </div>
              <div className="row-subject">{r.subject || "(no subject)"}</div>
              <div className="row-snippet">{r.snippet}</div>
              <div className="row-actions">
                <button
                  type="button"
                  className="link-button msg-action-pill"
                  data-testid={`followup-${r.thrid}`}
                  onClick={() => onFollowUp(r)}
                >
                  <Reply size={13} />
                  <span>{S.writeFollowUp}</span>
                </button>
              </div>
            </div>
          </div>
        );
      })}
      {!loading && rows.length > 0 && (
        <div className="tool-list-footer">
          <Text size={200} className="hint">
            {S.followUpDays}: see {S.settings}
          </Text>
        </div>
      )}
    </div>
  );
}
