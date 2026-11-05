/**
 * Theming (FR-29, Phase 5): system/light/dark choice (user-selectable in
 * Settings, `appearance.theme`), resolved instantly at the CSS-variable
 * layer by FluentProvider theme swap — no re-style pass, no tearing. The
 * html background + color-scheme are driven from `data-theme` so the
 * WebView surface outside the provider never flashes (D-040).
 */
import React, { createContext, useContext, useEffect, useState } from "react";
import {
  FluentProvider,
  teamsLightTheme,
  teamsDarkTheme,
  type Theme,
} from "@fluentui/react-components";
import { readLocalPref, writePref, syncDbPref } from "./prefs";

export type ThemeMode = "light" | "dark";
export type ThemeChoice = ThemeMode | "system";

const THEME_LOCAL_KEY = "cracktivity.theme";
const THEME_DB_KEY = "appearance.theme";

const themeFor = (mode: ThemeMode): Theme =>
  mode === "dark" ? teamsDarkTheme : teamsLightTheme;

export const resolveMode = (choice: ThemeChoice, systemDark: boolean): ThemeMode =>
  choice === "system" ? (systemDark ? "dark" : "light") : choice;

const initialChoice = (): ThemeChoice => {
  const v = readLocalPref(THEME_LOCAL_KEY);
  return v === "light" || v === "dark" || v === "system" ? v : "system";
};

const initialSystemDark = (): boolean =>
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-color-scheme: dark)").matches;

/** The system (or WebView) preferred scheme. `prefers-color-scheme` does
    not reliably update live on Windows/WebView2, so Tauri's theme-changed
    event is listened to as well (D-040). */
function useSystemMode(): ThemeMode {
  const [systemDark, setSystemDark] = useState(initialSystemDark);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => setSystemDark(mq.matches);
    apply();
    mq.addEventListener("change", apply);
    // The Tauri subscription is async; a `disposed` flag keeps a late
    // resolution from leaking the listener (same pattern as PanelApp).
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) =>
        getCurrentWindow()
          .onThemeChanged(apply)
          .then((off) => {
            if (disposed) {
              void Promise.resolve(off()).catch(() => {});
            } else {
              unlisten = off;
            }
          })
          .catch(() => {}),
      )
      .catch(() => {});
    return () => {
      disposed = true;
      mq.removeEventListener("change", apply);
      unlisten?.();
    };
  }, []);

  return systemDark ? "dark" : "light";
}

interface ThemeChoiceApi {
  choice: ThemeChoice;
  mode: ThemeMode;
  setChoice: (c: ThemeChoice) => void;
}

const ChoiceContext = createContext<ThemeChoiceApi>({
  choice: "system",
  mode: "light",
  setChoice: () => {},
});

/** Wraps children in a FluentProvider following the user's theme choice. */
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [choice, setChoiceState] = useState<ThemeChoice>(initialChoice);
  const system = useSystemMode();
  const mode = resolveMode(choice, system === "dark");

  // The DB row is canonical; adopt it on mount (both windows converge).
  useEffect(() => {
    syncDbPref(THEME_DB_KEY, THEME_LOCAL_KEY, (v) => {
      if (v === "light" || v === "dark" || v === "system") setChoiceState(v);
    });
  }, []);

  // Keep the document surface in lockstep with the provider theme.
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", mode);
    document.documentElement.style.colorScheme = mode;
  }, [mode]);

  const setChoice = (c: ThemeChoice) => {
    setChoiceState(c);
    writePref(THEME_DB_KEY, THEME_LOCAL_KEY, c);
  };

  return (
    <ChoiceContext.Provider value={{ choice, mode, setChoice }}>
      {/* FluentProvider does not self-fill, but its style prop is copied
          into every portal surface (Tooltip/Dialog, fluentui #29184) — so
          the height lives on this wrapper, not on the provider. */}
      <div className="provider-root">
        <FluentProvider theme={themeFor(mode)}>{children}</FluentProvider>
      </div>
    </ChoiceContext.Provider>
  );
}

export function useThemeChoice(): ThemeChoiceApi {
  return useContext(ChoiceContext);
}
