/** L1: notch panel triage UI (FR-28). queue render + e/Esc quick actions. */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { PanelApp } from "../panel/PanelApp";
import { accounts, message, thread } from "./fixtures";

const commands: string[] = [];

beforeEach(() => {
  commands.length = 0;
  mockIPC((cmd, args) => {
    commands.push(cmd);
    switch (cmd) {
      case "list_accounts":
        return accounts;
      case "thread_list":
        return [thread(4), thread(6)];
      case "thread_messages": {
        // Answer with that thread's own unread message, like the backend.
        const n = Number(String((args as { thrid: string }).thrid).slice(1));
        return [message(n, { unread: true })];
      }
      default:
        return null;
    }
  });
});
afterEach(() => {
  clearMocks();
  cleanup();
});

describe("PanelApp", () => {
  it("lists the newest unread across accounts", async () => {
    const { findAllByTestId } = render(<PanelApp />);
    expect(await findAllByTestId("panel-row")).toHaveLength(2);
  });

  it("e archives the current row and Esc hides the panel", async () => {
    const { findAllByTestId } = render(<PanelApp />);
    await findAllByTestId("panel-row");

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", bubbles: true }));
    await waitFor(() => expect(commands).toContain("archive_message"));

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await waitFor(() => expect(commands).toContain("hide_panel"));
  });
});
