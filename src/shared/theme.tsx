/**
 * Theming (FR-29, Phase 5): system/light/dark choice (user-selectable in
 * Settings, `appearance.theme`), resolved instantly at the CSS-variable
 * layer by FluentProvider theme swap. no re-style pass, no tearing. The
 * html background + color-scheme are driven from `data-theme` so the
 * WebView surface outside the provider never flashes (D-040).
 */
import React, { createContext, useContext, useEffect, useState } from "react";
import {
  FluentProvider,
  createLightTheme,
  createDarkTheme,
  type BrandVariants,
  type Theme,
} from "@fluentui/react-components";
import { readLocalPref, writePref, syncDbPref } from "./prefs";

export type ThemeMode = "light" | "dark";
export type ThemeChoice = ThemeMode | "system";

const THEME_LOCAL_KEY = "cracktivity.theme";
const THEME_DB_KEY = "appearance.theme";

/**
 * cracktivity brand ramp (FR-28): the accent the whole UI keys off. Hue
 * matches the video reference sampling (test/ui-redesign-notes.md §6).
 */
const BRAND: BrandVariants = {
  10: "#061630",
  20: "#0a2652",
  30: "#0d3a7e",
  40: "#0f4da8",
  50: "#145cd6",
  60: "#2b7ff7",
  70: "#4d8df6",
  80: "#6da4f8",
  90: "#96c0fa",
  100: "#bcd7fb",
  110: "#d9e8fd",
  120: "#e8f1fe",
  130: "#f2f7fe",
  140: "#f8fbff",
  150: "#fbfdff",
  160: "#fdfeff",
};

/** Dark surfaces sampled from the reference video (test/ui-redesign-notes.md):
 * nearer layers are lighter, nothing is pure black, the selected row is a
 * quiet #181818 rather than a blue wash. */
const cracktivityDark: Theme = (() => {
  const base = createDarkTheme(BRAND);
  return {
    ...base,
    colorNeutralBackground1: "#050505",
    colorNeutralBackground1Hover: "#121212",
    colorNeutralBackground1Selected: "#181818",
    colorNeutralBackground2: "#0a0a0a",
    colorNeutralBackground2Hover: "#161616",
    colorNeutralBackground2Selected: "#1b1b1b",
    colorNeutralBackground3: "#141414",
    colorNeutralBackground4: "#181818",
    colorNeutralBackground5: "#1b1b1b",
    colorNeutralBackground6: "#202020",
    colorNeutralForeground1: "#ececec",
    colorNeutralForeground2: "#a8a8a8",
    colorNeutralForeground3: "#6b6b6b",
    colorNeutralStroke1: "#1c1c1c",
    colorNeutralStroke2: "#242424",
    colorNeutralStrokeAccessible: "#a8a8a8",
    colorBrandBackground: "#2b7ff7",
    colorBrandBackgroundHover: "#4d9df8",
    colorBrandBackgroundPressed: "#1f66d0",
    colorBrandForeground1: "#5b9df8",
    colorBrandForeground2: "#7ab0fa",
  };
})();

/** Light theme keeps the same accent on near-white surfaces. */
const cracktivityLight: Theme = (() => {
  const base = createLightTheme(BRAND);
  return {
    ...base,
    colorNeutralBackground1: "#ffffff",
    colorNeutralBackground2: "#f5f5f7",
    colorNeutralBackground3: "#f2f2f4",
    colorBrandBackground: "#0a6cd6",
    colorBrandForeground1: "#0a6cd6",
  };
})();

const themeFor = (mode: ThemeMode): Theme =>
  mode === "dark" ? cracktivityDark : cracktivityLight;

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
          into every portal surface (Tooltip/Dialog, fluentui #29184). so
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
