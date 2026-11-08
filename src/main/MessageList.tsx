/**
 * Middle pane: virtualized thread list (NFR-1: 60 fps at 10k+ rows).
 * @tanstack/react-virtual drives absolute-positioned rows inside one
 * scroller; lightweight design-system markup keeps scroll frames under 2ms.
 */
import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Archive,
  Inbox,
  Keyboard,
  Mail,
  Plus,
  RefreshCw,
  Star,
} from "lucide-react";
import { S } from "../shared/strings";
import { Avatar, UiButton } from "../shared/ui";
import { relativeTime, senderName } from "../shared/format";
import type { ThreadRow } from "../shared/types";

export const ROW_HEIGHT = 76;

export interface MessageListProps {
  threads: ThreadRow[];
  selectedThrid: string | null;
  onSelect: (thrid: string) => void;
  loading: boolean;
  hasAccounts?: boolean;
  onAddAccount?: () => void;
  onSyncNow?: () => void;
  onShowAll?: () => void;
  onOpenShortcuts?: () => void;
  onArchiveThread?: (thrid: string) => void;
  onToggleStarThread?: (thrid: string, nextStarred: boolean) => void;
}

export function MessageList({
  threads,
  selectedThrid,
  onSelect,
  loading,
  hasAccounts = true,
  onAddAccount,
  onSyncNow,
  onShowAll,
  onOpenShortcuts,
  onArchiveThread,
  onToggleStarThread,
}: MessageListProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: threads.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  // Keyboard selection must stay visible (j/k past the viewport edge).
  const selectedIndex = threads.findIndex((t) => t.thrid === selectedThrid);
  useEffect(() => {
    if (selectedIndex >= 0) virtualizer.scrollToIndex(selectedIndex);
  }, [selectedIndex, virtualizer]);

  if (loading && threads.length === 0) {
    return (
      <div className="pane-list empty" role="status">
        <div className="empty-state-inner">
          <div className="empty-icon-tile is-pulsing">
            <Mail size={20} />
          </div>
          <div className="empty-title">{S.loading}</div>
          <div className="empty-subtitle">Reading local mail cache…</div>
        </div>
      </div>
    );
  }

  if (threads.length === 0) {
    return (
      <div className="pane-list empty" role="status">
        <div className="empty-state-inner">
          <div className="empty-icon-tile">
            {hasAccounts ? <Inbox size={20} /> : <Mail size={20} />}
          </div>
          <div className="empty-title">{S.noMail}</div>
          <div className="empty-subtitle">
            {!hasAccounts
              ? "Connect your Gmail, iCloud, Zoho, Yahoo, or custom IMAP account to start syncing messages."
              : "You are all caught up in this view. Switch filters or sync to check for new messages."}
          </div>
          <div className="empty-actions">
            {!hasAccounts && onAddAccount && (
              <UiButton
                variant="primary"
                size="sm"
                pill
                icon={<Plus size={14} />}
                onClick={onAddAccount}
              >
                {S.addAccount}
              </UiButton>
            )}
            {hasAccounts && onShowAll && (
              <UiButton variant="secondary" size="sm" pill onClick={onShowAll}>
                View all mail
              </UiButton>
            )}
            {hasAccounts && onSyncNow && (
              <UiButton
                variant="subtle"
                size="sm"
                pill
                icon={<RefreshCw size={13} />}
                onClick={onSyncNow}
              >
                Sync now
              </UiButton>
            )}
            {onOpenShortcuts && (
              <UiButton
                variant="ghost"
                size="sm"
                pill
                icon={<Keyboard size={13} />}
                onClick={onOpenShortcuts}
              >
                Shortcuts
              </UiButton>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className="pane-list"
      ref={parentRef}
      role="listbox"
      aria-label={S.unifiedInbox}
      tabIndex={0}
      aria-activedescendant={
        selectedThrid ? `thread-opt-${selectedThrid}` : undefined
      }
    >
      <div
        style={{
          height: virtualizer.getTotalSize(),
          position: "relative",
        }}
      >
        {virtualizer.getVirtualItems().map((vi) => {
          const t = threads[vi.index]!;
          const unread = t.unread_count > 0;
          const selected = t.thrid === selectedThrid;
          const name = senderName(t.from);

          return (
            <div
              key={t.thrid}
              className={`row-item-wrap${selected ? " is-selected" : ""}${unread ? " is-unread" : ""}`}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: vi.size,
                transform: `translateY(${vi.start}px)`,
              }}
            >
              <div
                role="option"
                id={`thread-opt-${t.thrid}`}
                aria-selected={selected}
                data-testid="thread-row"
                data-thrid={t.thrid}
                className={`row${unread ? " unread" : ""}${selected ? " selected" : ""}`}
                onClick={() => onSelect(t.thrid)}
              >
                <div className="row-lead-col">
                  <span
                    className={`row-unread-dot${unread ? " is-visible" : ""}`}
                    role={unread ? "img" : undefined}
                    aria-label={unread ? "Unread" : undefined}
                    aria-hidden={unread ? undefined : "true"}
                  />
                  <Avatar
                    name={name}
                    seed={t.from?.email ?? name}
                    size="sm"
                    className="row-avatar-col"
                  />
                </div>

                <div className="row-body-col">
                  <div className="row-main">
                    <span className={`row-sender${unread ? " strong" : ""}`}>
                      {name}
                    </span>
                    {t.msg_count > 1 && (
                      <span
                        className="row-count-pill"
                        title={`${t.msg_count} messages in thread`}
                      >
                        {t.msg_count}
                      </span>
                    )}
                    <span className="row-meta-right">
                      {t.starred && (
                        <span className="row-star starred" aria-label="starred">
                          <Star size={12} fill="currentColor" />
                        </span>
                      )}
                      <span className="row-time">
                        {relativeTime(t.last_date)}
                      </span>
                    </span>
                  </div>

                  <div className={`row-subject${unread ? " strong" : ""}`}>
                    {t.subject || "(no subject)"}
                  </div>

                  <div className="row-bottom">
                    <span className="row-snippet">
                      {t.snippet || "No preview text"}
                    </span>
                  </div>
                </div>
              </div>

              {(onArchiveThread || onToggleStarThread) && (
                <div
                  className="row-hover-actions"
                  role="group"
                  aria-label={`Quick actions for ${t.subject || name}`}
                >
                  {onToggleStarThread && (
                    <button
                      type="button"
                      className={`row-quick-btn${t.starred ? " is-starred" : ""}`}
                      title={t.starred ? "Unstar (S)" : "Star (S)"}
                      aria-label={t.starred ? "Unstar thread" : "Star thread"}
                      onClick={(e) => {
                        e.stopPropagation();
                        onToggleStarThread(t.thrid, !t.starred);
                      }}
                    >
                      <Star
                        size={12}
                        fill={t.starred ? "currentColor" : "none"}
                      />
                    </button>
                  )}
                  {onArchiveThread && (
                    <button
                      type="button"
                      className="row-quick-btn"
                      title="Archive (E)"
                      aria-label="Archive thread"
                      onClick={(e) => {
                        e.stopPropagation();
                        onArchiveThread(t.thrid);
                      }}
                    >
                      <Archive size={12} />
                    </button>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
