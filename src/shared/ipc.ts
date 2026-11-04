/**
 * Typed IPC client (L1-testable: every function goes through
 * `@tauri-apps/api/core`'s `invoke`, which vitest intercepts with mockIPC).
 * Argument names are camelCase on this side; Tauri maps them to the Rust
 * snake_case parameters.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  AccountStatus,
  AccountSummary,
  ImportanceFilter,
  MessageRow,
  OpenedBody,
  SearchRow,
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
  messageBody: (messageId: number) =>
    invoke<OpenedBody>("message_body", { messageId }),
  searchMessages: (query: string) =>
    invoke<SearchRow[]>("search_messages", { query }),
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
  ) =>
    invoke<AccountSummary>("add_account", {
      providerId,
      email,
      password,
      imapHost: imapHost ?? null,
      imapPort: imapPort ?? null,
    }),
  syncNow: () => invoke<void>("sync_now"),
  hidePanel: () => invoke<void>("hide_panel"),
};
