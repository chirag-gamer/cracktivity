/**
 * Right pane: thread reader (FR-16) with the Phase-4 AI affordances
 * (FR-21/22: summarize + draft reply) and tracking badges (FR-15).
 * HTML bodies render inside a sandboxed iframe (`sandbox=""`. no scripts,
 * unique origin, no IPC access, spec §10.2) whose srcDoc was already
 * sanitized in Rust and carries its own CSP meta (spec §8.4).
 */
import { useEffect, useRef, useState } from "react";
import {
  Archive,
  Image,
  Keyboard,
  Mail,
  Reply,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  SquarePen,
  Star,
  X,
} from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { Avatar, BadgePill, Kbd, UiButton } from "../shared/ui";
import { senderName, relativeTime } from "../shared/format";
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
            <span className="badge badge--shield">
              <ShieldCheck size={14} />
              <span>
                {body.tracking_count > 0
                  ? S.blockedTrackers(body.tracking_count)
                  : "No trackers found"}
              </span>
            </span>
            {body.blocked_images > 0 && (
              <div className="badge-row__actions">
                <button
                  type="button"
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
                  <Image size={13} />
                  <span>{S.showImages}</span>
                </button>
                <button
                  type="button"
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
                  <span>{S.showImagesAlways}</span>
                </button>
              </div>
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
  threadSubject,
  onMarkRead,
  onRefresh,
}: {
  m: MessageRow;
  threadSubject?: string | null;
  onMarkRead: (m: MessageRow) => void;
  onRefresh?: () => void;
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
  const showCardSubject =
    Boolean(m.subject) &&
    threadSubject !== undefined &&
    threadSubject !== null &&
    (m.subject ?? "").trim() !== threadSubject.trim();

  return (
    <article className="msg-card" data-testid="msg-card" data-message-id={m.id}>
      <header className="msg-head">
        <div className="msg-author-info">
          <Avatar name={name} seed={fromSender?.email ?? name} size="md" />
          <div className="msg-author-details">
            <div className="msg-author-top">
              <span className="msg-author-name">{name}</span>
              {fromSender?.email && (
                <span className="msg-email-pill">{fromSender.email}</span>
              )}
            </div>
            <span className="msg-to">
              to {m.to.map((a) => a.name || a.email).join(", ") || "me"}
            </span>
          </div>
        </div>

        <div className="msg-meta-right">
          <span className="msg-date-time">{relativeTime(m.date)}</span>
          <div className="msg-actions">
            <button
              type="button"
              className="link-button msg-action-pill"
              title={S.toggleUnreadTitle}
              onClick={() => onMarkRead(m)}
            >
              <Mail size={13} />
              <span>{m.unread ? S.markRead : S.markUnread}</span>
            </button>
            <button
              type="button"
              className={`link-button msg-action-pill${m.starred ? " is-starred" : ""}`}
              title={S.toggleStarTitle}
              aria-label={S.toggleStarTitle}
              onClick={() =>
                ipc
                  .setStarred(m.id, !m.starred)
                  .then(() => onRefresh?.())
                  .catch(() => {})
              }
            >
              <Star
                size={13}
                fill={m.starred ? "currentColor" : "none"}
                style={m.starred ? { color: "var(--star-color)" } : undefined}
              />
              <span>{m.starred ? S.star : S.unstar}</span>
            </button>
            <button
              type="button"
              className="link-button msg-action-pill"
              title={S.archiveTitle}
              onClick={() =>
                ipc
                  .archiveMessage(m.id)
                  .then(() => onRefresh?.())
                  .catch(() => {})
              }
            >
              <Archive size={13} />
              <span>{S.archive}</span>
            </button>
          </div>
        </div>
      </header>

      {showCardSubject && <div className="msg-subject">{m.subject}</div>}

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

export interface ReaderProps {
  messages: MessageRow[];
  onMarkRead: (m: MessageRow) => void;
  /** AI master switch state (FR-25: affordances hidden when off). */
  aiEnabled: boolean;
  /** Selected thread id, for the AI actions. */
  threadId: string | null;
  /** Opens the composer with an AI-drafted reply (FR-22). */
  onDraftReply: (threadId: string, instruction: string) => void;
  onReply?: () => void;
  onArchiveThread?: () => void;
  onToggleStarThread?: () => void;
  onRefresh?: () => void;
  onCompose?: () => void;
  onFocusSearch?: () => void;
  onOpenPalette?: () => void;
  onOpenShortcuts?: () => void;
}

export function Reader({
  messages,
  onMarkRead,
  aiEnabled,
  threadId,
  onDraftReply,
  onReply,
  onArchiveThread,
  onToggleStarThread,
  onRefresh,
  onCompose,
  onFocusSearch,
  onOpenPalette,
  onOpenShortcuts,
}: ReaderProps) {
  const [summary, setSummary] = useState<string | null>(null);
  const [summaryBusy, setSummaryBusy] = useState(false);
  const summarizeReqRef = useRef(0);

  // Reset thread summary and invalidate any in-flight stream when switching threads
  useEffect(() => {
    summarizeReqRef.current += 1;
    setSummary(null);
    setSummaryBusy(false);
  }, [threadId]);

  if (messages.length === 0) {
    return (
      <div className="pane-reader empty" role="status">
        <div className="reader-empty-card">
          <div className="reader-empty-icon">
            <Mail size={20} />
          </div>
          <h2 className="reader-empty-title">{S.noSelection}</h2>
          <p className="reader-empty-desc">
            Select a conversation from the list, or use keyboard shortcuts to
            compose and navigate.
          </p>

          {(onCompose || onFocusSearch || onOpenPalette || onOpenShortcuts) && (
            <div className="reader-empty-actions">
              {onCompose && (
                <UiButton
                  variant="primary"
                  size="sm"
                  pill
                  icon={<SquarePen size={14} />}
                  kbd="C"
                  onClick={onCompose}
                >
                  Compose
                </UiButton>
              )}
              {onFocusSearch && (
                <UiButton
                  variant="secondary"
                  size="sm"
                  pill
                  icon={<Search size={14} />}
                  kbd="/"
                  onClick={onFocusSearch}
                >
                  Search mail
                </UiButton>
              )}
              {onOpenPalette && (
                <UiButton
                  variant="secondary"
                  size="sm"
                  pill
                  icon={<SlidersHorizontal size={14} />}
                  kbd="Ctrl+K"
                  onClick={onOpenPalette}
                >
                  Command palette
                </UiButton>
              )}
              {onOpenShortcuts && (
                <UiButton
                  variant="subtle"
                  size="sm"
                  pill
                  icon={<Keyboard size={14} />}
                  kbd="?"
                  onClick={onOpenShortcuts}
                >
                  Shortcuts
                </UiButton>
              )}
            </div>
          )}

          <div className="reader-shortcut-grid" aria-label="Quick keyboard shortcuts">
            <div className="reader-shortcut-item">
              <span>Navigate threads</span>
              <span className="reader-shortcut-keys">
                <Kbd size="xs">J</Kbd>
                <Kbd size="xs">K</Kbd>
              </span>
            </div>
            <div className="reader-shortcut-item">
              <span>Archive selected</span>
              <Kbd size="xs">E</Kbd>
            </div>
            <div className="reader-shortcut-item">
              <span>Reply to thread</span>
              <Kbd size="xs">R</Kbd>
            </div>
            <div className="reader-shortcut-item">
              <span>Toggle star</span>
              <Kbd size="xs">S</Kbd>
            </div>
          </div>
        </div>
      </div>
    );
  }

  const latest = messages[messages.length - 1];
  const quickChips = [
    "Sounds good, thanks!",
    "Let me check and get back to you",
    "Can we talk this week?",
  ];

  const triggerReply = () => {
    if (onReply) {
      onReply();
    } else {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "r", bubbles: true }),
      );
    }
  };

  const triggerArchive = () => {
    if (onArchiveThread) {
      onArchiveThread();
    } else if (latest) {
      ipc
        .archiveMessage(latest.id)
        .then(() => onRefresh?.())
        .catch(() => {});
    }
  };

  const triggerStar = () => {
    if (onToggleStarThread) {
      onToggleStarThread();
    } else if (latest) {
      ipc
        .setStarred(latest.id, !latest.starred)
        .then(() => onRefresh?.())
        .catch(() => {});
    }
  };

  const runSummarize = () => {
    if (!threadId) return;
    const reqId = ++summarizeReqRef.current;
    setSummaryBusy(true);
    setSummary("");
    ipc
      .aiStream(
        "ai_summarize",
        { threadId },
        (t) => {
          if (reqId === summarizeReqRef.current) {
            setSummary((s) => (s ?? "") + t);
          }
        },
        (n) => {
          if (reqId === summarizeReqRef.current) {
            setSummary((s) => `${s ?? ""}\n[${n}]`);
          }
        },
        () => {
          if (reqId === summarizeReqRef.current) {
            setSummaryBusy(false);
          }
        },
      )
      .catch(() => {
        if (reqId === summarizeReqRef.current) {
          setSummaryBusy(false);
        }
      });
  };

  return (
    <div className="pane-reader" data-testid="reader">
      <header className="reader-toolbar">
        <div className="reader-action-group">
          <button
            type="button"
            className="reader-btn"
            title="Archive thread (e)"
            onClick={triggerArchive}
          >
            <Archive size={14} />
            <span>{S.archive}</span>
            <Kbd size="xs">e</Kbd>
          </button>
          <button
            type="button"
            className="reader-btn"
            title="Toggle read (u)"
            onClick={() => {
              if (latest) onMarkRead(latest);
            }}
          >
            <Mail size={14} />
            <span>{latest?.unread ? S.markRead : S.markUnread}</span>
            <Kbd size="xs">u</Kbd>
          </button>
          <button
            type="button"
            className={`reader-btn${latest?.starred ? " is-starred" : ""}`}
            title="Star thread (s)"
            onClick={triggerStar}
          >
            <Star
              size={14}
              fill={latest?.starred ? "currentColor" : "none"}
              style={latest?.starred ? { color: "var(--star-color)" } : undefined}
            />
            <span>{latest?.starred ? "Starred" : "Star"}</span>
            <Kbd size="xs">s</Kbd>
          </button>
          <button
            type="button"
            className="reader-btn"
            title="Reply (r)"
            onClick={triggerReply}
          >
            <Reply size={14} />
            <span>Reply</span>
            <Kbd size="xs">r</Kbd>
          </button>
        </div>

        {aiEnabled && threadId && (
          <div className="reader-action-group" data-testid="ai-bar">
            <button
              type="button"
              className="reader-btn ai-btn"
              data-testid="ai-summarize"
              disabled={summaryBusy}
              onClick={runSummarize}
            >
              <Sparkles size={14} />
              <span>{summaryBusy ? S.aiSummarizing : S.aiSummarize}</span>
            </button>
            <button
              type="button"
              className="reader-btn ai-btn"
              data-testid="ai-draft-reply"
              onClick={() => onDraftReply(threadId, "Reply to this thread")}
            >
              <Sparkles size={14} />
              <span>{S.aiDraftReply}</span>
            </button>
          </div>
        )}
      </header>

      <div className="reader-content">
        <div className="reader-subject-bar">
          <h1 className="reader-subject-title">
            {latest?.subject || "(no subject)"}
          </h1>
          <BadgePill variant="neutral" size="xs">
            {messages.length} {messages.length === 1 ? "message" : "messages"}
          </BadgePill>
        </div>

        {summary !== null && (
          <div className="ai-summary-card">
            <div className="ai-summary-head">
              <Sparkles size={14} />
              <span>AI Thread Summary</span>
              <button
                type="button"
                className="ai-summary-close"
                aria-label="Close summary"
                onClick={() => setSummary(null)}
              >
                <X size={13} />
              </button>
            </div>
            <pre className="ai-summary ai-summary-body" data-testid="ai-summary">
              {summary || "Generating summary…"}
            </pre>
          </div>
        )}

        {messages.map((m) => (
          <MessageCard
            key={m.id}
            m={m}
            threadSubject={latest?.subject ?? ""}
            onMarkRead={onMarkRead}
            onRefresh={onRefresh}
          />
        ))}

        <div className="quick-reply-box">
          {aiEnabled && threadId && (
            <div className="quick-reply-chips">
              <span className="quick-reply-chips__label">
                <Sparkles size={12} />
                <span>AI quick replies</span>
              </span>
              {quickChips.map((chip) => (
                <button
                  type="button"
                  key={chip}
                  className="quick-chip"
                  onClick={() => onDraftReply(threadId, chip)}
                >
                  <Sparkles size={11} />
                  <span>{chip}</span>
                </button>
              ))}
            </div>
          )}
          <button
            type="button"
            className="quick-reply-trigger"
            onClick={triggerReply}
          >
            <Reply size={14} />
            <span>Reply to {senderName(latest?.from[0] ?? null)}…</span>
            <Kbd size="xs" className="quick-reply-kbd">
              R
            </Kbd>
          </button>
        </div>
      </div>
    </div>
  );
}
