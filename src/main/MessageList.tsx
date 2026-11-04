/**
 * Middle pane: virtualized thread list (NFR-1: 60 fps at 10k+ rows).
 * @tanstack/react-virtual drives absolute-positioned rows inside one
 * scroller; Fluent paints them (no v9 virtualized list exists — verified).
 */
import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Text, Badge } from "@fluentui/react-components";
import { S } from "../shared/strings";
import { relativeTime, senderName } from "../shared/format";
import type { ThreadRow } from "../shared/types";

export const ROW_HEIGHT = 68;

export interface MessageListProps {
  threads: ThreadRow[];
  selectedThrid: string | null;
  onSelect: (thrid: string) => void;
  loading: boolean;
}

export function MessageList({
  threads,
  selectedThrid,
  onSelect,
  loading,
}: MessageListProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: threads.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  if (loading && threads.length === 0) {
    return (
      <div className="pane-list empty" role="status">
        <Text>{S.loading}</Text>
      </div>
    );
  }
  if (threads.length === 0) {
    return (
      <div className="pane-list empty" role="status">
        <Text>{S.noMail}</Text>
      </div>
    );
  }

  return (
    <div className="pane-list" ref={parentRef} role="listbox" aria-label={S.unifiedInbox}>
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((vi) => {
          const t = threads[vi.index];
          const unread = t.unread_count > 0;
          const selected = t.thrid === selectedThrid;
          return (
            <div
              key={t.thrid}
              role="option"
              aria-selected={selected}
              data-testid="thread-row"
              data-thrid={t.thrid}
              className={`row${unread ? " unread" : ""}${selected ? " selected" : ""}`}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: vi.size,
                transform: `translateY(${vi.start}px)`,
              }}
              onClick={() => onSelect(t.thrid)}
            >
              <div className="row-main">
                <span className={`row-sender${unread ? " strong" : ""}`}>
                  {senderName(t.from)}
                </span>
                {t.starred && <span aria-label="starred">★</span>}
                <span className="row-time">{relativeTime(t.last_date)}</span>
              </div>
              <div className={`row-subject${unread ? " strong" : ""}`}>
                {t.subject}
              </div>
              <div className="row-bottom">
                <span className="row-snippet">{t.snippet}</span>
                {t.msg_count > 1 && (
                  <Badge appearance="ghost" size="small">
                    {t.msg_count}
                  </Badge>
                )}
                {unread && <Badge appearance="filled" size="small">{t.unread_count}</Badge>}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
