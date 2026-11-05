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
  tracking_count: number;
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

/** The selected view: unified inbox, one account (FR-4), or a tool view. */
export type ViewScope =
  | { kind: "unified" }
  | { kind: "account"; id: number }
  | { kind: "followups" }
  | { kind: "cleanup" }
  | { kind: "settings" };

// ------------------------------------------------------------------ Phase 4

/** One "Waiting for Reply" row (FR-13). */
export interface FollowupRow {
  thrid: string;
  account_id: number;
  account_email: string;
  subject: string;
  snippet: string;
  to: Addr | null;
  sent_date: number;
  days_waiting: number;
}

/** One sender row of the subscription-cleanup view (FR-14). */
export interface SubscriptionRow {
  sender_email: string;
  sender_name: string | null;
  account_id: number;
  received: number;
  opened: number;
  last_date: number;
  list_unsubscribe: string | null;
  list_unsub_post: string | null;
}

/** Where an archived message went — the FR-14 Undo input. */
export interface ArchivedRef {
  account_id: number;
  from_folder: string;
  to_folder: string;
  new_uid: number;
}

export interface DraftRow {
  id: number;
  account_id: number;
  thread_id: number | null;
  to: Addr[];
  cc: Addr[];
  subject: string | null;
  body_text: string | null;
  in_reply_to: string | null;
  status: string;
  error: string | null;
  updated_at: number;
}

export interface AiStatus {
  enabled: boolean;
  engine: string;
  tier: string;
  tier_reason: string;
  tier_is_override: boolean;
  model_downloaded: boolean;
  sidecar_running: boolean;
  byo_configured: { openai: boolean; anthropic: boolean; xai: boolean };
  model_disk_bytes: number;
}

/** Audit tail row: id, at, actor, action, detail json. */
export type AuditRow = [number, number, string, string, string];
