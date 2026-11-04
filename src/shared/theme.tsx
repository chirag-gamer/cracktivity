import React, { createContext, useContext, useEffect, useState } from "react";
import {
  FluentProvider,
  teamsLightTheme,
  teamsDarkTheme,
  type Theme,
} from "@fluentui/react-components";

export type ThemeMode = "light" | "dark";

const themeFor = (mode: ThemeMode): Theme =>
  mode === "dark" ? teamsDarkTheme : teamsLightTheme;

/** Reads the OS (or WebView) preferred color scheme. */
export function useSystemThemeMode(): ThemeMode {
  const [mode, setMode] = useState<ThemeMode>("light");

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => setMode(mq.matches ? "dark" : "light");
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, []);

  return mode;
}

const ModeContext = createContext<ThemeMode>("light");

/** Wraps children in a FluentProvider that follows the system light/dark setting (FR-29). */
export function SystemThemeProvider({ children }: { children: React.ReactNode }) {
  const mode = useSystemThemeMode();
  return (
    <ModeContext.Provider value={mode}>
      {/* FluentProvider renders a div that does not self-fill; without the
          explicit height the three-pane grid collapses to zero (verified in
          the first e2e run: elements existed but were never displayed). */}
      <FluentProvider theme={themeFor(mode)} style={{ height: "100%" }}>
        {children}
      </FluentProvider>
    </ModeContext.Provider>
  );
}

export function useThemeMode(): ThemeMode {
  return useContext(ModeContext);
}
