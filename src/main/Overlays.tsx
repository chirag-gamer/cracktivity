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

export function ShortcutsOverlay({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{S.shortcutsTitle}</DialogTitle>
          <DialogContent>
            <table className="shortcut-table" data-testid="shortcuts-overlay">
              <tbody>
                {S.shortcuts.map(([k, v]) => (
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
  const filtered = useMemo(
    () => commands.filter((c) => c.label.toLowerCase().includes(q.toLowerCase())),
    [commands, q],
  );
  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface className="palette-surface">
        <DialogBody>
          <DialogTitle>{S.commands}</DialogTitle>
          <DialogContent>
            <Input
              autoFocus
              placeholder="Type a command…"
              value={q}
              onChange={(_, d) => setQ(d.value)}
              data-testid="palette-input"
            />
            <div className="palette-list" role="listbox">
              {filtered.map((c) => (
                <button
                  key={c.id}
                  className="nav-item"
                  role="option"
                  aria-selected={false}
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
