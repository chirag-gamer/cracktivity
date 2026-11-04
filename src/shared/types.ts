/** Shared shapes mirroring the Rust `Serialize` structs (src-tauri/src). */

export interface Addr {
  name?: string | null;
  email: string;
}

export interface AccountSummary {
  id: number;
  provider: string;
  email: string;
  display_name: string | null;
  sync_state: string;
  enabled: boolean;
  unread: number;
}

export interface AccountStatus {
  account_id: number;
  /** connecting | syncing | ok | offline | auth_failed */
  state: string;
  error: string | null;
}

export interface ThreadRow {
  thrid: string;
  account_id: number;
  account_email: string;
  subject: string;
  snippet: string;
  from: Addr | null;
  last_date: number;
  msg_count: number;
  unread_count: number;
  starred: boolean;
  importance: string | null;
}

export interface MessageRow {
  id: number;
  account_id: number;
  folder_id: number;
  uid: number;
  subject: string | null;
  from: Addr[];
  to: Addr[];
  date: number;
  unread: boolean;
  starred: boolean;
  snippet: string;
  has_body: boolean;
  importance: string | null;
}

export interface OpenedBody {
  text: string | null;
  html: string | null;
  blocked_images: number;
  snippet: string;
}

export interface SearchRow {
  message_id: number;
  thrid: string | null;
  account_id: number;
  subject: string;
  snippet: string;
  from: Addr | null;
  date: number;
  unread: boolean;
}

export type ImportanceFilter = "all" | "important" | "other";

/** The selected view: the unified inbox (FR-11) or one account (FR-4). */
export type ViewScope = { kind: "unified" } | { kind: "account"; id: number };
