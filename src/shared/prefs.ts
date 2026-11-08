/**
 * User-preference persistence pair (D-039): the settings DB row is
 * canonical; a localStorage mirror lets a window apply the choice
 * synchronously at startup (pre-paint, no flash) and keeps the panel and
 * main windows converged.
 */
import { invoke } from "@tauri-apps/api/core";

export function readLocalPref(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Writes the mirror and the canonical DB row (DB failure is non-fatal). */
export function writePref(dbKey: string, localKey: string, value: string): void {
  try {
    localStorage.setItem(localKey, value);
  } catch {
    /* storage unavailable. DB row still holds the truth */
  }
  void invoke("set_setting", { key: dbKey, value }).catch(() => {});
}

/** Adopts the canonical DB value once it differs from the mirror. */
export function syncDbPref(
  dbKey: string,
  localKey: string,
  apply: (v: string) => void,
): void {
  invoke<Record<string, unknown>>("get_settings")
    .then((s) => {
      const v = s[dbKey];
      if (typeof v === "string" && v !== readLocalPref(localKey)) {
        try {
          localStorage.setItem(localKey, v);
        } catch {
          /* ignore */
        }
        apply(v);
      }
    })
    .catch(() => {});
}
