/**
 * Toolbar: filter tabs (Important/Other/All, FR-11), search box (FR-9 local
 * FTS5), add-account entry (FR-1) and the offline banner (FR-5).
 */
import { useState } from "react";
import {
  Input,
  MessageBar,
  MessageBarBody,
  MessageBarTitle,
} from "@fluentui/react-components";
import { S } from "../shared/strings";
import type { AccountStatus, ImportanceFilter, SearchRow, ThreadRow } from "../shared/types";
import { relativeTime, senderName } from "../shared/format";

export interface ToolbarProps {
  filter: ImportanceFilter;
  onFilter: (f: ImportanceFilter) => void;
  onSearch: (results: SearchRow[] | null) => void;
  onAddAccount: () => void;
  statuses: AccountStatus[];
}

export function Toolbar({ filter, onFilter, onSearch, onAddAccount, statuses }: ToolbarProps) {
  const [q, setQ] = useState("");
  const offline = statuses.some((s) => s.state === "offline");
  const authTrouble = statuses.some((s) => s.state === "auth_failed");

  const run = async (query: string) => {
    setQ(query);
    if (!query.trim()) {
      onSearch(null);
      return;
    }
    try {
      const { ipc } = await import("../shared/ipc");
      onSearch(await ipc.searchMessages(query));
    } catch {
      onSearch([]);
    }
  };

  return (
    <div className="toolbar" data-testid="toolbar">
      <div className="toolbar-row">
        <div className="tabs" role="tablist">
          {(["important", "other", "all"] as const).map((f) => (
            <button
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
              {f === "important" ? S.important : f === "other" ? S.other : S.all}
            </button>
          ))}
        </div>
        <Input
          type="search"
          placeholder={S.search}
          value={q}
          onChange={(_, d) => void run(d.value)}
          data-testid="search-input"
          id="global-search"
        />
        <button className="link-button" onClick={onAddAccount} data-testid="add-account">
          + {S.addAccount}
        </button>
      </div>
      {(offline || authTrouble) && (
        <MessageBar intent={authTrouble ? "error" : "warning"} data-testid="offline-banner">
          <MessageBarBody>
            <MessageBarTitle>
              {authTrouble ? S.authBanner : S.offlineBanner}
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
    <div className="pane-list" data-testid="search-results">
      {results.length === 0 && <div className="empty">{S.noMatches}</div>}
      {results.map((r) => (
        <button
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
          <div className="row-main">
            <span className="row-sender strong">{senderName(r.from)}</span>
            <span className="row-time">{relativeTime(r.date)}</span>
          </div>
          <div className="row-subject">{r.subject}</div>
          <div className="row-snippet">{r.snippet}</div>
        </button>
      ))}
    </div>
  );
}
