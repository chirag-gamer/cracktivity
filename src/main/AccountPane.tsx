/** Left pane: unified inbox + accounts with Ctrl+slot hints (FR-4/FR-11). */
import { Brush, CircleX, Clock, Mail, RefreshCw, Settings, SquarePen, TriangleAlert } from "lucide-react";
import { S } from "../shared/strings";
import { addressHue } from "../shared/format";
import type { AccountStatus, AccountSummary, ViewScope } from "../shared/types";

export interface AccountPaneProps {
  accounts: AccountSummary[];
  statuses: AccountStatus[];
  scope: ViewScope;
  onScope: (s: ViewScope) => void;
  onCompose?: () => void;
}

export function AccountPane({ accounts, statuses, scope, onScope, onCompose }: AccountPaneProps) {
  const totalUnread = accounts.reduce((n, a) => n + a.unread, 0);
  const statusOf = (id: number) => statuses.find((s) => s.account_id === id);

  const handleCompose = () => {
    if (onCompose) {
      onCompose();
    } else {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "c", bubbles: true }));
    }
  };

  return (
    <nav className="pane-accounts" aria-label="Accounts" data-testid="account-pane">
      <div className="sidebar-header">
        <div className="brand-badge">
          <div className="brand-icon-box">
            <Mail fill="currentColor" />
          </div>
          <span>cracktivity</span>
        </div>
        <span className="brand-status-dot" title="Local direct connection active" />
      </div>

      <button
        className="compose-button"
        onClick={handleCompose}
        title="Compose new email (C)"
        aria-label="Compose new email"
      >
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
          <SquarePen />
          <span>New Message</span>
        </span>
        <span className="compose-kbd">C</span>
      </button>

      <button
        className={`nav-item${scope.kind === "unified" ? " active" : ""}`}
        onClick={() => onScope({ kind: "unified" })}
        data-testid="nav-unified"
      >
        <span className="nav-icon">
          {scope.kind === "unified" ? <Mail fill="currentColor" /> : <Mail />}
        </span>
        <span className="nav-label">{S.unifiedInbox}</span>
        {totalUnread > 0 && <span className="nav-count">{totalUnread}</span>}
      </button>

      {accounts.length > 0 && (
        <div className="sidebar-section-title">Accounts</div>
      )}

      {accounts.map((a, i) => {
        const st = statusOf(a.id);
        const active = scope.kind === "account" && scope.id === a.id;
        const state = st?.state ?? "idle";
        const initial = (a.display_name || a.email || "A")[0].toUpperCase();
        const hue = addressHue(a.email);

        return (
          <button
            key={a.id}
            className={`nav-item${active ? " active" : ""}`}
            onClick={() => onScope({ kind: "account", id: a.id })}
            data-testid={`nav-account-${a.id}`}
          >
            <span
              className="nav-avatar"
              style={{ background: `hsl(${hue} 65% 45%)` }}
            >
              {initial}
            </span>
            <span className="nav-label">
              {a.display_name ?? a.email}
              {i < 9 && (
                <span className="nav-slot">
                  Ctrl+{i + 1}
                </span>
              )}
            </span>
            {a.unread > 0 && <span className="nav-count">{a.unread}</span>}
            {state === "offline" && (
              <span className="nav-state offline" title={st?.error ?? "Offline"}>
                <TriangleAlert />
              </span>
            )}
            {state === "auth_failed" && (
              <span className="nav-state error" title={st?.error ?? "Auth failed"}>
                <CircleX />
              </span>
            )}
            {state === "syncing" && (
              <span className="nav-state syncing" title="Syncing">
                <RefreshCw />
              </span>
            )}
          </button>
        );
      })}

      <div className="nav-tools">
        <div className="sidebar-section-title">Smart Views</div>
        <button
          className={`nav-item${scope.kind === "followups" ? " active" : ""}`}
          onClick={() => onScope({ kind: "followups" })}
          data-testid="nav-followups"
        >
          <span className="nav-icon">
            <Clock />
          </span>
          <span className="nav-label">{S.followups}</span>
        </button>
        <button
          className={`nav-item${scope.kind === "cleanup" ? " active" : ""}`}
          onClick={() => onScope({ kind: "cleanup" })}
          data-testid="nav-cleanup"
        >
          <span className="nav-icon">
            <Brush />
          </span>
          <span className="nav-label">{S.cleanup}</span>
        </button>
        <button
          className={`nav-item${scope.kind === "settings" ? " active" : ""}`}
          onClick={() => onScope({ kind: "settings" })}
          data-testid="nav-settings"
        >
          <span className="nav-icon">
            <Settings />
          </span>
          <span className="nav-label">{S.settings}</span>
        </button>
      </div>
    </nav>
  );
}
