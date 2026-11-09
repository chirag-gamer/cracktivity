/**
 * Composer (FR-17/18): new message or in-thread reply. The draft autosaves
 * (kill-safe restore), and Send queues the driver's SMTP path. a failure
 * keeps the reason visible and the draft retryable in the outbox.
 */
import { useEffect, useRef, useState } from "react";
import { Input, Text, Textarea } from "@fluentui/react-components";
import { Maximize2, Send, Sparkles, SquarePen, X } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { IconButton, Kbd } from "../shared/ui";
import type { Addr } from "../shared/types";

export interface ComposerTarget {
  accountId: number;
  /** Prefilled reply context (FR-13 follow-up / FR-22 AI draft). */
  to: Addr[];
  cc?: Addr[];
  subject: string;
  inReplyTo?: string | null;
  threadId?: string | null;
  /** AI-generated starting text streams into the body (FR-22). */
  initialBody?: string;
  /** FR-22: steering instruction for the AI draft. */
  instruction?: string;
  /** True when popped out after initial AI draft completed or manual edits exist. */
  skipAutoStream?: boolean;
}

export interface ComposerProps {
  target: ComposerTarget;
  onClose: () => void;
  onSent: () => void;
  standaloneWindow?: boolean;
}

export function Composer({
  target,
  onClose,
  onSent,
  standaloneWindow = false,
}: ComposerProps) {
  const [to, setTo] = useState(target.to.map((a) => a.email).join(", "));
  const [cc, setCc] = useState(
    target.cc ? target.cc.map((a) => a.email).join(", ") : "",
  );
  const [subject, setSubject] = useState(target.subject);
  const [body, setBody] = useState(target.initialBody ?? "");
  const [status, setStatus] = useState<"editing" | "sending" | "failed">("editing");
  const [aiStreaming, setAiStreaming] = useState(false);
  const [activeInstruction, setActiveInstruction] = useState<string | undefined>(
    target.instruction,
  );
  const [error, setError] = useState<string | null>(null);
  const draftId = useRef<number | null>(null);
  const bodyRef = useRef(body);
  bodyRef.current = body;

  const savingRef = useRef(false);
  useEffect(() => {
    if (!to.trim() && !subject.trim() && !body.trim()) return;
    const t = window.setTimeout(() => {
      if (savingRef.current) return;
      savingRef.current = true;
      saveNow()
        .catch(() => {})
        .finally(() => {
          savingRef.current = false;
        });
    }, 800);
    return () => window.clearTimeout(t);
  });

  useEffect(() => {
    if (!target.threadId) return;
    if (target.skipAutoStream) return;
    if (target.initialBody && !target.instruction) return;
    let live = true;
    setAiStreaming(true);
    ipc
      .aiStream(
        "ai_draft_reply",
        {
          threadId: target.threadId,
          instruction: target.instruction || target.subject || S.aiInstruction,
        },
        (t) => live && setBody((b) => b + t),
        (n) => live && setError(n),
        () => {
          if (live) setAiStreaming(false);
        },
      )
      .catch(() => {
        if (live) setAiStreaming(false);
      });
    return () => {
      live = false;
      setAiStreaming(false);
    };
  }, [
    target.threadId,
    target.initialBody,
    target.instruction,
    target.subject,
    target.skipAutoStream,
  ]);

  const parseAddrs = (raw: string): Addr[] =>
    raw
      .split(/[,;]/)
      .map((s) => s.trim())
      .filter(Boolean)
      .map((email) => ({ email }));

  const saveNow = async () => {
    const id = await ipc.saveDraft({
      accountId: target.accountId,
      to: parseAddrs(to),
      cc: parseAddrs(cc),
      subject,
      bodyText: bodyRef.current,
      inReplyTo: target.inReplyTo ?? null,
      draftId: draftId.current,
    });
    draftId.current = id;
  };

  const send = async () => {
    setStatus("sending");
    setError(null);
    savingRef.current = true;
    try {
      await saveNow();
      if (draftId.current == null) throw new Error("draft vanished");
      await ipc.sendDraft(draftId.current);
      onSent();
      onClose();
    } catch (e) {
      setStatus("failed");
      setError(String(e));
    } finally {
      savingRef.current = false;
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void send();
    }
  };

  const applyTone = (tone: string) => {
    // A tone re-streams the whole draft; a second stream while one is live
    // would interleave both into the body and let the first completion
    // re-enable pop-out before the second finishes.
    if (aiStreaming) return;
    if (target.threadId) {
      const nextInstruction = `Make the reply ${tone.toLowerCase()}`;
      setActiveInstruction(nextInstruction);
      setBody("");
      setAiStreaming(true);
      ipc
        .aiStream(
          "ai_draft_reply",
          {
            threadId: target.threadId,
            instruction: nextInstruction,
          },
          (t) => setBody((b) => b + t),
          (n) => setError(n),
          () => setAiStreaming(false),
        )
        .catch(() => setAiStreaming(false));
    }
  };

  const handlePopOut = async () => {
    if (aiStreaming) return;
    const hasBody = bodyRef.current.length > 0;
    const payload: ComposerTarget = {
      accountId: target.accountId,
      to: parseAddrs(to),
      cc: parseAddrs(cc),
      subject,
      inReplyTo: target.inReplyTo ?? null,
      threadId: target.threadId ?? null,
      initialBody: bodyRef.current,
      instruction: activeInstruction,
      skipAutoStream: hasBody,
    };
    try {
      await ipc.openComposerWindow(JSON.stringify(payload));
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div
      className={`composer${standaloneWindow ? " composer--standalone" : ""}`}
      data-testid="composer"
      onKeyDown={onKeyDown}
    >
      <div className="composer-head">
        <div className="composer-head__title">
          <span className="composer-head__icon">
            <SquarePen size={14} />
          </span>
          <span>{target.inReplyTo || target.threadId ? S.replyTitle : S.composeTitle}</span>
          <span className="composer-autosave-pill">Autosaved locally</span>
        </div>
        <div className="composer-head__actions">
          {!standaloneWindow && (
            <IconButton
              label="Pop out into separate window"
              size="xs"
              disabled={aiStreaming || status === "sending"}
              data-testid="composer-popout"
              onClick={() => void handlePopOut()}
            >
              <Maximize2 size={13} />
            </IconButton>
          )}
          <IconButton label="Close composer" size="xs" onClick={onClose}>
            <X size={14} />
          </IconButton>
        </div>
      </div>

      <div className="composer-fields">
        <label className="field">
          <span>{S.to}</span>
          <Input
            value={to}
            placeholder="recipient@example.com"
            onChange={(_, d) => setTo(d.value)}
            data-testid="composer-to"
            autoFocus
          />
        </label>

        <label className="field">
          <span>{S.cc}</span>
          <Input
            value={cc}
            placeholder="cc@example.com (optional)"
            onChange={(_, d) => setCc(d.value)}
          />
        </label>

        <label className="field">
          <span>{S.subject}</span>
          <Input
            value={subject}
            placeholder="Subject"
            onChange={(_, d) => setSubject(d.value)}
            data-testid="composer-subject"
          />
        </label>
      </div>

      {target.threadId && (
        <div className="composer-ai-chips">
          <span className="composer-ai-chips__label">
            <Sparkles size={12} />
            <span>AI Tone</span>
          </span>
          {["Professional", "Shorter", "Friendly", "Detailed"].map((tone) => (
            <button
              key={tone}
              type="button"
              className="quick-chip"
              disabled={aiStreaming}
              onClick={() => applyTone(tone)}
            >
              <Sparkles size={11} />
              <span>{tone}</span>
            </button>
          ))}
        </div>
      )}

      <Textarea
        value={body}
        onChange={(_, d) => setBody(d.value)}
        rows={11}
        placeholder="Write your message… (Ctrl+Enter to send)"
        data-testid="composer-body"
        aria-label={S.composeTitle}
        className="composer-textarea"
      />

      <div className="composer-actions">
        <div className="composer-actions__left">
          <button
            type="button"
            className="primary-button composer-send-btn"
            data-testid="composer-send"
            disabled={status === "sending" || !to.trim()}
            onClick={() => void send()}
          >
            <Send size={14} />
            <span>{status === "sending" ? S.sending : S.send}</span>
            <Kbd size="xs">Ctrl+Enter</Kbd>
          </button>
          <button type="button" className="link-button" onClick={onClose}>
            {S.cancel}
          </button>
        </div>

        {error && (
          <Text size={200} className="error-hint" data-testid="composer-error">
            {S.sendFailed}: {error}
          </Text>
        )}
      </div>
    </div>
  );
}
