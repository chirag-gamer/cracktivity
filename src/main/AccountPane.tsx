/** Left pane: unified inbox + accounts with Ctrl+slot hints (FR-4/FR-11). */
import {
  Brush,
  CircleX,
  Clock,
  Inbox,
  Keyboard,
  Mail,
  Plus,
  RefreshCw,
  Search,
  Settings,
  SquarePen,
  TriangleAlert,
} from "lucide-react";
import { S } from "../shared/strings";
import { Avatar, IconButton, Kbd } from "../shared/ui";
import type { AccountStatus, AccountSummary, ViewScope } from "../shared/types";

export interface AccountPaneProps {
  accounts: AccountSummary[];
  statuses: AccountStatus[];
  scope: ViewScope;
  onScope: (s: ViewScope) => void;
  onCompose?: () => void;
  onAddAccount?: () => void;
  onOpenPalette?: () => void;
  onOpenShortcuts?: () => void;
}

export function AccountPane({
  accounts,
  statuses,
  scope,
  onScope,
  onCompose,
  onAddAccount,
  onOpenPalette,
  onOpenShortcuts,
}: AccountPaneProps) {
  const totalUnread = accounts.reduce((n, a) => n + a.unread, 0);
  const statusOf = (id: number) => statuses.find((s) => s.account_id === id);
  const hasAuthError = statuses.some((s) => s.state === "auth_failed");
  const hasOffline = statuses.some((s) => s.state === "offline");
  const isSyncing = statuses.some((s) => s.state === "syncing");
  const isConnecting = statuses.some((s) => s.state === "connecting");
  const isConnected =
    statuses.length > 0 &&
    !hasAuthError &&
    !hasOffline &&
    !isSyncing &&
    !isConnecting &&
    statuses.some((s) => s.state === "ok");

  const statusSummary = hasAuthError
    ? {
        dotClass: " is-error",
        sub: "Auth attention",
        label: "Account authentication needs attention",
      }
    : hasOffline
      ? {
          dotClass: " is-offline",
          sub: "Offline",
          label: "Working offline from local cache",
        }
      : isSyncing
        ? {
            dotClass: " is-syncing",
            sub: "Syncing…",
            label: "Syncing mail with server",
          }
        : isConnecting
          ? {
              dotClass: " is-syncing",
              sub: "Connecting…",
              label: "Connecting to mail server",
            }
          : isConnected
            ? {
                dotClass: " is-ok",
                sub: "Connected",
                label: "Local direct IMAP/SMTP connection active",
              }
            : {
                dotClass: " is-idle",
                sub: accounts.length === 0 ? "No accounts" : "Ready",
                label:
                  accounts.length === 0
                    ? "No mail accounts connected yet"
                    : "Waiting for connection status",
              };

  const handleCompose = () => {
    if (onCompose) {
      onCompose();
    } else {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "c", bubbles: true }),
      );
    }
  };

  return (
    <nav
      className="pane-accounts"
      aria-label="Accounts"
      data-testid="account-pane"
    >
      <div className="sidebar-header">
        <div className="brand-badge">
          <div className="brand-icon-box" aria-hidden="true">
            <Mail />
          </div>
          <div className="brand-text-col">
            <span className="brand-name">cracktivity</span>
            <span className="brand-sub">{statusSummary.sub}</span>
          </div>
        </div>
        <div className="sidebar-header-actions">
          <span
            role="img"
            className={`brand-status-dot${statusSummary.dotClass}`}
            title={statusSummary.label}
            aria-label={statusSummary.label}
          />
          {onOpenPalette && (
            <IconButton
              label="Command palette (Ctrl+K)"
              size="xs"
              variant="ghost"
              onClick={onOpenPalette}
            >
              <Search />
            </IconButton>
          )}
        </div>
      </div>

      <div className="compose-row">
        <button
          type="button"
          className="compose-button"
          onClick={handleCompose}
          title="Compose new email (C)"
          aria-label="Compose new email"
        >
          <span className="compose-button__left">
            <SquarePen />
            <span>New Message</span>
          </span>
          <Kbd size="xs" className="compose-kbd">
            C
          </Kbd>
        </button>
      </div>

      <div className="sidebar-scroll">
        <div className="sidebar-section">
          <button
            type="button"
            className={`nav-item${scope.kind === "unified" ? " active" : ""}`}
            onClick={() => onScope({ kind: "unified" })}
            aria-current={scope.kind === "unified" ? "page" : undefined}
            data-testid="nav-unified"
          >
            <span className="nav-icon">
              <Inbox />
            </span>
            <span className="nav-label">{S.unifiedInbox}</span>
            {totalUnread > 0 && (
              <span className="nav-count">{totalUnread}</span>
            )}
          </button>
        </div>

        <div className="sidebar-section">
          <div className="sidebar-section-header">
            <span className="sidebar-section-title">Accounts</span>
            {onAddAccount && (
              <button
                type="button"
                className="sidebar-section-action"
                onClick={onAddAccount}
                title="Add mail account"
                aria-label="Add mail account"
              >
                <Plus />
              </button>
            )}
          </div>

          {accounts.length === 0 ? (
            <div className="sidebar-empty-accounts">
              <span>No accounts linked</span>
              {onAddAccount && (
                <button
                  type="button"
                  className="sidebar-inline-link"
                  onClick={onAddAccount}
                >
                  + Connect account
                </button>
              )}
            </div>
          ) : (
            accounts.map((a, i) => {
              const st = statusOf(a.id);
              const active = scope.kind === "account" && scope.id === a.id;
              const state = st?.state ?? "idle";
              const label = a.display_name || a.email;
              const showEmailSub =
                Boolean(a.display_name) &&
                a.display_name !== a.email &&
                Boolean(a.email);

              return (
                <button
                  type="button"
                  key={a.id}
                  className={`nav-item nav-item--account${active ? " active" : ""}`}
                  onClick={() => onScope({ kind: "account", id: a.id })}
                  aria-current={active ? "page" : undefined}
                  data-testid={`nav-account-${a.id}`}
                >
                  <Avatar name={label} seed={a.email} size="xs" />
                  <span className="nav-label-stack">
                    <span className="nav-label">
                      <span className="nav-label-text">{label}</span>
                      {i < 9 && <span className="nav-slot">Ctrl+{i + 1}</span>}
                    </span>
                    {showEmailSub && (
                      <span className="nav-sublabel">{a.email}</span>
                    )}
                  </span>
                  {a.unread > 0 && (
                    <span className="nav-count">{a.unread}</span>
                  )}
                  {state === "offline" && (
                    <span
                      role="img"
                      className="nav-state offline"
                      title={st?.error ?? "Offline"}
                      aria-label={st?.error ?? "Offline"}
                    >
                      <TriangleAlert />
                    </span>
                  )}
                  {state === "auth_failed" && (
                    <span
                      role="img"
                      className="nav-state error"
                      title={st?.error ?? "Auth failed"}
                      aria-label={st?.error ?? "Authentication failed"}
                    >
                      <CircleX />
                    </span>
                  )}
                  {state === "syncing" && (
                    <span
                      role="img"
                      className="nav-state syncing"
                      title="Syncing"
                      aria-label="Syncing"
                    >
                      <RefreshCw />
                    </span>
                  )}
                </button>
              );
            })
          )}
        </div>

        <div className="sidebar-section">
          <div className="sidebar-section-header">
            <span className="sidebar-section-title">Smart Views</span>
          </div>
          <button
            type="button"
            className={`nav-item${scope.kind === "followups" ? " active" : ""}`}
            onClick={() => onScope({ kind: "followups" })}
            aria-current={scope.kind === "followups" ? "page" : undefined}
            data-testid="nav-followups"
          >
            <span className="nav-icon">
              <Clock />
            </span>
            <span className="nav-label">{S.followups}</span>
          </button>
          <button
            type="button"
            className={`nav-item${scope.kind === "cleanup" ? " active" : ""}`}
            onClick={() => onScope({ kind: "cleanup" })}
            aria-current={scope.kind === "cleanup" ? "page" : undefined}
            data-testid="nav-cleanup"
          >
            <span className="nav-icon">
              <Brush />
            </span>
            <span className="nav-label">{S.cleanup}</span>
          </button>
        </div>
      </div>

      <div className="nav-tools">
        <div className="nav-footer-row">
          <button
            type="button"
            className={`nav-item nav-item--footer${scope.kind === "settings" ? " active" : ""}`}
            onClick={() => onScope({ kind: "settings" })}
            aria-current={scope.kind === "settings" ? "page" : undefined}
            data-testid="nav-settings"
          >
            <span className="nav-icon">
              <Settings />
            </span>
            <span className="nav-label">{S.settings}</span>
          </button>
          {onOpenShortcuts && (
            <IconButton
              label="Keyboard shortcuts (?)"
              size="sm"
              variant="ghost"
              onClick={onOpenShortcuts}
            >
              <Keyboard />
            </IconButton>
          )}
        </div>
      </div>
    </nav>
  );
}
