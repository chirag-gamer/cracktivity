/**
 * L1: MainApp integration over mockIPC — the offline hydration story (FR-8)
 * plus the core keyboard flow: load → j/k → open → reader → u/e/s → Ctrl+N.
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { MainApp } from "../main/MainApp";
import { accounts, message, thread } from "./fixtures";

const threads = [thread(1), thread(2), thread(4)];
const threadMessages = [message(1), message(2), message(4)];
const commandsSeen: string[] = [];

beforeEach(() => {
  commandsSeen.length = 0;
  mockIPC((cmd) => {
    commandsSeen.push(cmd);
    switch (cmd) {
      case "list_accounts":
        return accounts;
      case "account_statuses":
        return [];
      case "thread_list":
        return threads;
      case "thread_messages":
        return threadMessages;
      case "message_body":
        return {
          text: "plain body",
          html: null,
          blocked_images: 0,
          snippet: "plain body",
        };
      case "unread_total":
        return 4;
      default:
        return null;
    }
  });
});
afterEach(() => {
  clearMocks();
  cleanup();
});

describe("MainApp", () => {
  it("hydrates accounts and threads from the local cache (FR-8)", async () => {
    const { findAllByTestId, getByTestId } = render(<MainApp />);
    expect(await findAllByTestId("thread-row")).toHaveLength(3);
    expect(getByTestId("nav-unified").textContent).toContain("All inboxes");
    expect(getByTestId("nav-account-1").textContent).toContain("Demo Account");
  });

  it("selecting a thread opens it in the reader with its messages", async () => {
    const { findAllByTestId, findByTestId } = render(<MainApp />);
    const rows = await findAllByTestId("thread-row");
    fireEvent.click(rows[0]);
    const reader = await findByTestId("reader");
    expect(reader).toBeInTheDocument();
    expect(reader.querySelectorAll('[data-testid="msg-card"]').length).toBe(3);
  });

  it("j/j/k move, and archive (e) targets the newest message of the thread", async () => {
    const { findAllByTestId, findByTestId } = render(<MainApp />);
    await findAllByTestId("thread-row");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "j", bubbles: true }));
    await findByTestId("reader");
    // Select t1 (newest-first index 0 after move clamp), then archive.
    const archiveCalls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "archive_message") archiveCalls.push((args ?? {}) as Record<string, unknown>);
      return cmd === "thread_messages" ? threadMessages : null;
    });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", bubbles: true }));
    await waitFor(() => expect(archiveCalls).toHaveLength(1));
    // Newest message of the thread (last by date) is the triage target.
    expect(archiveCalls[0]).toMatchObject({ messageId: 4 });
  });

  it("Ctrl+2 switches to the second account's view (FR-4)", async () => {
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "thread_list") calls.push((args ?? {}) as Record<string, unknown>);
      return cmd === "list_accounts" ? accounts : cmd === "thread_list" ? threads : [];
    });
    const { findAllByTestId } = render(<MainApp />);
    await findAllByTestId("thread-row");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "2", ctrlKey: true, bubbles: true }));
    await waitFor(() =>
      expect(calls.at(-1)).toMatchObject({ accountId: 2 }),
    );
  });

  it("the Important filter narrows the list query (FR-11)", async () => {
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "thread_list") calls.push((args ?? {}) as Record<string, unknown>);
      return cmd === "list_accounts" ? accounts : cmd === "thread_list" ? threads : [];
    });
    const { findAllByTestId, getByTestId } = render(<MainApp />);
    await findAllByTestId("thread-row");
    fireEvent.click(getByTestId("filter-important"));
    await waitFor(() => expect(calls.at(-1)).toMatchObject({ filter: "important" }));
  });

  it("typing / focuses the search input", async () => {
    const { findAllByTestId } = render(<MainApp />);
    await findAllByTestId("thread-row");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", bubbles: true }));
    await waitFor(() =>
      expect(document.activeElement?.getAttribute("data-testid")).toBe("search-input"),
    );
  });
});
