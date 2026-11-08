/** `?` cheat sheet (FR-30) and the Ctrl+K command palette + unified search. */
import { useEffect, useMemo, useRef, useState } from "react";
import {
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Input,
} from "@fluentui/react-components";
import {
  Clock3,
  CornerDownLeft,
  Inbox,
  Keyboard,
  Mail,
  PenSquare,
  Plus,
  RefreshCw,
  Search,
  Settings,
  SlidersHorizontal,
  Sparkles,
  X,
} from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import type { SearchRow } from "../shared/types";
import { IconButton, Kbd } from "../shared/ui";
import { KEYMAP_SCHEMES, shortcutRows, type KeymapScheme } from "./keymap";

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
  const schemeLabel =
    KEYMAP_SCHEMES.find((k) => k.id === scheme)?.label ?? scheme;
  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface className="shortcuts-surface">
        <DialogBody>
          <DialogTitle>
            <span className="dialog-title-row">
              <span className="dialog-title-icon">
                <Keyboard size={15} />
              </span>
              <span>
                {S.shortcutsTitle} · {schemeLabel}
              </span>
              <span style={{ marginLeft: "auto" }}>
                <IconButton label={S.cancel} size="xs" onClick={onClose}>
                  <X size={14} />
                </IconButton>
              </span>
            </span>
          </DialogTitle>
          <DialogContent>
            <table
              className="shortcut-table"
              data-testid="shortcuts-overlay"
            >
              <tbody>
                {rows.map(([k, v]) => (
                  <tr key={k}>
                    <td className="key">
                      <Kbd>{k}</Kbd>
                    </td>
                    <td className="shortcut-desc">{v}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </DialogContent>
          <DialogActions>
            <button type="button" className="link-button" onClick={onClose}>
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
  kbd?: string;
  category?: string;
  run: () => void;
}

function commandIconFor(id: string) {
  if (id.includes("unified") || id.includes("account")) return <Inbox size={14} />;
  if (id.includes("followup")) return <Clock3 size={14} />;
  if (id.includes("cleanup")) return <Sparkles size={14} />;
  if (id.includes("settings")) return <Settings size={14} />;
  if (id.includes("sync")) return <RefreshCw size={14} />;
  if (id.includes("compose")) return <PenSquare size={14} />;
  if (id.includes("add")) return <Plus size={14} />;
  return <SlidersHorizontal size={14} />;
}

export function CommandPalette({
  open,
  onClose,
  commands,
  onPickSearchHit,
}: {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
  onPickSearchHit?: (hit: SearchRow) => void;
}) {
  const [q, setQ] = useState("");
  const [hi, setHi] = useState(0);
  const [mailHits, setMailHits] = useState<SearchRow[]>([]);
  const reqGenRef = useRef(0);

  const filtered = useMemo(
    () => commands.filter((c) => c.label.toLowerCase().includes(q.toLowerCase())),
    [commands, q],
  );

  useEffect(() => {
    const trimmed = q.trim();
    if (!open || !onPickSearchHit || trimmed.length < 2) {
      reqGenRef.current += 1;
      setMailHits([]);
      return;
    }
    const gen = ++reqGenRef.current;
    const t = setTimeout(() => {
      void ipc
        .searchMessages(trimmed, false)
        .then((rows) => {
          if (reqGenRef.current === gen) {
            setMailHits(rows.slice(0, 5));
          }
        })
        .catch(() => {
          if (reqGenRef.current === gen) {
            setMailHits([]);
          }
        });
    }, 110);
    return () => clearTimeout(t);
  }, [q, open, onPickSearchHit]);

  const totalItems = filtered.length + mailHits.length;

  const onKey = (e: React.KeyboardEvent) => {
    if (totalItems === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      setHi((i) => (i + 1) % totalItems);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      setHi((i) => (i - 1 + totalItems) % totalItems);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (hi < filtered.length) {
        filtered[hi]?.run();
        onClose();
      } else {
        const hit = mailHits[hi - filtered.length];
        if (hit && onPickSearchHit) {
          onPickSearchHit(hit);
          onClose();
        }
      }
    }
  };

  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface className="palette-surface">
        <DialogBody>
          <DialogTitle>
            <span className="dialog-title-row">
              <span className="dialog-title-icon">
                <SlidersHorizontal size={15} />
              </span>
              <span>{S.commands}</span>
              <span className="palette-title-sub">Commands & Quick Search</span>
            </span>
          </DialogTitle>
          <DialogContent>
            <div className="palette-input-wrap">
              <Input
                autoFocus
                contentBefore={<Search size={14} />}
                placeholder={S.palettePlaceholder}
                value={q}
                onChange={(_, d) => {
                  setQ(d.value);
                  setHi(0);
                }}
                onKeyDown={onKey}
                data-testid="palette-input"
                style={{ width: "100%" }}
              />
            </div>
            <div
              className="palette-list"
              role="listbox"
              aria-activedescendant={
                filtered[hi] ? `palette-opt-${filtered[hi].id}` : undefined
              }
            >
              {filtered.length > 0 && (
                <div className="palette-group-header">Actions & Views</div>
              )}
              {filtered.map((c, i) => (
                <button
                  type="button"
                  key={c.id}
                  id={`palette-opt-${c.id}`}
                  className={`nav-item palette-item${i === hi ? " active" : ""}`}
                  role="option"
                  aria-selected={i === hi}
                  onMouseEnter={() => setHi(i)}
                  onClick={() => {
                    c.run();
                    onClose();
                  }}
                  data-testid={`palette-${c.id}`}
                >
                  <span className="palette-item-lead">
                    <span className="palette-item-icon">{commandIconFor(c.id)}</span>
                    <span className="nav-label">{c.label}</span>
                  </span>
                  <span className="palette-item-right">
                    {c.kbd && <Kbd size="xs">{c.kbd}</Kbd>}
                    {i === hi && (
                      <span className="palette-item-enter" aria-hidden="true">
                        <CornerDownLeft size={12} />
                      </span>
                    )}
                  </span>
                </button>
              ))}

              {mailHits.length > 0 && (
                <>
                  <div className="palette-group-header">Matching Messages</div>
                  {mailHits.map((hit, idx) => {
                    const itemIdx = filtered.length + idx;
                    const active = itemIdx === hi;
                    return (
                      <button
                        type="button"
                        key={`mail-${hit.message_id}`}
                        className={`nav-item palette-item palette-mail-item${active ? " active" : ""}`}
                        role="option"
                        aria-selected={active}
                        onMouseEnter={() => setHi(itemIdx)}
                        onClick={() => {
                          onPickSearchHit?.(hit);
                          onClose();
                        }}
                      >
                        <span className="palette-item-lead">
                          <span className="palette-item-icon">
                            <Mail size={14} />
                          </span>
                          <span className="palette-mail-copy">
                            <span className="palette-mail-subj">
                              {hit.subject || "(no subject)"}
                            </span>
                            <span className="palette-mail-meta">
                              {hit.from?.name || hit.from?.email || ""} · {hit.snippet}
                            </span>
                          </span>
                        </span>
                        {active && (
                          <span className="palette-item-enter" aria-hidden="true">
                            <CornerDownLeft size={12} />
                          </span>
                        )}
                      </button>
                    );
                  })}
                </>
              )}

              {totalItems === 0 && (
                <div className="palette-empty-state">
                  No matching commands or messages for “{q}”
                </div>
              )}
            </div>
            <div className="palette-footer-bar">
              <span>
                <Kbd size="xs">↑↓</Kbd> Navigate
              </span>
              <span>
                <Kbd size="xs">Enter</Kbd> Execute
              </span>
              <span>
                <Kbd size="xs">Esc</Kbd> Close
              </span>
            </div>
          </DialogContent>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
