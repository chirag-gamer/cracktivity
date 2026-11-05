/**
 * Typed IPC client (L1-testable: every function goes through
 * `@tauri-apps/api/core`'s `invoke`, which vitest intercepts with mockIPC).
 * Argument names are camelCase on this side; Tauri maps them to the Rust
 * snake_case parameters.
 */
import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  AccountStatus,
  AccountSummary,
  AiStatus,
  ArchivedRef,
  AuditRow,
  DraftRow,
  FollowupRow,
  ImportanceFilter,
  MessageRow,
  OpenedBody,
  SearchRow,
  SubscriptionRow,
  ThreadRow,
} from "./types";

export const ipc = {
  listAccounts: () => invoke<AccountSummary[]>("list_accounts"),
  accountStatuses: () => invoke<AccountStatus[]>("account_statuses"),
  threadList: (
    accountId: number | null,
    filter: ImportanceFilter,
    limit = 200,
    offset = 0,
  ) =>
    invoke<ThreadRow[]>("thread_list", {
      accountId,
      filter,
      limit,
      offset,
    }),
  threadMessages: (thrid: string) =>
    invoke<MessageRow[]>("thread_messages", { thrid }),
  messageBody: (messageId: number, showImages?: boolean) =>
    invoke<OpenedBody>("message_body", {
      messageId,
      showImages: showImages ?? null,
    }),
  messageOpened: (messageId: number) =>
    invoke<void>("message_opened", { messageId }),
  setSenderImages: (messageId: number, allow: boolean) =>
    invoke<void>("set_sender_images", { messageId, allow }),
  searchMessages: (query: string, alsoServer?: boolean) =>
    invoke<SearchRow[]>("search_messages", {
      query,
      alsoServer: alsoServer ?? null,
    }),
  markRead: (messageId: number, read: boolean) =>
    invoke<void>("mark_read", { messageId, read }),
  setStarred: (messageId: number, starred: boolean) =>
    invoke<void>("set_starred", { messageId, starred }),
  archiveMessage: (messageId: number) =>
    invoke<void>("archive_message", { messageId }),
  setImportance: (messageId: number, important: boolean) =>
    invoke<void>("set_importance", { messageId, important }),
  addAccount: (
    providerId: string,
    email: string,
    password: string,
    imapHost?: string,
    imapPort?: number,
    smtpHost?: string,
    smtpPort?: number,
  ) =>
    invoke<AccountSummary>("add_account", {
      providerId,
      email,
      password,
      imapHost: imapHost ?? null,
      imapPort: imapPort ?? null,
      smtpHost: smtpHost ?? null,
      smtpPort: smtpPort ?? null,
    }),
  syncNow: () => invoke<void>("sync_now"),
  hidePanel: () => invoke<void>("hide_panel"),

  // -- Phase 4: follow-ups (FR-13)
  followupsList: (accountId: number | null) =>
    invoke<FollowupRow[]>("followups_list", { accountId: accountId ?? null }),
  setFollowupDays: (days: number) =>
    invoke<void>("set_followup_days", { days }),

  // -- Phase 4: subscription cleanup (FR-14)
  cleanupList: () => invoke<SubscriptionRow[]>("cleanup_list"),
  unsubscribeSender: (
    sender: string,
    listUnsubscribe: string,
    listUnsubPost: string | null,
  ) =>
    invoke<number>("unsubscribe_sender", {
      sender,
      listUnsubscribe,
      listUnsubPost: listUnsubPost ?? null,
    }),
  bulkArchiveSender: (sender: string) =>
    invoke<{ archived: number; undo: ArchivedRef[] }>("bulk_archive_sender", {
      sender,
    }),
  undoBulkArchive: (undo: ArchivedRef[]) =>
    invoke<void>("undo_bulk_archive", { undo }),
  openMailto: (mailto: string) => invoke<void>("open_mailto", { mailto }),

  // -- Phase 4: compose/drafts (FR-17/18)
  saveDraft: (d: {
    accountId: number;
    to: { email: string; name?: string | null }[];
    cc: { email: string; name?: string | null }[];
    subject: string;
    bodyText: string;
    inReplyTo?: string | null;
    threadId?: number | null;
    draftId?: number | null;
  }) =>
    invoke<number>("save_draft", {
      accountId: d.accountId,
      to: d.to,
      cc: d.cc,
      subject: d.subject,
      bodyText: d.bodyText,
      inReplyTo: d.inReplyTo ?? null,
      threadId: d.threadId ?? null,
      draftId: d.draftId ?? null,
    }),
  listDrafts: (onlyPending?: boolean) =>
    invoke<DraftRow[]>("list_drafts", { onlyPending: onlyPending ?? null }),
  deleteDraft: (draftId: number) => invoke<void>("delete_draft", { draftId }),
  sendDraft: (draftId: number) => invoke<void>("send_draft", { draftId }),

  // -- Phase 4: AI (FR-20..25)
  aiStatus: () => invoke<AiStatus>("ai_status"),
  aiSetEnabled: (enabled: boolean) =>
    invoke<void>("ai_set_enabled", { enabled }),
  aiDownloadModel: (onNotice: (text: string) => void, onDone: () => void) => {
    const channel = new Channel<{ type: string; text?: string }>();
    channel.onmessage = (msg) => {
      if (msg.type === "Notice" && msg.text) onNotice(msg.text);
      if (msg.type === "Done") onDone();
    };
    return invoke<void>("ai_download_model", { onEvent: channel });
  },
  aiDeleteModel: () => invoke<void>("ai_delete_model"),
  aiSetKey: (kind: string, key: string | null) =>
    invoke<void>("ai_set_key", { kind, key: key ?? null }),
  aiStream: (
    command: "ai_summarize" | "ai_draft_reply",
    args: Record<string, unknown>,
    onDelta: (text: string) => void,
    onNotice: (text: string) => void,
    onDone: () => void,
  ) => {
    const channel = new Channel<{ type: string; text?: string }>();
    channel.onmessage = (msg) => {
      if (msg.type === "Delta" && msg.text) onDelta(msg.text);
      else if (msg.type === "Notice" && msg.text) onNotice(msg.text);
      else if (msg.type === "Done") onDone();
    };
    return invoke<void>(command, { ...args, onEvent: channel });
  },

  // -- Phase 4/5: settings + MCP (FR-26/27)
  getSettings: () => invoke<Record<string, unknown>>("get_settings"),
  setSetting: (key: string, value: unknown) =>
    invoke<void>("set_setting", { key, value }),
  mcpRegister: (client: string) => invoke<string>("mcp_register", { client }),
  mcpAuditTail: (limit?: number) =>
    invoke<AuditRow[]>("mcp_audit_tail", { limit: limit ?? null }),
  diagnosticsHosts: () => invoke<string[]>("diagnostics_hosts"),
  storageUsage: () =>
    invoke<{ db_bytes: number; models_bytes: number }>("storage_usage"),
};
