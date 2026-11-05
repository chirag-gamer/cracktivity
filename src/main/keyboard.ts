/**
 * Keyboard-first navigation model (FR-30). One global keydown handler owned
 * by the main window; j/k move a *selection* (controlled state, not DOM
 * focus — Fluent's tabster moves focus, which is the wrong tool here).
 *
 * When an input has focus, single-letter keys are left for typing; Escape
 * and Ctrl-combos always pass through.
 */
import { useEffect, useRef } from "react";

export interface KeyboardActions {
  move: (delta: number) => void;
  open: () => void;
  archive: () => void;
  toggleUnread: () => void;
  toggleStar: () => void;
  compose: () => void;
  reply: () => void;
  focusSearch: () => void;
  toggleOverlay: () => void;
  togglePalette: () => void;
  /** 1..9 → account slot (FR-4 Ctrl+N). */
  accountSlot: (n: number) => void;
  escape: () => void;
}

function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return (
    t.tagName === "INPUT" ||
    t.tagName === "TEXTAREA" ||
    t.isContentEditable
  );
}

export function useKeyboard(actions: KeyboardActions, enabled = true): void {
  const ref = useRef(actions);
  ref.current = actions;

  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      const a = ref.current;
      const typing = isTypingTarget(e.target);
      const ctrl = e.ctrlKey || e.metaKey;

      if (e.key === "Escape") {
        a.escape();
        return;
      }
      if (ctrl && e.key.toLowerCase() === "k") {
        e.preventDefault();
        a.togglePalette();
        return;
      }
      if (ctrl && e.key >= "1" && e.key <= "9") {
        e.preventDefault();
        a.accountSlot(Number(e.key));
        return;
      }
      if (typing || ctrl) return;

      switch (e.key) {
        case "j":
        case "ArrowDown":
          e.preventDefault();
          a.move(1);
          break;
        case "k":
        case "ArrowUp":
          e.preventDefault();
          a.move(-1);
          break;
        case "Enter":
          e.preventDefault();
          a.open();
          break;
        case "e":
          e.preventDefault();
          a.archive();
          break;
        case "u":
          e.preventDefault();
          a.toggleUnread();
          break;
        case "s":
          e.preventDefault();
          a.toggleStar();
          break;
        case "c":
          e.preventDefault();
          a.compose();
          break;
        case "r":
          e.preventDefault();
          a.reply();
          break;
        case "/":
          e.preventDefault();
          a.focusSearch();
          break;
        case "?":
          e.preventDefault();
          a.toggleOverlay();
          break;
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [enabled]);
}
