/**
 * Toolbar: filter tabs (Important/Other/All, FR-11), search box (FR-9 local
 * FTS5), add-account entry (FR-1) and the offline banner (FR-5).
 */
import { useState } from "react";
import {
  MessageBar,
  MessageBarBody,
  MessageBarTitle,
} from "@fluentui/react-components";
import {
  CircleAlert,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  TriangleAlert,
  X,
} from "lucide-react";
import { S } from "../shared/strings";
import { Avatar, BadgePill, IconButton, Kbd } from "../shared/ui";
import type {
  AccountStatus,
  ImportanceFilter,
  SearchRow,
  ThreadRow,
} from "../shared/types";
import { relativeTime, senderName } from "../shared/format";

export interface ToolbarProps {
  filter: ImportanceFilter;
  onFilter: (f: ImportanceFilter) => void;
  onSearch: (results: SearchRow[] | null) => void;
  onAddAccount: () => void;
  statuses: AccountStatus[];
  /** FR-9 completion: extend results with server-side IMAP SEARCH. */
  alsoServer?: boolean;
  title?: string;
  threadCount?: number;
  unreadCount?: number;
  onSyncNow?: () => void;
}

export function Toolbar({
  filter,
  onFilter,
  onSearch,
  onAddAccount,
  statuses,
  alsoServer,
  title = "Inbox",
  threadCount,
  unreadCount,
  onSyncNow,
}: ToolbarProps) {
  const [q, setQ] = useState("");
  const offline = statuses.some((s) => s.state === "offline");
  const authTrouble = statuses.some((s) => s.state === "auth_failed");
  const syncing = statuses.some((s) => s.state === "syncing");

  const run = async (query: string) => {
    setQ(query);
    if (!query.trim()) {
      onSearch(null);
      return;
    }
    try {
      const { ipc } = await import("../shared/ipc");
      onSearch(await ipc.searchMessages(query, alsoServer));
    } catch {
      onSearch([]);
    }
  };

  return (
    <div className="toolbar" data-testid="toolbar">
      <div className="toolbar-top-row">
        <div className="toolbar-title-group">
          <h2 className="toolbar-view-title">{title}</h2>
          {unreadCount !== undefined && unreadCount > 0 ? (
            <BadgePill variant="brand" size="xs">
              {unreadCount} unread
            </BadgePill>
          ) : threadCount !== undefined && threadCount > 0 ? (
            <span className="toolbar-count-caption">{threadCount}</span>
          ) : null}
        </div>

        <div className="toolbar-actions">
          {onSyncNow && (
            <IconButton
              label="Sync mail now"
              size="xs"
              variant="subtle"
              onClick={onSyncNow}
              className={syncing ? "is-spinning" : ""}
            >
              <RefreshCw size={13} />
            </IconButton>
          )}
          <button
            type="button"
            className="link-button toolbar-add-btn"
            onClick={onAddAccount}
            data-testid="add-account"
            title={S.addAccount}
          >
            <Plus size={13} />
            <span>{S.addAccount}</span>
          </button>
        </div>
      </div>

      <div className="toolbar-row">
        <div className="tabs" role="tablist" aria-label="Importance filter">
          {(["important", "other", "all"] as const).map((f) => (
            <button
              type="button"
              key={f}
              role="tab"
              aria-selected={filter === f}
              className={`tab${filter === f ? " active" : ""}`}
              onClick={() => {
                onFilter(f);
                void run("");
              }}
              data-testid={`filter-${f}`}
            >
              {f === "important" && (
                <Sparkles size={11} className="tab-sparkle" aria-hidden="true" />
              )}
              <span>
                {f === "important" ? S.important : f === "other" ? S.other : S.all}
              </span>
            </button>
          ))}
        </div>
      </div>

      <div className="toolbar-search-row">
        <div className="search-wrap">
          <span className="search-icon-badge" aria-hidden="true">
            <Search size={14} />
          </span>
          <input
            type="search"
            placeholder={S.search}
            value={q}
            onChange={(e) => void run(e.target.value)}
            data-testid="search-input"
            id="global-search"
            aria-label={S.search}
          />
          {q ? (
            <button
              type="button"
              className="search-clear-btn"
              onClick={() => void run("")}
              aria-label="Clear search"
              title="Clear search"
            >
              <X size={12} />
            </button>
          ) : (
            <Kbd size="xs" className="search-kbd-badge">
              /
            </Kbd>
          )}
        </div>
      </div>

      {(offline || authTrouble) && (
        <MessageBar
          intent={authTrouble ? "error" : "warning"}
          data-testid="offline-banner"
          className="toolbar-banner"
        >
          <MessageBarBody>
            <MessageBarTitle
              style={{ display: "flex", alignItems: "center", gap: 6 }}
            >
              {authTrouble ? (
                <CircleAlert size={14} />
              ) : (
                <TriangleAlert size={14} />
              )}
              <span>{authTrouble ? S.authBanner : S.offlineBanner}</span>
            </MessageBarTitle>
          </MessageBarBody>
        </MessageBar>
      )}
    </div>
  );
}

/** Search results replace the thread list content (FR-9 local-only v1). */
export function SearchResults({
  results,
  onOpen,
}: {
  results: SearchRow[];
  onOpen: (t: ThreadRow) => void;
}) {
  return (
    <div
      className="pane-list"
      role="listbox"
      aria-label={S.search}
      data-testid="search-results"
    >
      {results.length === 0 && (
        <div className="pane-list empty">
          <div className="empty-state-inner">
            <div className="empty-icon-tile">
              <Search size={18} />
            </div>
            <div className="empty-title">{S.noMatches}</div>
            <div className="empty-subtitle">
              Try searching by sender name, subject keyword, or phrase.
            </div>
          </div>
        </div>
      )}
      {results.map((r) => {
        const sName = senderName(r.from);
        return (
          <button
            type="button"
            key={r.message_id}
            className="row search-row"
            onClick={() =>
              onOpen({
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
              })
            }
          >
            <Avatar name={sName} seed={r.from?.email ?? sName} size="sm" />
            <div className="row-content">
              <div className="row-main">
                <span className="row-sender strong">{sName}</span>
                <span className="row-time">{relativeTime(r.date)}</span>
              </div>
              <div className="row-subject">{r.subject}</div>
              <div className="row-snippet">
                {r.snippet}
                {!r.has_body && (
                  <span className="row-uncached"> · {S.uncached}</span>
                )}
              </div>
            </div>
          </button>
        );
      })}
    </div>
  );
}
