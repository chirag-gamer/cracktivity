/**
 * Right pane: thread reader (FR-16) with the Phase-4 AI affordances
 * (FR-21/22: summarize + draft reply) and tracking badges (FR-15).
 * HTML bodies render inside a sandboxed iframe (`sandbox=""` — no scripts,
 * unique origin, no IPC access, spec §10.2) whose srcDoc was already
 * sanitized in Rust and carries its own CSP meta (spec §8.4).
 */
import { useEffect, useState } from "react";
import { Text } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { senderName } from "../shared/format";
import type { MessageRow, OpenedBody } from "../shared/types";

function MailFrame({
  body,
  messageId,
  onReload,
}: {
  body: OpenedBody;
  messageId: number;
  onReload: () => void;
}) {
  if (body.html) {
    return (
      <div className="mail-frame-wrap">
        {(body.blocked_images > 0 || body.tracking_count > 0) && (
          <div className="badge-row" data-testid="tracking-badge">
            {body.tracking_count > 0 && (
              <Text size={200} className="badge">
                {S.blockedTrackers(body.tracking_count)}
              </Text>
            )}
            {body.blocked_images > 0 && (
              <>
                <button
                  className="link-button"
                  data-testid="show-images"
                  title={S.imagesAllowedNote}
                  onClick={() =>
                    ipc
                      .messageBody(messageId, true)
                      .then(() => onReload())
                      .catch(() => {})
                  }
                >
                  {S.showImages}
                </button>
                <button
                  className="link-button"
                  data-testid="show-images-always"
                  title={S.imagesAllowedNote}
                  onClick={() =>
                    ipc
                      .setSenderImages(messageId, true)
                      .then(() => onReload())
                      .catch(() => {})
                  }
                >
                  {S.showImagesAlways}
                </button>
              </>
            )}
          </div>
        )}
        <iframe
          title={S.mailFrameTitle}
          sandbox=""
          srcDoc={body.html}
          className="mail-frame"
          data-testid="mail-frame"
        />
      </div>
    );
  }
  return <pre className="mail-text">{body.text ?? body.snippet}</pre>;
}

function MessageCard({
  m,
  onMarkRead,
}: {
  m: MessageRow;
  onMarkRead: (m: MessageRow) => void;
}) {
  const [body, setBody] = useState<OpenedBody | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    let live = true;
    if (m.has_body) {
      ipc
        .messageBody(m.id)
        .then((b) => live && setBody(b))
        .catch(() => live && setBody(null));
    } else {
      // Text-only open still latches the FR-14 opened counter.
      ipc.messageOpened(m.id).catch(() => {});
    }
    return () => {
      live = false;
    };
  }, [m.id, m.has_body, reloadKey]);

  return (
    <article className="msg-card" data-testid="msg-card" data-message-id={m.id}>
      <header className="msg-head">
        <div>
          <Text weight="semibold">{senderName(m.from[0] ?? null)}</Text>
          <Text size={200} className="msg-to">
            {" "}
            → {m.to.map((a) => a.email).join(", ")}
          </Text>
        </div>
        <div className="msg-actions">
          <button
            className="link-button"
            title={S.toggleUnreadTitle}
            onClick={() => onMarkRead(m)}
          >
            {m.unread ? S.markRead : S.markUnread}
          </button>
          <button
            className="link-button"
            title={S.toggleStarTitle}
            aria-label={S.toggleStarTitle}
            onClick={() => ipc.setStarred(m.id, !m.starred).catch(() => {})}
          >
            {m.starred ? S.star : S.unstar}
          </button>
          <button
            className="link-button"
            title={S.archiveTitle}
            onClick={() => ipc.archiveMessage(m.id).catch(() => {})}
          >
            {S.archive}
          </button>
        </div>
      </header>
      <div className="msg-subject">{m.subject}</div>
      {body ? (
        <MailFrame
          body={body}
          messageId={m.id}
          onReload={() => setReloadKey((k) => k + 1)}
        />
      ) : (
        <pre className="mail-text">{m.snippet}</pre>
      )}
    </article>
  );
}

function SummarizeButton({ threadId }: { threadId: string }) {
  const [summary, setSummary] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <>
      <button
        className="link-button"
        data-testid="ai-summarize"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          setSummary("");
          ipc
            .aiStream(
              "ai_summarize",
              { threadId },
              (t) => setSummary((s) => (s ?? "") + t),
              (n) => setSummary((s) => `${s ?? ""}\n[${n}]`),
              () => setBusy(false),
            )
            .catch(() => setBusy(false));
        }}
      >
        {busy ? S.aiSummarizing : S.aiSummarize}
      </button>
      {summary !== null && (
        <pre className="ai-summary" data-testid="ai-summary">
          {summary || "…"}
        </pre>
      )}
    </>
  );
}

export interface ReaderProps {
  messages: MessageRow[];
  onMarkRead: (m: MessageRow) => void;
  /** AI master switch state (FR-25: affordances hidden when off). */
  aiEnabled: boolean;
  /** Selected thread id, for the AI actions. */
  threadId: string | null;
  /** Opens the composer with an AI-drafted reply (FR-22). */
  onDraftReply: (threadId: string, instruction: string) => void;
}

export function Reader({
  messages,
  onMarkRead,
  aiEnabled,
  threadId,
  onDraftReply,
}: ReaderProps) {
  if (messages.length === 0) {
    return (
      <div className="pane-reader empty" role="status">
        <Text>{S.noSelection}</Text>
      </div>
    );
  }
  return (
    <div className="pane-reader" data-testid="reader">
      {aiEnabled && threadId && (
        <div className="ai-bar" data-testid="ai-bar">
          <SummarizeButton threadId={threadId} />
          <button
            className="link-button"
            data-testid="ai-draft-reply"
            onClick={() => onDraftReply(threadId, "Reply to this thread")}
          >
            {S.aiDraftReply}
          </button>
        </div>
      )}
      {messages.map((m) => (
        <MessageCard key={m.id} m={m} onMarkRead={onMarkRead} />
      ))}
    </div>
  );
}
