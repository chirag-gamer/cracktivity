/** Left pane: unified inbox + accounts with Ctrl+slot hints (FR-4/FR-11). */
import { Text } from "@fluentui/react-components";
import { S } from "../shared/strings";
import { addressHue } from "../shared/format";
import type { AccountStatus, AccountSummary, ViewScope } from "../shared/types";

export interface AccountPaneProps {
  accounts: AccountSummary[];
  statuses: AccountStatus[];
  scope: ViewScope;
  onScope: (s: ViewScope) => void;
}

export function AccountPane({ accounts, statuses, scope, onScope }: AccountPaneProps) {
  const totalUnread = accounts.reduce((n, a) => n + a.unread, 0);
  const statusOf = (id: number) => statuses.find((s) => s.account_id === id);

  return (
    <nav className="pane-accounts" aria-label="Accounts" data-testid="account-pane">
      <button
        className={`nav-item${scope.kind === "unified" ? " active" : ""}`}
        onClick={() => onScope({ kind: "unified" })}
        data-testid="nav-unified"
      >
        <span className="nav-dot all" />
        <span className="nav-label">{S.unifiedInbox}</span>
        {totalUnread > 0 && <span className="nav-count">{totalUnread}</span>}
      </button>

      {accounts.map((a, i) => {
        const st = statusOf(a.id);
        const active = scope.kind === "account" && scope.id === a.id;
        const state = st?.state ?? "idle";
        return (
          <button
            key={a.id}
            className={`nav-item${active ? " active" : ""}`}
            onClick={() => onScope({ kind: "account", id: a.id })}
            data-testid={`nav-account-${a.id}`}
          >
            <span
              className="nav-dot"
              style={{ background: `hsl(${addressHue(a.email)} 60% 45%)` }}
            />
            <span className="nav-label">
              {a.display_name ?? a.email}
              {i < 9 && (
                <Text size={100} className="nav-slot">
                  {" "}
                  Ctrl+{i + 1}
                </Text>
              )}
            </span>
            {a.unread > 0 && <span className="nav-count">{a.unread}</span>}
            {state === "offline" && <span className="nav-state" title={st?.error ?? ""}>⚠</span>}
            {state === "auth_failed" && <span className="nav-state" title={st?.error ?? ""}>⛔</span>}
            {state === "syncing" && <span className="nav-state">⟳</span>}
          </button>
        );
      })}
    </nav>
  );
}
