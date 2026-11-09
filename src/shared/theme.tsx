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
import { LucideProvider } from "lucide-react";
import { readLocalPref, writePref, syncDbPref } from "./prefs";

export type ThemeMode = "light" | "dark";
export type ThemeChoice = ThemeMode | "system";

const THEME_LOCAL_KEY = "cracktivity.theme";
const THEME_DB_KEY = "appearance.theme";

/**
 * cracktivity brand ramp (FR-28, D-049): WCAG AA-calibrated blue accent scale.
 * Shade 50 (#2563eb) achieves 5.17:1 contrast with #ffffff; shade 40 (#1d4ed8)
 * achieves 6.70:1 contrast with #ffffff for light-mode primary fills.
 */
const BRAND: BrandVariants = {
  10: "#061226",
  20: "#0b2144",
  30: "#123469",
  40: "#1d4ed8",
  50: "#2563eb",
  60: "#3b82f6",
  70: "#60a5fa",
  80: "#7bb6fc",
  90: "#9ec9fd",
  100: "#bfdafe",
  110: "#d8e9fe",
  120: "#e8f2ff",
  130: "#f2f7ff",
  140: "#f8fbff",
  150: "#fbfdff",
  160: "#ffffff",
};

/**
 * Dark surfaces calibrated for distinct elevation separation and WCAG AA
 * contrast (≥4.5:1 for all text tokens, ≥3:1 for interactive borders/focus).
 */
const cracktivityDark: Theme = (() => {
  const base = createDarkTheme(BRAND);
  return {
    ...base,
    colorNeutralBackground1: "#090a0d",
    colorNeutralBackground1Hover: "#151820",
    colorNeutralBackground1Selected: "#1c202a",
    colorNeutralBackground2: "#0d0f14",
    colorNeutralBackground2Hover: "#181b24",
    colorNeutralBackground2Selected: "#1f2430",
    colorNeutralBackground3: "#14171f",
    colorNeutralBackground4: "#1a1e28",
    colorNeutralBackground5: "#202531",
    colorNeutralBackground6: "#262c3a",
    colorNeutralForeground1: "#f3f4f6",
    colorNeutralForeground2: "#b8beca",
    colorNeutralForeground3: "#8b93a2",
    colorNeutralStroke1: "#1f232d",
    colorNeutralStroke2: "#2a2f3d",
    colorNeutralStrokeAccessible: "#9ca3af",
    colorBrandBackground: "#2563eb",
    colorBrandBackgroundHover: "#1d4ed8",
    colorBrandBackgroundPressed: "#1e40af",
    colorBrandForeground1: "#60a5fa",
    colorBrandForeground2: "#93c5fd",
  };
})();

/** Light theme with distinct surface elevation and WCAG AA muted foregrounds. */
const cracktivityLight: Theme = (() => {
  const base = createLightTheme(BRAND);
  return {
    ...base,
    colorNeutralBackground1: "#ffffff",
    colorNeutralBackground1Hover: "#f3f5f9",
    colorNeutralBackground1Selected: "#e9eef8",
    colorNeutralBackground2: "#f5f6f9",
    colorNeutralBackground2Hover: "#ebedf3",
    colorNeutralBackground2Selected: "#e2e6f0",
    colorNeutralBackground3: "#eef0f5",
    colorNeutralForeground1: "#111827",
    colorNeutralForeground2: "#374151",
    colorNeutralForeground3: "#5b6472",
    colorNeutralStroke1: "#e2e6ee",
    colorNeutralStroke2: "#cfd5e2",
    colorNeutralStrokeAccessible: "#4b5563",
    colorBrandBackground: "#1d4ed8",
    colorBrandBackgroundHover: "#1e40af",
    colorBrandBackgroundPressed: "#1e3a8a",
    colorBrandForeground1: "#1d4ed8",
    colorBrandForeground2: "#1e40af",
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
        <FluentProvider theme={themeFor(mode)}>
          <LucideProvider size={15} strokeWidth={1.75}>
            {children}
          </LucideProvider>
        </FluentProvider>
      </div>
    </ChoiceContext.Provider>
  );
}

export function useThemeChoice(): ThemeChoiceApi {
  return useContext(ChoiceContext);
}
