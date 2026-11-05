/**
 * L1: IPC contract (spec §13). Verifies the typed client maps to the exact
 * Rust command names and argument shapes src-tauri/src/app.rs defines.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ipc } from "../shared/ipc";
import type { ThreadRow } from "../shared/types";

afterEach(clearMocks);

const thread = (n: number): ThreadRow => ({
  thrid: `t${n}`,
  account_id: 1,
  account_email: "a@b.c",
  subject: `subject ${n}`,
  snippet: "snippet",
  from: { name: "Sender", email: "s@x.y" },
  last_date: Date.now(),
  msg_count: 1,
  unread_count: 1,
  starred: false,
  importance: "important",
});

describe("ipc client", () => {
  it("threadList invokes thread_list with unified scope and camelCase args", async () => {
    const handler = vi.fn(() => [thread(1)]);
    mockIPC(handler);
    const rows = await ipc.threadList(null, "important", 50, 10);
    expect(rows).toHaveLength(1);
    expect(handler).toHaveBeenCalledWith("thread_list", {
      accountId: null,
      filter: "important",
      limit: 50,
      offset: 10,
    });
  });

  it("threadList passes the account id for per-account views (FR-4)", async () => {
    const handler = vi.fn(() => []);
    mockIPC(handler);
    await ipc.threadList(7, "all");
    expect(handler).toHaveBeenCalledWith(
      "thread_list",
      expect.objectContaining({ accountId: 7 }),
    );
  });

  it("actions map to their commands with message ids", async () => {
    const handler = vi.fn(() => null);
    mockIPC(handler);
    await ipc.markRead(11, true);
    await ipc.setStarred(12, false);
    await ipc.archiveMessage(13);
    await ipc.setImportance(14, true);
    expect(handler).toHaveBeenNthCalledWith(1, "mark_read", { messageId: 11, read: true });
    expect(handler).toHaveBeenNthCalledWith(2, "set_starred", { messageId: 12, starred: false });
    expect(handler).toHaveBeenNthCalledWith(3, "archive_message", { messageId: 13 });
    expect(handler).toHaveBeenNthCalledWith(4, "set_importance", {
      messageId: 14,
      important: true,
    });
  });

  it("addAccount maps optional host fields to null, not undefined", async () => {
    const handler = vi.fn(() => ({}));
    mockIPC(handler);
    await ipc.addAccount("gmail", "a@gmail.com", "pw");
    expect(handler).toHaveBeenCalledWith("add_account", {
      providerId: "gmail",
      email: "a@gmail.com",
      password: "pw",
      imapHost: null,
      imapPort: null,
      smtpHost: null,
      smtpPort: null,
    });
  });
});
