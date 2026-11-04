/** Shared L1 fixtures mirroring the `--e2e-seed` shapes. */
import type { AccountSummary, MessageRow, ThreadRow } from "../shared/types";

export const accounts: AccountSummary[] = [
  {
    id: 1,
    provider: "gmail",
    email: "demo@example.com",
    display_name: "Demo Account",
    sync_state: "synced",
    enabled: true,
    unread: 4,
  },
  {
    id: 2,
    provider: "icloud",
    email: "me@icloud.com",
    display_name: null,
    sync_state: "idle",
    enabled: true,
    unread: 0,
  },
];

export const thread = (n: number, over: Partial<ThreadRow> = {}): ThreadRow => ({
  thrid: `t${n}`,
  account_id: 1,
  account_email: "demo@example.com",
  subject: `Thread ${n}`,
  snippet: `snippet ${n}`,
  from: { name: `Sender ${n}`, email: `s${n}@x.y` },
  last_date: 1_700_000_000_000 + n * 3_600_000,
  msg_count: n % 3 === 0 ? 3 : 1,
  unread_count: n % 2 === 0 ? 1 : 0,
  starred: n === 5,
  importance: n % 2 === 0 ? "important" : "other",
  ...over,
});

export const message = (n: number, over: Partial<MessageRow> = {}): MessageRow => ({
  id: n,
  account_id: 1,
  folder_id: 1,
  uid: n,
  subject: `Thread ${n}`,
  from: [{ name: `Sender ${n}`, email: `s${n}@x.y` }],
  to: [{ name: null, email: "demo@example.com" }],
  date: 1_700_000_000_000 + n * 3_600_000,
  unread: n % 2 === 0,
  starred: false,
  snippet: `snippet ${n}`,
  has_body: n === 4,
  importance: null,
  ...over,
});
