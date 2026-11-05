/**
 * Composer (FR-17/18): new message or in-thread reply. The draft autosaves
 * (kill-safe restore), and Send queues the driver's SMTP path — a failure
 * keeps the reason visible and the draft retryable in the outbox.
 */
import { useEffect, useRef, useState } from "react";
import { Input, Text, Textarea } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import type { Addr } from "../shared/types";

export interface ComposerTarget {
  accountId: number;
  /** Prefilled reply context (FR-13 follow-up / FR-22 AI draft). */
  to: Addr[];
  subject: string;
  inReplyTo?: string | null;
  threadId?: string | null;
  /** AI-generated starting text streams into the body (FR-22). */
  initialBody?: string;
  /** FR-22: steering instruction for the AI draft. */
  instruction?: string;
}

export interface ComposerProps {
  target: ComposerTarget;
  onClose: () => void;
  onSent: () => void;
}

export function Composer({ target, onClose, onSent }: ComposerProps) {
  const [to, setTo] = useState(target.to.map((a) => a.email).join(", "));
  const [cc, setCc] = useState("");
  const [subject, setSubject] = useState(target.subject);
  const [body, setBody] = useState(target.initialBody ?? "");
  const [status, setStatus] = useState<"editing" | "sending" | "failed">("editing");
  const [error, setError] = useState<string | null>(null);
  const draftId = useRef<number | null>(null);
  const bodyRef = useRef(body);
  bodyRef.current = body;

  // FR-17 kill-safe drafts: every edit autosaves (debounced). The first
  // edit creates the draft row; later edits update it. A killed app
  // therefore loses at most the last debounce window of typing. Empty
  // drafts are never persisted (a blank compose left open must not spawn
  // rows), and an in-flight save is never raced by a second one.
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
    // saveNow is a ref-stable closure over the current fields.
  });

  // FR-22: the AI draft streams into the editable body.
  useEffect(() => {
    if (!target.threadId || target.initialBody !== undefined) return;
    let live = true;
    ipc
      .aiStream(
        "ai_draft_reply",
        { threadId: target.threadId, instruction: target.instruction || target.subject || S.aiInstruction },
        (t) => live && setBody((b) => b + t),
        (n) => live && setError(n),
        () => {},
      )
      .catch(() => {});
    return () => {
      live = false;
    };

  }, []);

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

  return (
    // biome-ignore lint/a11y: composite editor surface
    <div className="composer" data-testid="composer" onKeyDown={onKeyDown}>
      <div className="composer-head">{target.inReplyTo ? S.replyTitle : S.composeTitle}</div>
      <label className="field">
        <span>{S.to}</span>
        <Input
          value={to}
          onChange={(_, d) => setTo(d.value)}
          data-testid="composer-to"
          autoFocus
        />
      </label>
      <label className="field">
        <span>{S.cc}</span>
        <Input value={cc} onChange={(_, d) => setCc(d.value)} />
      </label>
      <label className="field">
        <span>{S.subject}</span>
        <Input
          value={subject}
          onChange={(_, d) => setSubject(d.value)}
          data-testid="composer-subject"
        />
      </label>
      <Textarea
        value={body}
        onChange={(_, d) => setBody(d.value)}
        rows={12}
        data-testid="composer-body"
        aria-label={S.composeTitle}
      />
      <div className="composer-actions">
        <button
          className="primary-button"
          data-testid="composer-send"
          disabled={status === "sending" || !to.trim()}
          onClick={() => void send()}
        >
          {status === "sending" ? S.sending : S.send}
        </button>
        <button className="link-button" onClick={onClose}>
          {S.cancel}
        </button>
        {error && (
          <Text size={200} className="error-hint" data-testid="composer-error">
            {S.sendFailed}: {error}
          </Text>
        )}
      </div>
    </div>
  );
}
