/** L1: theme choice resolution (Phase 5/D-040). provider + document surface. */
import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import React from "react";
import { ThemeProvider, useThemeChoice, resolveMode } from "../shared/theme";

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("resolveMode", () => {
  it("system follows the OS preference", () => {
    expect(resolveMode("system", true)).toBe("dark");
    expect(resolveMode("system", false)).toBe("light");
  });
  it("light/dark override the OS preference", () => {
    expect(resolveMode("dark", false)).toBe("dark");
    expect(resolveMode("light", true)).toBe("light");
  });
});

describe("ThemeProvider", () => {
  it("defaults to system when nothing is persisted", () => {
    render(React.createElement(ThemeProvider, null, "x"));
    // matchMedia is absent in jsdom → system resolves light.
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("applies the persisted choice to the document at first render", () => {
    localStorage.setItem("cracktivity.theme", "dark");
    render(React.createElement(ThemeProvider, null, "x"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  it("setChoice persists to localStorage, the DB, and the document", async () => {
    const calls: Array<[string, unknown]> = [];
    const { mockIPC } = await import("@tauri-apps/api/mocks");
    mockIPC((cmd, args) => {
      if (cmd === "set_setting")
        calls.push([
          (args as { key: string; value: unknown }).key,
          (args as { key: string; value: unknown }).value,
        ]);
      return undefined;
    });
    let setChoiceRef: ((c: "dark") => void) | null = null;
    function Consumer() {
      const { choice, setChoice } = useThemeChoice();
      setChoiceRef = setChoice;
      return React.createElement("div", { "data-testid": "choice" }, choice);
    }
    render(React.createElement(ThemeProvider, null, React.createElement(Consumer)));
    expect(document.querySelector('[data-testid="choice"]')!.textContent).toBe("system");
    act(() => setChoiceRef!("dark"));
    expect(localStorage.getItem("cracktivity.theme")).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(calls).toContainEqual(["appearance.theme", "dark"]);
  });
});
