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
import { relativeTime, addressHue } from "../shared/format";
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
      {loading && <div className="pane-list empty">{S.loading}</div>}
      {!loading && rows.length === 0 && (
        <div className="pane-list empty" role="status">
          <Clock style={{ fontSize: 36, opacity: 0.4, marginBottom: 8 }} />
          <Text weight="semibold" size={400}>{S.followupsEmpty}</Text>
        </div>
      )}
      {rows.map((r) => {
        const targetEmail = r.to?.email ?? r.account_email;
        const initial = (targetEmail || "U")[0].toUpperCase();
        const hue = addressHue(targetEmail);

        return (
          <div key={`${r.account_id}-${r.thrid}`} className="row" role="listitem">
            <div className="row-main">
              <span
                className="row-avatar"
                style={{ background: `hsl(${hue} 65% 42%)` }}
              >
                {initial}
              </span>
              <span className="row-sender strong">
                {targetEmail}
              </span>
              <span className="row-badge">
                {S.daysWaiting(r.days_waiting)}
              </span>
              <span className="row-time">
                {relativeTime(r.sent_date)}
              </span>
            </div>
            <div className="row-subject">{r.subject || "(no subject)"}</div>
            <div className="row-snippet">{r.snippet}</div>
            <div className="row-actions">
              <button
                className="link-button"
                data-testid={`followup-${r.thrid}`}
                onClick={() => onFollowUp(r)}
                style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
              >
                <Reply />
                <span>{S.writeFollowUp}</span>
              </button>
            </div>
          </div>
        );
      })}
      {!loading && rows.length > 0 && (
        <div style={{ padding: "12px 14px" }}>
          <Text size={200} className="hint">
            {S.followUpDays}: see {S.settings}
          </Text>
        </div>
      )}
    </div>
  );
}
