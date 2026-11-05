/**
 * Keymap schemes (FR-30 AC: "cracktivity/Gmail-style selectable in
 * Settings"). j/k, arrows, Enter, Esc and Ctrl-combos are scheme-independent;
 * a scheme only aliases the action keys and labels the cheat sheet.
 * Persisted as `appearance.keymap` (DB) + localStorage mirror (D-039).
 */
import { readLocalPref, writePref, syncDbPref } from "../shared/prefs";

export type KeymapScheme = "cracktivity" | "gmail";
export type KeyAction =
  | "archive"
  | "toggleUnread"
  | "toggleStar"
  | "compose"
  | "reply"
  | "focusSearch";

const KEYMAP_LOCAL_KEY = "cracktivity.keymap";
const KEYMAP_DB_KEY = "appearance.keymap";

export const KEYMAP_SCHEMES: { id: KeymapScheme; label: string }[] = [
  { id: "cracktivity", label: "cracktivity" },
  { id: "gmail", label: "Gmail-style" },
];

/** key → action per scheme; `y` archives only in the Gmail-style scheme. */
export function keyActionFor(scheme: KeymapScheme, key: string): KeyAction | undefined {
  switch (key) {
    case "e":
      return "archive";
    case "y":
      return scheme === "gmail" ? "archive" : undefined;
    case "u":
      return "toggleUnread";
    case "s":
      return "toggleStar";
    case "c":
      return "compose";
    case "r":
      return "reply";
    case "/":
      return "focusSearch";
    default:
      return undefined;
  }
}

/** Cheat-sheet rows for the scheme-dependent keys, in display order. */
export function shortcutRows(scheme: KeymapScheme): [string, string][] {
  return [
    ["j / k", "Move down / up"],
    ["Enter", "Open thread"],
    [scheme === "gmail" ? "e / y" : "e", "Archive"],
    ["u", "Toggle unread"],
    ["s", "Toggle star"],
    ["/", "Search"],
    ["c", "Compose"],
    ["r", "Reply"],
  ];
}

export function isKeymapScheme(v: string | null): v is KeymapScheme {
  return v === "gmail" || v === "cracktivity";
}

export function readKeymapScheme(): KeymapScheme {
  const v = readLocalPref(KEYMAP_LOCAL_KEY);
  return isKeymapScheme(v) ? v : "cracktivity";
}

// In-window pub-sub (D-046): changing the scheme in Settings must take
// effect immediately in the running window (FR-30 AC), not after a restart.
// Cross-window propagation stays on the DB-adoption path (D-039).
type SchemeListener = (s: KeymapScheme) => void;
const listeners = new Set<SchemeListener>();

export function onKeymapChange(listener: SchemeListener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function notify(scheme: KeymapScheme): void {
  listeners.forEach((l) => l(scheme));
}

export function writeKeymapScheme(scheme: KeymapScheme): void {
  writePref(KEYMAP_DB_KEY, KEYMAP_LOCAL_KEY, scheme);
  notify(scheme);
}

/** Adopts the canonical DB scheme (converges panel and main windows). */
export function syncKeymapScheme(apply: (s: KeymapScheme) => void): void {
  syncDbPref(KEYMAP_DB_KEY, KEYMAP_LOCAL_KEY, (v) => {
    if (isKeymapScheme(v)) apply(v);
  });
}
