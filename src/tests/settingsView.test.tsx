/** L1: Settings surface (Phase 5) — appearance, keymap, hotkey, diagnostics. */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import React from "react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { SettingsView } from "../main/SettingsView";
import { ThemeProvider } from "../shared/theme";
import { S } from "../shared/strings";
import { onKeymapChange } from "../main/keymap";

afterEach(() => {
  clearMocks();
  cleanup();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

function setup(settings: Record<string, unknown>) {
  const calls: Array<[string, Record<string, unknown> | undefined]> = [];
  mockIPC((cmd, args) => {
    calls.push([cmd, args as Record<string, unknown>]);
    switch (cmd) {
      case "ai_status":
        return {
          enabled: true,
          engine: "local",
          tier: "mid",
          tier_reason: "8 GB RAM, 4 cores → mid tier",
          tier_is_override: false,
          model_downloaded: true,
          sidecar_running: false,
          byo_configured: { openai: false, anthropic: false, xai: false },
          model_disk_bytes: 0,
        };
      case "get_settings":
        return settings;
      case "mcp_audit_tail":
        return [];
      case "diagnostics_hosts":
        return ["imap.gmail.com"];
      case "storage_usage":
        return { db_bytes: 1024, models_bytes: 2048 };
      case "diagnostics_logs":
        return [{ ts: Date.now(), level: "error", msg: "boom" }];
      case "prune_cache":
        return 1_500_000;
      default:
        return null;
    }
  });
  return calls;
}

function renderSettings() {
  return render(
    React.createElement(ThemeProvider, null, React.createElement(SettingsView)),
  );
}

describe("SettingsView appearance (FR-29/FR-30 AC)", () => {
  it("renders the appearance section with theme + keymap controls", () => {
    setup({});
    const { getByLabelText, getByTestId } = renderSettings();
    expect(getByLabelText(S.themeChoice)).toBeTruthy();
    expect(getByTestId("theme-choice")).toBeTruthy();
    expect(getByTestId("keymap-choice")).toBeTruthy();
  });

  it("changing the theme writes appearance.theme to the DB and localStorage", async () => {
    const calls = setup({});
    const { getByTestId } = renderSettings();
    fireEvent.change(getByTestId("theme-choice"), { target: { value: "dark" } });
    await waitFor(() =>
      expect(localStorage.getItem("cracktivity.theme")).toBe("dark"),
    );
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    await waitFor(() =>
      expect(calls.some(([cmd]) => cmd === "set_setting")).toBe(true),
    );
    const db = calls.find(([cmd]) => cmd === "set_setting");
    expect(db?.[1]).toMatchObject({ key: "appearance.theme", value: "dark" });
  });

  it("changing the keymap persists the scheme and notifies listeners (FR-30 AC/D-046)", async () => {
    const calls = setup({});
    const seen: string[] = [];
    const off = onKeymapChange((s) => seen.push(s));
    const { getByTestId } = renderSettings();
    fireEvent.change(getByTestId("keymap-choice"), { target: { value: "gmail" } });
    await waitFor(() =>
      expect(localStorage.getItem("cracktivity.keymap")).toBe("gmail"),
    );
    expect(seen).toEqual(["gmail"]);
    off();
    await waitFor(() =>
      expect(calls.some(([cmd]) => cmd === "set_setting")).toBe(true),
    );
  });
});

describe("SettingsView panel hotkey (FR-28)", () => {
  it("saves a hotkey through set_panel_hotkey and shows the IME note", async () => {
    const calls = setup({});
    const { getByTestId, getByText } = renderSettings();
    expect(getByText(S.hotkeyImeNote)).toBeTruthy();
    const input = getByTestId("panel-hotkey") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Ctrl+Alt+M" } });
    fireEvent.click(getByTestId("hotkey-save"));
    await waitFor(() => expect(getByTestId("hotkey-note").textContent).toBe(S.hotkeySaved));
    const call = calls.find(([cmd]) => cmd === "set_panel_hotkey");
    expect(call?.[1]).toMatchObject({ hotkey: "Ctrl+Alt+M" });
  });
});

describe("SettingsView diagnostics (FR-32)", () => {
  it("renders hosts, storage, and the log viewer", async () => {
    setup({});
    const { findByTestId, getByTestId } = renderSettings();
    expect((await findByTestId("diagnostics-hosts")).textContent).toContain("imap.gmail.com");
    const log = getByTestId("diag-logs");
    expect(log.getAttribute("role")).toBe("log");
    expect(log.getAttribute("aria-label")).toBe(S.logsTitle);
    await waitFor(() => expect(log.textContent).toContain("boom"));
  });

  it("clear clears the log buffer and prune reports bytes freed", async () => {
    const calls = setup({});
    const { findByTestId } = renderSettings();
    fireEvent.click(await findByTestId("logs-clear"));
    await waitFor(() =>
      expect(calls.some(([cmd]) => cmd === "diagnostics_clear_logs")).toBe(true),
    );
    fireEvent.click(await findByTestId("prune-cache"));
    const result = await findByTestId("prune-result");
    expect(result.textContent).toBe(S.prunedBytes(1_500_000));
  });
});
