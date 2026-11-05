/** `?` cheat sheet (FR-30) and the Ctrl+K command palette (v1: views + sync). */
import { useMemo, useState } from "react";
import {
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Input,
} from "@fluentui/react-components";
import { S } from "../shared/strings";
import { shortcutRows, type KeymapScheme } from "./keymap";

export function ShortcutsOverlay({
  open,
  onClose,
  scheme = "cracktivity",
}: {
  open: boolean;
  onClose: () => void;
  scheme?: KeymapScheme;
}) {
  const rows: [string, string][] = [
    ...shortcutRows(scheme),
    ["Ctrl+K", S.shortcutPalette],
    ["Ctrl+1..9", S.shortcutAccounts],
    ["Esc", S.shortcutEsc],
    ["?", S.shortcutSelf],
  ];
  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{S.shortcutsTitle}</DialogTitle>
          <DialogContent>
            <table className="shortcut-table" data-testid="shortcuts-overlay">
              <tbody>
                {rows.map(([k, v]) => (
                  <tr key={k}>
                    <td className="key">{k}</td>
                    <td>{v}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </DialogContent>
          <DialogActions>
            <button className="link-button" onClick={onClose}>
              {S.cancel}
            </button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}

export interface PaletteCommand {
  id: string;
  label: string;
  run: () => void;
}

export function CommandPalette({
  open,
  onClose,
  commands,
}: {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
}) {
  const [q, setQ] = useState("");
  const [hi, setHi] = useState(0);
  const filtered = useMemo(
    () => commands.filter((c) => c.label.toLowerCase().includes(q.toLowerCase())),
    [commands, q],
  );
  // Keyboard-first (FR-30): arrows move, Enter runs. The keys are consumed
  // here so the window-level handler never sees them.
  const onKey = (e: React.KeyboardEvent) => {
    if (filtered.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      setHi((i) => (i + 1) % filtered.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      setHi((i) => (i - 1 + filtered.length) % filtered.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      filtered[hi]?.run();
      onClose();
    }
  };
  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface className="palette-surface">
        <DialogBody>
          <DialogTitle>{S.commands}</DialogTitle>
          <DialogContent>
            <Input
              autoFocus
              placeholder={S.palettePlaceholder}
              value={q}
              onChange={(_, d) => {
                setQ(d.value);
                setHi(0);
              }}
              onKeyDown={onKey}
              data-testid="palette-input"
            />
            <div
              className="palette-list"
              role="listbox"
              aria-activedescendant={filtered[hi] ? `palette-opt-${filtered[hi].id}` : undefined}
            >
              {filtered.map((c, i) => (
                <button
                  key={c.id}
                  id={`palette-opt-${c.id}`}
                  className={`nav-item${i === hi ? " active" : ""}`}
                  role="option"
                  aria-selected={i === hi}
                  onMouseEnter={() => setHi(i)}
                  onClick={() => {
                    c.run();
                    onClose();
                  }}
                  data-testid={`palette-${c.id}`}
                >
                  <span className="nav-label">{c.label}</span>
                </button>
              ))}
            </div>
          </DialogContent>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
