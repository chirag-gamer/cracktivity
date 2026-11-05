/**
 * Settings (Phase 4/5): AI (master switch, tier, engine, BYO keys, model
 * manager), MCP (safety gate, one-click registrations, audit log), general
 * (follow-up days, server-side search), appearance (theme, keymap scheme,
 * panel hotkey — FR-28/FR-30 AC), and diagnostics (hosts, storage, log
 * viewer, prune — FR-32).
 */
import { useCallback, useEffect, useState } from "react";
import { Switch, Text } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { useThemeChoice, type ThemeChoice } from "../shared/theme";
import {
  KEYMAP_SCHEMES,
  readKeymapScheme,
  writeKeymapScheme,
  type KeymapScheme,
} from "./keymap";
import type { AiStatus, AuditRow, LogLine } from "../shared/types";

const ENGINES = ["local", "openai", "anthropic", "xai"] as const;
const TIERS = ["low", "mid", "high"] as const;
const MCP_CLIENTS = ["claude_desktop", "claude_code", "codex", "cursor"] as const;

export function SettingsView() {
  const [ai, setAi] = useState<AiStatus | null>(null);
  const [settings, setSettings] = useState<Record<string, unknown>>({});
  const [audit, setAudit] = useState<AuditRow[]>([]);
  const [downloadNote, setDownloadNote] = useState<string | null>(null);
  const [registerNote, setRegisterNote] = useState<string | null>(null);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [hosts, setHosts] = useState<string[] | null>(null);
  const [storage, setStorage] = useState<{ db_bytes: number; models_bytes: number } | null>(null);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [logsCleared, setLogsCleared] = useState(false);
  const [pruneNote, setPruneNote] = useState<string | null>(null);
  const [hotkey, setHotkey] = useState("");
  const [hotkeyNote, setHotkeyNote] = useState<string | null>(null);
  const [keymap, setKeymap] = useState<KeymapScheme>(() => readKeymapScheme());
  const { choice, setChoice } = useThemeChoice();

  const reload = useCallback(() => {
    ipc.aiStatus().then(setAi).catch(() => {});
    ipc.getSettings().then(setSettings).catch(() => {});
    ipc.mcpAuditTail(50).then(setAudit).catch(() => {});
    ipc.diagnosticsHosts().then(setHosts).catch(() => {});
    ipc.storageUsage().then(setStorage).catch(() => {});
  }, []);

  const reloadLogs = useCallback(() => {
    ipc.diagnosticsLogs().then(setLogs).catch(() => {});
  }, []);

  useEffect(() => {
    reload();
    reloadLogs();
    const t = setInterval(reloadLogs, 3000);
    return () => clearInterval(t);
  }, [reload, reloadLogs]);

  const set = (key: string, value: unknown) => {
    ipc.setSetting(key, value).catch(() => {});
    setSettings((s) => ({ ...s, [key]: value }));
  };

  const download = () => {
    setDownloadNote(S.aiModelDownloading);
    ipc
      .aiDownloadModel(
        (t) => setDownloadNote(`${S.aiModelDownloading} ${t}`),
        () => {
          setDownloadNote(null);
          reload();
        },
      )
      .catch((e) => setDownloadNote(String(e)));
  };

  const saveHotkey = () => {
    setHotkeyNote(null);
    ipc
      .setPanelHotkey(hotkey)
      .then(() => {
        setHotkeyNote(S.hotkeySaved);
        // Keep the placeholder in lockstep with what Rust persisted (the
        // empty input resets to the default, D-045).
        const saved = hotkey.trim() || "ctrl+shift+space";
        setSettings((s) => ({ ...s, "panel.hotkey": saved }));
        setHotkey("");
      })
      .catch((e) => setHotkeyNote(String(e)));
  };

  const bool = (key: string) => settings[key] === true;
  const str = (key: string) => (typeof settings[key] === "string" ? (settings[key] as string) : "");

  return (
    <div className="pane-list settings" data-testid="settings-view">
      <section aria-label={S.settingsAi}>
        <h3>{S.settingsAi}</h3>
        {ai && (
          <>
            <Switch
              checked={ai.enabled}
              label={S.aiMasterSwitch}
              data-testid="ai-master-switch"
              onChange={(_, d) => {
                ipc.aiSetEnabled(d.checked).then(reload).catch(() => {});
              }}
            />
            <div className="field-row">
              <label htmlFor="ai-tier">
                {S.aiTier} ({ai.tier_reason})
              </label>
              <select
                id="ai-tier"
                value={ai.tier_is_override ? ai.tier : "auto"}
                data-testid="ai-tier"
                onChange={(e) => set("ai.tier_override", e.target.value === "auto" ? null : e.target.value)}
              >
                <option value="auto">{S.aiTierAuto}</option>
                {TIERS.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
              <label htmlFor="ai-engine">{S.aiEngine}</label>
              <select
                id="ai-engine"
                value={ai.engine}
                data-testid="ai-engine"
                onChange={(e) => set("ai.engine", e.target.value)}
              >
                {ENGINES.map((e) => (
                  <option key={e} value={e}>
                    {e}
                  </option>
                ))}
              </select>
            </div>
            <div className="field-row">
              <span data-testid="ai-model-state">
                {ai.model_downloaded ? S.aiModelReady : S.aiModelDownload}
              </span>
              {!ai.model_downloaded ? (
                <button className="primary-button" onClick={download} data-testid="ai-download">
                  {downloadNote ?? S.aiModelDownload}
                </button>
              ) : (
                <button
                  className="link-button"
                  onClick={() => ipc.aiDeleteModel().then(reload).catch(() => {})}
                >
                  {S.aiModelDelete}
                </button>
              )}
              <Text size={200}>{Math.round(ai.model_disk_bytes / 1e6)} MB</Text>
            </div>
            <Text size={200} className="hint">
              {S.aiCostNote}
            </Text>
            {ENGINES.filter((e) => e !== "local").map((e) => (
              <div key={e} className="field-row">
                <span>{e}</span>
                {ai.byo_configured[e as keyof typeof ai.byo_configured] ? (
                  <Text size={200}>{S.aiKeySet}</Text>
                ) : null}
                <Input2
                  value={keys[e] ?? ""}
                  placeholder={S.aiKeyPlaceholder}
                  onChange={(v) => setKeys((k) => ({ ...k, [e]: v }))}
                />
                <button
                  className="link-button"
                  onClick={() =>
                    ipc
                      .aiSetKey(e, keys[e] || null)
                      .then(() => {
                        setKeys((k) => ({ ...k, [e]: "" }));
                        reload();
                      })
                      .catch(() => {})
                  }
                >
                  {S.aiSaveKey}
                </button>
              </div>
            ))}
          </>
        )}
      </section>

      <section aria-label={S.settingsMcp}>
        <h3>{S.settingsMcp}</h3>
        <Switch
          checked={bool("mcp.allow_send")}
          label={S.mcpAllowSend}
          data-testid="mcp-allow-send"
          onChange={(_, d) => set("mcp.allow_send", d.checked)}
        />
        <div className="field-row">
          {MCP_CLIENTS.map((c) => (
            <button
              key={c}
              className="link-button"
              data-testid={`mcp-register-${c}`}
              onClick={() =>
                ipc
                  .mcpRegister(c)
                  .then(() => setRegisterNote(S.mcpRegistered))
                  .catch((e) => setRegisterNote(String(e)))
              }
            >
              {S.mcpRegister} {c.replace("_", " ")}
            </button>
          ))}
        </div>
        {registerNote && (
          <Text size={200} data-testid="mcp-register-note">
            {registerNote}
          </Text>
        )}
        <h4>{S.mcpAudit}</h4>
        <div className="audit" data-testid="mcp-audit">
          {audit.map(([id, at, actor, action, detail]) => (
            <div key={id} className="audit-row">
              <span className="audit-time">{new Date(at).toLocaleString()}</span>
              <span className="audit-actor">{actor}</span>
              <span className="audit-action">{action}</span>
              <span className="audit-detail">{detail}</span>
            </div>
          ))}
        </div>
      </section>

      <section aria-label={S.settingsGeneral}>
        <h3>{S.settingsGeneral}</h3>
        <Switch
          checked={bool("search.also_server")}
          label={S.settingsSearch}
          data-testid="search-also-server"
          onChange={(_, d) => set("search.also_server", d.checked)}
        />
        <div className="field-row">
          <label htmlFor="followup-days">{S.followUpDays}</label>
          <select
            id="followup-days"
            value={str("followup.days") || "3"}
            data-testid="followup-days"
            onChange={(e) => ipc.setFollowupDays(Number(e.target.value)).then(reload).catch(() => {})}
          >
            {[1, 2, 3, 5, 7].map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section aria-label={S.settingsAppearance}>
        <h3>{S.settingsAppearance}</h3>
        <div className="field-row">
          <label htmlFor="theme-choice">{S.themeChoice}</label>
          <select
            id="theme-choice"
            data-testid="theme-choice"
            value={choice}
            onChange={(e) => setChoice(e.target.value as ThemeChoice)}
          >
            <option value="system">{S.themeSystem}</option>
            <option value="light">{S.themeLight}</option>
            <option value="dark">{S.themeDark}</option>
          </select>
        </div>
        <div className="field-row">
          <label htmlFor="keymap-choice">{S.keymapChoice}</label>
          <select
            id="keymap-choice"
            data-testid="keymap-choice"
            value={keymap}
            onChange={(e) => {
              const scheme = e.target.value as KeymapScheme;
              writeKeymapScheme(scheme);
              setKeymap(scheme);
            }}
          >
            {KEYMAP_SCHEMES.map((k) => (
              <option key={k.id} value={k.id}>
                {k.label}
              </option>
            ))}
          </select>
        </div>
        <div className="field-row">
          <label htmlFor="panel-hotkey">{S.panelHotkey}</label>
          <input
            id="panel-hotkey"
            data-testid="panel-hotkey"
            value={hotkey}
            placeholder={str("panel.hotkey") || S.hotkeyDefault}
            onChange={(e) => setHotkey(e.target.value)}
          />
          <button className="link-button" onClick={saveHotkey} data-testid="hotkey-save">
            {S.hotkeySave}
          </button>
        </div>
        {hotkeyNote && (
          <Text size={200} data-testid="hotkey-note">
            {hotkeyNote}
          </Text>
        )}
        <Text size={200} className="hint">
          {S.hotkeyImeNote}
        </Text>
      </section>

      <section aria-label={S.diagnostics}>
        <h3>{S.diagnostics}</h3>
        <div className="field-row" data-testid="diagnostics-hosts">
          <span>{S.diagnosticsHosts}:</span>
          {hosts && hosts.length > 0 ? (
            hosts.map((h) => (
              <span key={h} className="audit-detail">{h}</span>
            ))
          ) : (
            <Text size={200}>{S.noHosts}</Text>
          )}
        </div>
        <div className="field-row" data-testid="storage-usage">
          <span>{S.settingsStorage}:</span>
          {storage && (
            <>
              <span>{S.storageDb}: {Math.round(storage.db_bytes / 1e6)} MB</span>
              <span>{S.storageModels}: {Math.round(storage.models_bytes / 1e6)} MB</span>
            </>
          )}
        </div>
        <div className="field-row">
          <button
            className="link-button"
            data-testid="prune-cache"
            onClick={() =>
              ipc
                .pruneCache()
                .then((n) => setPruneNote(n > 0 ? S.prunedBytes(n) : S.pruneNothing))
                .catch(() => setPruneNote(S.pruneFailed))
            }
          >
            {S.pruneCache}
          </button>
          {pruneNote && (
            <Text size={200} data-testid="prune-result">{pruneNote}</Text>
          )}
        </div>
        <h4>{S.logsTitle}</h4>
        <div className="audit" role="log" aria-label={S.logsTitle} data-testid="diag-logs">
          {logs.length === 0 ? (
            <div className="audit-row">{S.logsEmpty}</div>
          ) : (
            logs.map((l, i) => (
              <div key={i} className="audit-row">
                <span className="audit-time">{new Date(l.ts).toLocaleTimeString()}</span>
                <span className="audit-actor">{l.level}</span>
                <span className="audit-detail">{l.msg}</span>
              </div>
            ))
          )}
        </div>
        <div className="field-row">
          <button
            className="link-button"
            data-testid="logs-clear"
            onClick={() => {
              ipc.clearDiagnosticsLogs().then(() => {
                setLogsCleared(true);
                reloadLogs();
              }).catch(() => {});
            }}
          >
            {S.clearLogs}
          </button>
          {logsCleared && <Text size={200}>{S.logsCleared}</Text>}
        </div>
      </section>
    </div>
  );
}

function Input2({
  value,
  placeholder,
  onChange,
}: {
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
}) {
  return (
    <input
      type="password"
      value={value}
      placeholder={placeholder}
      data-testid="byo-key-input"
      aria-label={placeholder}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
