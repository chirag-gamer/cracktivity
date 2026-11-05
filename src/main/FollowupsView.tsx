/**
 * "Waiting for Reply" view (FR-13): threads whose latest message is the
 * user's own and older than the follow-up threshold, with a one-click
 * pre-filled follow-up draft.
 */
import { useCallback, useEffect, useState } from "react";
import { Text } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
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
      {loading && <div className="empty">{S.loading}</div>}
      {!loading && rows.length === 0 && (
        <div className="empty" role="status">
          {S.followupsEmpty}
        </div>
      )}
      {rows.map((r) => (
        <div key={`${r.account_id}-${r.thrid}`} className="row" role="listitem">
          <div className="row-main">
            <span className="row-sender strong">
              {r.to?.email ?? r.account_email}
            </span>
            <span className="row-time">
              {relativeTime(r.sent_date)} · {S.daysWaiting(r.days_waiting)}
            </span>
          </div>
          <div className="row-subject">{r.subject}</div>
          <div className="row-snippet">{r.snippet}</div>
          <div className="row-actions">
            <button
              className="link-button"
              data-testid={`followup-${r.thrid}`}
              onClick={() => onFollowUp(r)}
            >
              {S.writeFollowUp}
            </button>
          </div>
        </div>
      ))}
      {!loading && rows.length > 0 && (
        <Text size={200} className="hint">
          {S.followUpDays}: see {S.settings}
        </Text>
      )}
    </div>
  );
}
