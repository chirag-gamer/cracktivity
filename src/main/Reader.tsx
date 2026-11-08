/**
 * Right pane: thread reader (FR-16) with the Phase-4 AI affordances
 * (FR-21/22: summarize + draft reply) and tracking badges (FR-15).
 * HTML bodies render inside a sandboxed iframe (`sandbox=""`. no scripts,
 * unique origin, no IPC access, spec §10.2) whose srcDoc was already
 * sanitized in Rust and carries its own CSP meta (spec §8.4).
 */
import { useEffect, useState } from "react";
import { Text } from "@fluentui/react-components";
import { Archive, Image, Mail, Reply, ShieldCheck, Sparkles, Star } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { senderName, addressHue, relativeTime } from "../shared/format";
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
            <span className="badge">
              <ShieldCheck />
              {body.tracking_count > 0
                ? S.blockedTrackers(body.tracking_count)
                : "No trackers found"}
            </span>
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
                  <Image />
                  <span>{S.showImages}</span>
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

  const fromSender = m.from[0] ?? null;
  const name = senderName(fromSender);
  const initial = (name || "U")[0].toUpperCase();
  const hue = addressHue(fromSender?.email ?? name);

  return (
    <article className="msg-card" data-testid="msg-card" data-message-id={m.id}>
      <header className="msg-head">
        <div className="msg-author-info">
          <div
            className="msg-author-avatar"
            style={{ background: `hsl(${hue} 65% 42%)` }}
          >
            {initial}
          </div>
          <div className="msg-author-details">
            <span className="msg-author-name">{name}</span>
            <span className="msg-to">
              {fromSender?.email && `<${fromSender.email}> `}→ {m.to.map((a) => a.email).join(", ")}
            </span>
          </div>
        </div>

        <div className="msg-meta-right">
          <span className="msg-date-time">{relativeTime(m.date)}</span>
          <div className="msg-actions">
            <button
              className="link-button"
              title={S.toggleUnreadTitle}
              onClick={() => onMarkRead(m)}
            >
              <Mail />
              <span>{m.unread ? S.markRead : S.markUnread}</span>
            </button>
            <button
              className="link-button"
              title={S.toggleStarTitle}
              aria-label={S.toggleStarTitle}
              onClick={() => ipc.setStarred(m.id, !m.starred).catch(() => {})}
            >
              {m.starred ? <Star fill="currentColor" style={{ color: "var(--star-color)" }} /> : <Star />}
              <span>{m.starred ? S.star : S.unstar}</span>
            </button>
            <button
              className="link-button"
              title={S.archiveTitle}
              onClick={() => ipc.archiveMessage(m.id).catch(() => {})}
            >
              <Archive />
              <span>{S.archive}</span>
            </button>
          </div>
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
        className="reader-btn ai-btn"
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
        <Sparkles fill="currentColor" />
        <span>{busy ? S.aiSummarizing : S.aiSummarize}</span>
      </button>
      {summary !== null && (
        <div className="ai-summary-card">
          <div className="ai-summary-head">
            <Sparkles fill="currentColor" />
            <span>AI Thread Summary</span>
          </div>
          <pre className="ai-summary ai-summary-body" data-testid="ai-summary">
            {summary || "Generating summary…"}
          </pre>
        </div>
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
        <Mail style={{ fontSize: 40, opacity: 0.4, marginBottom: 12 }} />
        <Text weight="semibold" size={400}>{S.noSelection}</Text>
      </div>
    );
  }

  const latest = messages[messages.length - 1];
  const quickChips = [
    "Sounds good, thanks!",
    "Let me check and get back to you",
    "Can we talk this week?",
  ];

  return (
    <div className="pane-reader" data-testid="reader">
      <header className="reader-toolbar">
        <div className="reader-action-group">
          <button
            className="reader-btn"
            title="Archive thread (e)"
            onClick={() => {
              if (latest) ipc.archiveMessage(latest.id).catch(() => {});
            }}
          >
            <Archive />
            <span>{S.archive}</span>
            <span className="reader-kbd">e</span>
          </button>
          <button
            className="reader-btn"
            title="Toggle read (u)"
            onClick={() => {
              if (latest) onMarkRead(latest);
            }}
          >
            <Mail />
            <span>{latest?.unread ? S.markRead : S.markUnread}</span>
            <span className="reader-kbd">u</span>
          </button>
          <button
            className="reader-btn"
            title="Star thread (s)"
            onClick={() => {
              if (latest) ipc.setStarred(latest.id, !latest.starred).catch(() => {});
            }}
          >
            {latest?.starred ? <Star fill="currentColor" style={{ color: "var(--star-color)" }} /> : <Star />}
            <span>Star</span>
            <span className="reader-kbd">s</span>
          </button>
          <button
            className="reader-btn"
            title="Reply (r)"
            onClick={() => {
              window.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true }));
            }}
          >
            <Reply />
            <span>Reply</span>
            <span className="reader-kbd">r</span>
          </button>
        </div>

        {aiEnabled && threadId && (
          <div className="reader-action-group" data-testid="ai-bar">
            <SummarizeButton threadId={threadId} />
            <button
              className="reader-btn ai-btn"
              data-testid="ai-draft-reply"
              onClick={() => onDraftReply(threadId, "Reply to this thread")}
            >
              <Sparkles />
              <span>{S.aiDraftReply}</span>
            </button>
          </div>
        )}
      </header>

      <div className="reader-content">
        <h1 className="reader-subject-title">{latest?.subject || "(no subject)"}</h1>

        {messages.map((m) => (
          <MessageCard key={m.id} m={m} onMarkRead={onMarkRead} />
        ))}

        <div className="quick-reply-box">
          <div className="quick-reply-chips">
            <span style={{ fontSize: 11, fontWeight: 600, color: "var(--fg-muted)", marginRight: 4 }}>
              Quick Suggestions:
            </span>
            {quickChips.map((chip) => (
              <button
                key={chip}
                className="quick-chip"
                onClick={() => {
                  if (threadId) onDraftReply(threadId, chip);
                }}
              >
                {chip}
              </button>
            ))}
          </div>
          <div
            className="quick-reply-trigger"
            onClick={() => {
              window.dispatchEvent(new KeyboardEvent("keydown", { key: "r", bubbles: true }));
            }}
          >
            <span>Reply to {senderName(latest?.from[0] ?? null)}…</span>
            <span className="reader-kbd">r</span>
          </div>
        </div>
      </div>
    </div>
  );
}
