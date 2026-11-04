/** L1: keyboard model (FR-30) — the keymap contract, headless. */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import React from "react";
import { useKeyboard, type KeyboardActions } from "../main/keyboard";

function makeActions() {
  const fns = {
    move: vi.fn(),
    open: vi.fn(),
    archive: vi.fn(),
    toggleUnread: vi.fn(),
    toggleStar: vi.fn(),
    focusSearch: vi.fn(),
    toggleOverlay: vi.fn(),
    togglePalette: vi.fn(),
    accountSlot: vi.fn(),
    escape: vi.fn(),
  };
  return { fns, actions: fns as unknown as KeyboardActions };
}

function mount(actions: KeyboardActions, enabled = true) {
  render(
    React.createElement(() => {
      useKeyboard(actions, enabled);
      return React.createElement("input", { "aria-label": "entry" });
    }),
  );
}

function press(key: string, opts?: KeyboardEventInit, target?: HTMLElement) {
  (target ?? window).dispatchEvent(
    new KeyboardEvent("keydown", { key, bubbles: true, ...opts }),
  );
}

afterEach(cleanup);

describe("useKeyboard", () => {
  it("j/k and arrows move the selection", () => {
    const { fns, actions } = makeActions();
    mount(actions);
    press("j");
    press("ArrowDown");
    press("k");
    press("ArrowUp");
    expect(fns.move).toHaveBeenCalledTimes(4);
    expect(fns.move).toHaveBeenNthCalledWith(1, 1);
    expect(fns.move).toHaveBeenNthCalledWith(2, 1);
    expect(fns.move).toHaveBeenNthCalledWith(3, -1);
    expect(fns.move).toHaveBeenNthCalledWith(4, -1);
  });

  it("single-letter actions fire (Enter/e/u/s)", () => {
    const { fns, actions } = makeActions();
    mount(actions);
    press("Enter");
    press("e");
    press("u");
    press("s");
    expect(fns.open).toHaveBeenCalledTimes(1);
    expect(fns.archive).toHaveBeenCalledTimes(1);
    expect(fns.toggleUnread).toHaveBeenCalledTimes(1);
    expect(fns.toggleStar).toHaveBeenCalledTimes(1);
  });

  it("/ focuses search and ? opens the cheat sheet", () => {
    const { fns, actions } = makeActions();
    mount(actions);
    press("/");
    press("?");
    expect(fns.focusSearch).toHaveBeenCalledTimes(1);
    expect(fns.toggleOverlay).toHaveBeenCalledTimes(1);
  });

  it("Ctrl+K opens the palette; Ctrl+1..9 switch accounts (FR-4)", () => {
    const { fns, actions } = makeActions();
    mount(actions);
    press("k", { ctrlKey: true });
    press("2", { ctrlKey: true });
    press("9", { ctrlKey: true });
    expect(fns.togglePalette).toHaveBeenCalledTimes(1);
    expect(fns.accountSlot).toHaveBeenNthCalledWith(1, 2);
    expect(fns.accountSlot).toHaveBeenNthCalledWith(2, 9);
    // Plain ctrl+k without preventDefault leaks: assert it did not double-fire.
    expect(fns.move).not.toHaveBeenCalled();
  });

  it("Escape passes through even while typing; letters do not", () => {
    const { fns, actions } = makeActions();
    mount(actions);
    const input = document.querySelector("input")!;
    input.focus();
    press("Escape", undefined, input);
    expect(fns.escape).toHaveBeenCalledTimes(1);
    press("j", undefined, input);
    expect(fns.move).not.toHaveBeenCalled();
  });

  it("can be disabled (for overlays that own their keys)", () => {
    const { fns, actions } = makeActions();
    mount(actions, false);
    press("j");
    expect(fns.move).not.toHaveBeenCalled();
  });
});
