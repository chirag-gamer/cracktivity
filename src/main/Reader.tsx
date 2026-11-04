/**
 * Right pane: thread reader (FR-16). Each message shows header + body;
 * HTML bodies render inside a sandboxed iframe (`sandbox=""` — no scripts,
 * unique origin, no IPC access, spec §10.2) whose srcDoc was already
 * sanitized in Rust and carries its own CSP meta (spec §8.4).
 */
import { useEffect, useState } from "react";
import { Text, Tooltip } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { senderName } from "../shared/format";
import type { MessageRow, OpenedBody } from "../shared/types";

function MailFrame({ body }: { body: OpenedBody }) {
  if (body.html) {
    return (
      <Tooltip content={S.blockedImages(body.blocked_images)} relationship="label">
        <iframe
          title="mail body"
          sandbox=""
          srcDoc={body.html}
          className="mail-frame"
          data-testid="mail-frame"
        />
      </Tooltip>
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

  useEffect(() => {
    let live = true;
    if (m.has_body) {
      ipc
        .messageBody(m.id)
        .then((b) => live && setBody(b))
        .catch(() => live && setBody(null));
    }
    return () => {
      live = false;
    };
  }, [m.id, m.has_body]);

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
            title="Toggle unread (u)"
            onClick={() => onMarkRead(m)}
          >
            {m.unread ? "mark read" : "mark unread"}
          </button>
          <button
            className="link-button"
            title="Toggle star (s)"
            onClick={() => ipc.setStarred(m.id, !m.starred).catch(() => {})}
          >
            {m.starred ? "★" : "☆"}
          </button>
          <button
            className="link-button"
            title="Archive (e)"
            onClick={() => ipc.archiveMessage(m.id).catch(() => {})}
          >
            archive
          </button>
        </div>
      </header>
      <div className="msg-subject">{m.subject}</div>
      {body ? <MailFrame body={body} /> : <pre className="mail-text">{m.snippet}</pre>}
    </article>
  );
}

export interface ReaderProps {
  messages: MessageRow[];
  onMarkRead: (m: MessageRow) => void;
}

export function Reader({ messages, onMarkRead }: ReaderProps) {
  if (messages.length === 0) {
    return (
      <div className="pane-reader empty" role="status">
        <Text>{S.noSelection}</Text>
      </div>
    );
  }
  return (
    <div className="pane-reader" data-testid="reader">
      {messages.map((m) => (
        <MessageCard key={m.id} m={m} onMarkRead={onMarkRead} />
      ))}
    </div>
  );
}
