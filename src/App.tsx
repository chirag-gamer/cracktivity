/**
 * Window routing: one SPA, two windows. The main window gets the three-pane
 * app (FR-29); the notch panel window gets the triage list (FR-28). Routing
 * key = the window label set in Rust (`main` / `panel`); the URL query is a
 * fallback for plain-browser dev/test contexts where the Tauri API is absent.
 */
import { useEffect, useState } from "react";
import { MainApp } from "./main/MainApp";
import { PanelApp } from "./panel/PanelApp";

async function resolveWindowKind(): Promise<"main" | "panel"> {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return getCurrentWindow().label === "panel" ? "panel" : "main";
  } catch {
    return new URLSearchParams(window.location.search).get("window") === "panel"
      ? "panel"
      : "main";
  }
}

export function App() {
  const [kind, setKind] = useState<"main" | "panel" | null>(null);
  useEffect(() => {
    resolveWindowKind().then(setKind).catch(() => setKind("main"));
  }, []);
  if (kind === null) return null;
  return kind === "panel" ? <PanelApp /> : <MainApp />;
}
