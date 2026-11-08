/**
 * Window routing: one SPA, three window roles:
 * - `main`: the three-pane mail client (FR-29)
 * - `panel`: the notch triage panel (FR-28)
 * - `composer`: a standalone pop-out composer window for writing/replying
 */
import { useEffect, useState } from "react";
import { Composer, type ComposerTarget } from "./main/Composer";
import { MainApp } from "./main/MainApp";
import { PanelApp } from "./panel/PanelApp";

type WindowKind = "main" | "panel" | "composer";

async function resolveWindowKind(): Promise<WindowKind> {
  const queryKind = new URLSearchParams(window.location.search).get("window");
  if (queryKind === "composer") return "composer";
  if (queryKind === "panel") return "panel";
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const label = getCurrentWindow().label;
    if (label === "panel") return "panel";
    if (label.startsWith("composer")) return "composer";
    return "main";
  } catch {
    return "main";
  }
}

function parseComposerState(): ComposerTarget {
  try {
    const raw = new URLSearchParams(window.location.search).get("state");
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<ComposerTarget>;
      return {
        accountId: typeof parsed.accountId === "number" ? parsed.accountId : 1,
        to: Array.isArray(parsed.to) ? parsed.to : [],
        cc: Array.isArray(parsed.cc) ? parsed.cc : [],
        subject: typeof parsed.subject === "string" ? parsed.subject : "",
        inReplyTo: parsed.inReplyTo ?? null,
        threadId: parsed.threadId ?? null,
        initialBody: typeof parsed.initialBody === "string" ? parsed.initialBody : "",
        instruction:
          typeof parsed.instruction === "string" ? parsed.instruction : undefined,
        skipAutoStream: Boolean(parsed.skipAutoStream),
      };
    }
  } catch {
    // Fallback to blank draft if query param is malformed
  }
  return { accountId: 1, to: [], subject: "" };
}

function closeCurrentWindow() {
  void import("@tauri-apps/api/window")
    .then(({ getCurrentWindow }) => getCurrentWindow().close())
    .catch(() => window.close());
}

export function App() {
  const [kind, setKind] = useState<WindowKind | null>(null);
  useEffect(() => {
    resolveWindowKind().then(setKind).catch(() => setKind("main"));
  }, []);
  if (kind === null) return null;
  if (kind === "panel") return <PanelApp />;
  if (kind === "composer") {
    return (
      <div className="standalone-composer-window" data-testid="standalone-composer-window">
        <Composer
          target={parseComposerState()}
          standaloneWindow
          onClose={closeCurrentWindow}
          onSent={closeCurrentWindow}
        />
      </div>
    );
  }
  return <MainApp />;
}
