/**
 * Settings (Phase 4/5/8): Jan-inspired AI Model Providers & Custom Endpoints
 * (Local llama.cpp + OpenAI, Anthropic, xAI, OpenRouter, Mistral, Groq,
 * Gemini, Hugging Face, ChatGPT OAuth, and unlimited custom OpenAI/Anthropic
 * endpoints), Default/Writing/Fallback model routing, live `/models`
 * discovery, diagnostic error & GitHub issue reporting, MCP permissions,
 * custom accessible dropdowns (`UiSelect`), and storage/network diagnostics.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Switch,
  Text,
} from "@fluentui/react-components";
import {
  AlertTriangle,
  Bot,
  Bug,
  Check,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  History,
  KeyRound,
  Pencil,
  Plus,
  RefreshCw,
  Settings,
  Sparkles,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { BadgePill, IconButton, UiButton, UiSelect, UiSwitch } from "../shared/ui";
import { useThemeChoice, type ThemeChoice } from "../shared/theme";
import {
  KEYMAP_SCHEMES,
  readKeymapScheme,
  writeKeymapScheme,
  type KeymapScheme,
} from "./keymap";
import type { AiStatus, AuditRow, LogLine } from "../shared/types";

const TIERS = ["low", "mid", "high"] as const;
const MCP_CLIENTS = ["claude_desktop", "claude_code", "codex", "cursor"] as const;

export interface ProviderModelItem {
  id: string;
  capabilities: ("tools" | "vision")[];
}

export interface AiProviderDefinition {
  id: string;
  name: string;
  apiFormat: "openai" | "anthropic";
  baseUrl: string;
  apiKeysUrl?: string;
  docsUrl?: string;
  docsLabel?: string;
  enabled: boolean;
  custom?: boolean;
  defaultModel?: string;
  models: ProviderModelItem[];
  accentBg: string;
  accentFg: string;
  monogram: string;
}

export function inferModelCapabilities(modelId: string): ("tools" | "vision")[] {
  const lower = modelId.toLowerCase();
  const caps: ("tools" | "vision")[] = ["tools"];
  if (
    lower.includes("claude-3") ||
    lower.includes("claude-4") ||
    lower.includes("claude-sonnet") ||
    lower.includes("claude-opus") ||
    lower.includes("claude-haiku") ||
    lower.includes("gpt-4o") ||
    lower.includes("gpt-4.1") ||
    lower.includes("gpt-4.5") ||
    lower.includes("gpt-5") ||
    lower.includes("o1") ||
    lower.includes("o3") ||
    lower.includes("o4") ||
    lower.includes("vision") ||
    lower.includes("pixtral") ||
    lower.includes("gemini-") ||
    lower.includes("grok-2-vision") ||
    lower.includes("grok-3")
  ) {
    caps.push("vision");
  }
  return caps;
}

function modelsFromIds(ids: string[]): ProviderModelItem[] {
  return ids.map((id) => ({
    id,
    capabilities: inferModelCapabilities(id),
  }));
}

export const PREDEFINED_PROVIDERS: ReadonlyArray<AiProviderDefinition> = [
  {
    id: "openai",
    name: "OpenAI",
    apiFormat: "openai",
    baseUrl: "https://api.openai.com/v1",
    apiKeysUrl: "https://platform.openai.com/api-keys",
    docsUrl: "https://platform.openai.com/docs/api-reference/chat",
    docsLabel: "OpenAI API documentation",
    enabled: true,
    defaultModel: "gpt-4o-mini",
    models: modelsFromIds([
      "gpt-4.1",
      "gpt-4.1-mini",
      "gpt-4o",
      "gpt-4o-mini",
      "o3-mini",
    ]),
    accentBg: "rgba(16, 163, 127, 0.18)",
    accentFg: "#10a37f",
    monogram: "OA",
  },
  {
    id: "anthropic",
    name: "Anthropic",
    apiFormat: "anthropic",
    baseUrl: "https://api.anthropic.com",
    apiKeysUrl: "https://console.anthropic.com/settings/keys",
    docsUrl: "https://docs.anthropic.com/en/api/getting-started",
    docsLabel: "Anthropic API documentation",
    enabled: true,
    defaultModel: "claude-sonnet-4-5",
    models: modelsFromIds([
      "claude-sonnet-4-5",
      "claude-haiku-4-5",
      "claude-opus-4-1",
      "claude-sonnet-4",
      "claude-opus-4",
      "claude-3-7-sonnet-20250219",
      "claude-3-5-haiku-20241022",
      "claude-3-haiku-20240307",
    ]),
    accentBg: "rgba(217, 119, 87, 0.20)",
    accentFg: "#d97757",
    monogram: "AI",
  },
  {
    id: "xai",
    name: "xAI (Grok)",
    apiFormat: "openai",
    baseUrl: "https://api.x.ai/v1",
    apiKeysUrl: "https://console.x.ai/",
    docsUrl: "https://docs.x.ai/docs/overview",
    docsLabel: "xAI API documentation",
    enabled: true,
    defaultModel: "grok-2-latest",
    models: modelsFromIds([
      "grok-3-beta",
      "grok-3-mini-beta",
      "grok-2-latest",
      "grok-2-vision-1212",
    ]),
    accentBg: "rgba(148, 163, 184, 0.18)",
    accentFg: "#e2e8f0",
    monogram: "X",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    apiFormat: "openai",
    baseUrl: "https://openrouter.ai/api/v1",
    apiKeysUrl: "https://openrouter.ai/settings/keys",
    docsUrl: "https://openrouter.ai/docs/quick-start",
    docsLabel: "OpenRouter API documentation",
    enabled: false,
    defaultModel: "openai/gpt-4o-mini",
    models: modelsFromIds([
      "openai/gpt-4o-mini",
      "anthropic/claude-3.7-sonnet",
      "deepseek/deepseek-r1:free",
      "qwen/qwen3-30b-a3b:free",
    ]),
    accentBg: "rgba(99, 102, 241, 0.18)",
    accentFg: "#818cf8",
    monogram: "OR",
  },
  {
    id: "mistral",
    name: "Mistral",
    apiFormat: "openai",
    baseUrl: "https://api.mistral.ai/v1",
    apiKeysUrl: "https://console.mistral.ai/api-keys/",
    docsUrl: "https://docs.mistral.ai/getting-started/models/models_overview/",
    docsLabel: "Mistral API documentation",
    enabled: false,
    defaultModel: "mistral-small-latest",
    models: modelsFromIds([
      "mistral-large-latest",
      "mistral-medium-latest",
      "mistral-small-latest",
      "codestral-latest",
      "pixtral-large-latest",
      "ministral-8b-latest",
      "ministral-3b-latest",
      "open-mistral-nemo",
    ]),
    accentBg: "rgba(249, 115, 22, 0.18)",
    accentFg: "#fb923c",
    monogram: "M",
  },
  {
    id: "groq",
    name: "Groq",
    apiFormat: "openai",
    baseUrl: "https://api.groq.com/openai/v1",
    apiKeysUrl: "https://console.groq.com/keys",
    docsUrl: "https://console.groq.com/docs/models",
    docsLabel: "Groq API documentation",
    enabled: false,
    defaultModel: "llama-3.3-70b-versatile",
    models: modelsFromIds([
      "llama-3.3-70b-versatile",
      "llama-3.1-8b-instant",
      "deepseek-r1-distill-llama-70b",
      "qwen-qwq-32b",
      "gemma2-9b-it",
      "mixtral-8x7b-32768",
      "llama-3.2-90b-vision-preview",
      "llama-3.2-11b-vision-preview",
    ]),
    accentBg: "rgba(239, 68, 68, 0.18)",
    accentFg: "#f87171",
    monogram: "G",
  },
  {
    id: "gemini",
    name: "Gemini",
    apiFormat: "openai",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    apiKeysUrl: "https://aistudio.google.com/apikey",
    docsUrl: "https://ai.google.dev/gemini-api/docs/openai",
    docsLabel: "Gemini API documentation",
    enabled: false,
    defaultModel: "gemini-2.5-flash",
    models: modelsFromIds([
      "gemini-2.5-pro",
      "gemini-2.5-flash",
      "gemini-2.0-flash",
      "gemini-2.0-flash-lite",
      "gemini-1.5-pro",
      "gemini-1.5-flash",
      "gemma-3-27b-it",
    ]),
    accentBg: "rgba(59, 130, 246, 0.18)",
    accentFg: "#60a5fa",
    monogram: "✦",
  },
  {
    id: "huggingface",
    name: "Hugging Face",
    apiFormat: "openai",
    baseUrl: "https://router.huggingface.co/v1",
    apiKeysUrl: "https://huggingface.co/settings/tokens",
    docsUrl: "https://huggingface.co/docs/api-inference/index",
    docsLabel: "Hugging Face API documentation",
    enabled: false,
    defaultModel: "deepseek-ai/DeepSeek-V3-0324",
    models: modelsFromIds([
      "moonshotai/Kimi-K2-Instruct:groq",
      "deepseek-ai/DeepSeek-R1-0528",
      "deepseek-ai/DeepSeek-V3-0324",
    ]),
    accentBg: "rgba(234, 179, 8, 0.18)",
    accentFg: "#facc15",
    monogram: "HF",
  },
];

type StoredProviderPatch = {
  name?: string;
  apiFormat?: "openai" | "anthropic";
  baseUrl?: string;
  enabled?: boolean;
  custom?: boolean;
  defaultModel?: string;
  models?: (string | ProviderModelItem)[];
};

function mergeProvidersConfig(
  rawConfig: unknown,
  byoConfigured?: Record<string, boolean>,
): AiProviderDefinition[] {
  const map: Record<string, StoredProviderPatch> =
    rawConfig && typeof rawConfig === "object" && !Array.isArray(rawConfig)
      ? (rawConfig as Record<string, StoredProviderPatch>)
      : {};

  const normalizeModels = (
    raw: (string | ProviderModelItem)[] | undefined,
    fallback: ProviderModelItem[],
  ): ProviderModelItem[] => {
    if (!Array.isArray(raw)) return fallback;
    return raw.map((m) =>
      typeof m === "string"
        ? { id: m, capabilities: inferModelCapabilities(m) }
        : {
            id: m.id,
            capabilities:
              Array.isArray(m.capabilities) && m.capabilities.length > 0
                ? m.capabilities
                : inferModelCapabilities(m.id),
          },
    );
  };

  const merged: AiProviderDefinition[] = PREDEFINED_PROVIDERS.map((preset) => {
    const patch = map[preset.id];
    const hasKey = Boolean(byoConfigured?.[preset.id]);
    const models = normalizeModels(patch?.models, preset.models);
    return {
      ...preset,
      baseUrl: patch?.baseUrl?.trim() || preset.baseUrl,
      apiFormat: patch?.apiFormat ?? preset.apiFormat,
      enabled: patch?.enabled ?? ( preset.enabled || hasKey ),
      defaultModel:
        patch?.defaultModel ||
        models[0]?.id ||
        preset.defaultModel,
      models,
    };
  });

  for (const [id, patch] of Object.entries(map)) {
    if (merged.some((p) => p.id === id)) continue;
    const name = patch.name?.trim() || id;
    const models = normalizeModels(patch.models, [
      { id: patch.defaultModel || "gpt-4o-mini", capabilities: ["tools"] },
    ]);
    merged.push({
      id,
      name,
      apiFormat: patch.apiFormat === "anthropic" ? "anthropic" : "openai",
      baseUrl: patch.baseUrl?.trim() || "https://api.openai.com/v1",
      enabled: patch.enabled ?? true,
      custom: true,
      defaultModel: patch.defaultModel || models[0]?.id || "gpt-4o-mini",
      models,
      accentBg: "rgba(139, 92, 246, 0.18)",
      accentFg: "#a78bfa",
      monogram: name.slice(0, 2).toUpperCase(),
    });
  }

  return merged;
}

function serializeProvidersConfig(
  providers: AiProviderDefinition[],
): Record<string, StoredProviderPatch> {
  const out: Record<string, StoredProviderPatch> = {};
  for (const p of providers) {
    out[p.id] = {
      name: p.name,
      apiFormat: p.apiFormat,
      baseUrl: p.baseUrl,
      enabled: p.enabled,
      custom: Boolean(p.custom),
      defaultModel: p.defaultModel || p.models[0]?.id,
      models: p.models,
    };
  }
  return out;
}

export function SettingsView() {
  const [chatgptBusy, setChatgptBusy] = useState(false);
  const [chatgptError, setChatgptError] = useState<string | null>(null);
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

  // Jan-style Provider Settings Drawer/Modal & Custom Provider Dialog states
  const [activeProviderId, setActiveProviderId] = useState<string | null>(null);
  const [keyRevealed, setKeyRevealed] = useState(false);
  const [keyCopied, setKeyCopied] = useState(false);
  const [keySavedNote, setKeySavedNote] = useState<string | null>(null);
  const [refreshingModels, setRefreshingModels] = useState(false);
  const [providerStatusNote, setProviderStatusNote] = useState<string | null>(null);
  const [aiDiagnosticError, setAiDiagnosticError] = useState<{
    provider: string;
    message: string;
    endpoint?: string;
  } | null>(null);
  const [diagCopied, setDiagCopied] = useState(false);

  // Add/Edit Model inside Provider Settings Modal
  const [showAddModelBar, setShowAddModelBar] = useState(false);
  const [newModelIdInput, setNewModelIdInput] = useState("");
  const [editingModelId, setEditingModelId] = useState<string | null>(null);
  const [editingModelDraft, setEditingModelDraft] = useState("");

  // Add Custom Provider Dialog (`+ Add Provider`)
  const [addProviderOpen, setAddProviderOpen] = useState(false);
  const [customName, setCustomName] = useState("");
  const [customFormat, setCustomFormat] = useState<"openai" | "anthropic">("openai");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customApiKey, setCustomApiKey] = useState("");
  const [customInitialModel, setCustomInitialModel] = useState("");
  const [customError, setCustomError] = useState<string | null>(null);

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
    const done = ipc.setSetting(key, value).catch(() => {});
    setSettings((s) => ({ ...s, [key]: value }));
    return done;
  };

  const bool = (key: string) => settings[key] === true;
  const str = (key: string) =>
    typeof settings[key] === "string" ? (settings[key] as string) : "";

  const sanitizeEndpointUrl = (raw?: string): string | undefined => {
    if (!raw) return undefined;
    try {
      const parsed = new URL(raw.trim());
      parsed.username = "";
      parsed.password = "";
      parsed.search = "";
      parsed.hash = "";
      return parsed.toString();
    } catch {
      return "<redacted-url>";
    }
  };

  const providers = useMemo(
    () => mergeProvidersConfig(settings["ai.providers_config"], ai?.byo_configured),
    [settings, ai?.byo_configured],
  );

  const reconcileAiRoutes = (
    nextProviders: AiProviderDefinition[],
    modelChange?: { providerId: string; oldModelId: string; newModelId?: string },
  ) => {
    const currentEngine = str("ai.engine") || ai?.engine || "local";
    const currentModel = str("ai.byo_model");

    if (currentEngine !== "local" && currentEngine !== "chatgpt") {
      const prov = nextProviders.find((p) => p.id === currentEngine && p.enabled);
      if (!prov || prov.models.length === 0) {
        set("ai.engine", "local");
        set("ai.byo_model", "");
        setAi((prev) => (prev ? { ...prev, engine: "local" } : prev));
      } else if (
        modelChange &&
        modelChange.providerId === currentEngine &&
        (currentModel === modelChange.oldModelId || !currentModel)
      ) {
        const nextM =
          modelChange.newModelId || prov.defaultModel || prov.models[0]?.id || "";
        set("ai.byo_model", nextM);
      } else if (currentModel && !prov.models.some((m) => m.id === currentModel)) {
        set("ai.byo_model", prov.defaultModel || prov.models[0]?.id || "");
      }
    }

    const writingEng = str("ai.writing_engine") || "same";
    const writingMod = str("ai.writing_model");
    if (writingEng !== "same" && writingEng !== "local" && writingEng !== "chatgpt") {
      const wProv = nextProviders.find((p) => p.id === writingEng && p.enabled);
      if (!wProv || wProv.models.length === 0) {
        set("ai.writing_engine", "same");
        set("ai.writing_model", "");
      } else if (
        modelChange &&
        modelChange.providerId === writingEng &&
        (writingMod === modelChange.oldModelId || !writingMod)
      ) {
        const nextM =
          modelChange.newModelId || wProv.defaultModel || wProv.models[0]?.id || "";
        if (nextM) {
          set("ai.writing_model", nextM);
        } else {
          set("ai.writing_engine", "same");
          set("ai.writing_model", "");
        }
      } else if (writingMod && !wProv.models.some((m) => m.id === writingMod)) {
        const nextM = wProv.defaultModel || wProv.models[0]?.id || "";
        if (nextM) {
          set("ai.writing_model", nextM);
        } else {
          set("ai.writing_engine", "same");
          set("ai.writing_model", "");
        }
      }
    }

    const fbEng = str("ai.fallback_engine") || "local";
    if (fbEng !== "local" && fbEng !== "none" && fbEng !== "chatgpt") {
      const fbProv = nextProviders.find((p) => p.id === fbEng && p.enabled);
      if (!fbProv || fbProv.models.length === 0) {
        set("ai.fallback_engine", "local");
      }
    }
  };

  const saveProviders = (
    next: AiProviderDefinition[],
    modelChange?: { providerId: string; oldModelId: string; newModelId?: string },
  ) => {
    const serialized = serializeProvidersConfig(next);
    set("ai.providers_config", serialized);
    reconcileAiRoutes(next, modelChange);
  };

  const activeProvider = useMemo(
    () => providers.find((p) => p.id === activeProviderId) ?? null,
    [providers, activeProviderId],
  );

  const updateProvider = (
    id: string,
    updater: (prev: AiProviderDefinition) => AiProviderDefinition,
    modelChange?: { providerId: string; oldModelId: string; newModelId?: string },
  ) => {
    const next = providers.map((p) => (p.id === id ? updater(p) : p));
    saveProviders(next, modelChange);
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
      .catch((e) => {
        const msg = String(e);
        setDownloadNote(msg);
        setAiDiagnosticError({
          provider: "Local On-Device Model",
          message: msg,
        });
      });
  };

  const saveHotkey = () => {
    setHotkeyNote(null);
    ipc
      .setPanelHotkey(hotkey)
      .then(() => {
        setHotkeyNote(S.hotkeySaved);
        const saved = hotkey.trim() || "ctrl+shift+space";
        setSettings((s) => ({ ...s, "panel.hotkey": saved }));
        setHotkey("");
      })
      .catch((e) => setHotkeyNote(String(e)));
  };

  const handleSaveProviderKey = async (providerId: string) => {
    const draftKey = (keys[providerId] ?? "").trim();
    try {
      await ipc.aiSetKey(providerId, draftKey || null);
      setKeys((k) => ({ ...k, [providerId]: "" }));
      setKeySavedNote(draftKey ? "API key saved to Windows Credential Manager" : "API key cleared");
      setTimeout(() => setKeySavedNote(null), 2500);
      reload();
    } catch (e) {
      setAiDiagnosticError({
        provider: providerId,
        message: String(e),
      });
    }
  };

  const handleRefreshModelsFromEndpoint = async (p: AiProviderDefinition) => {
    setRefreshingModels(true);
    setProviderStatusNote(null);
    setAiDiagnosticError(null);
    try {
      const draftKey = (keys[p.id] ?? "").trim();
      if (draftKey) {
        await ipc.aiSetKey(p.id, draftKey);
        setKeys((k) => ({ ...k, [p.id]: "" }));
      }
      const discovered = await ipc.aiFetchModels(
        p.id,
        p.baseUrl,
        draftKey || null,
        p.apiFormat,
      );
      if (discovered.length > 0) {
        const existingIds = new Set(p.models.map((m) => m.id));
        const added = discovered
          .filter((id) => !existingIds.has(id))
          .slice(0, 25)
          .map((id) => ({ id, capabilities: inferModelCapabilities(id) }));
        const mergedModels = [...p.models, ...added];
        updateProvider(p.id, (prev) => ({
          ...prev,
          models: mergedModels,
          defaultModel: prev.defaultModel || mergedModels[0]?.id,
        }));
        setProviderStatusNote(
          added.length > 0
            ? `Connected · Added ${added.length} new models (${discovered.length} available)`
            : `Connected · ${discovered.length} models verified`,
        );
      } else {
        setProviderStatusNote("Endpoint reachable (0 models listed; add model IDs manually)");
      }
      reload();
    } catch (e) {
      const msg = String(e);
      setAiDiagnosticError({
        provider: p.name,
        message: msg,
        endpoint: sanitizeEndpointUrl(p.baseUrl),
      });
      reloadLogs();
    } finally {
      setRefreshingModels(false);
    }
  };

  const handleCreateCustomProvider = async () => {
    const trimmedName = customName.trim();
    const trimmedUrl = customBaseUrl.trim().replace(/\/+$/, "");
    const trimmedKey = customApiKey.trim();
    if (!trimmedName || !trimmedUrl) {
      setCustomError("Provider name and Base URL are required.");
      return;
    }
    if (
      !/^https:\/\/[^\s]+$/i.test(trimmedUrl) &&
      !/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?(\/[^\s]*)?$/i.test(trimmedUrl)
    ) {
      setCustomError("Base URL must start with https:// (or http://127.0.0.1 for local servers).");
      return;
    }
    const slug = `custom_${trimmedName
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "")
      .slice(0, 36) || Date.now()}`;
    if (providers.some((p) => p.id === slug || p.name.toLowerCase() === trimmedName.toLowerCase())) {
      setCustomError(`A provider named "${trimmedName}" already exists.`);
      return;
    }
    const firstModel = customInitialModel.trim() || (customFormat === "anthropic" ? "claude-sonnet-4-5" : "gpt-4o-mini");
    const newProvider: AiProviderDefinition = {
      id: slug,
      name: trimmedName,
      apiFormat: customFormat,
      baseUrl: trimmedUrl,
      enabled: true,
      custom: true,
      defaultModel: firstModel,
      models: [{ id: firstModel, capabilities: inferModelCapabilities(firstModel) }],
      accentBg: "rgba(139, 92, 246, 0.18)",
      accentFg: "#a78bfa",
      monogram: trimmedName.slice(0, 2).toUpperCase(),
    };
    const next = [...providers, newProvider];
    saveProviders(next);
    if (trimmedKey) {
      try {
        await ipc.aiSetKey(slug, trimmedKey);
      } catch (e) {
        setAiDiagnosticError({
          provider: trimmedName,
          message: String(e),
          endpoint: sanitizeEndpointUrl(trimmedUrl),
        });
      }
    }
    await set("ai.engine", slug);
    await set("ai.byo_model", firstModel);
    setAi((prev) => (prev ? { ...prev, engine: slug } : prev));
    // Refresh ai_status immediately so the provider modal we open below sees
    // the freshly stored key as `supplied` instead of unset until next reload.
    reload();
    setAddProviderOpen(false);
    setCustomName("");
    setCustomBaseUrl("");
    setCustomApiKey("");
    setCustomInitialModel("");
    setCustomError(null);
    setActiveProviderId(slug);
  };

  // Options for Engine, Default Model, Writing Model, and Fallback Engine
  const engineOptions = useMemo(() => {
    const base = [
      { value: "local", label: "Local On-Device (llama.cpp)" },
      { value: "chatgpt", label: "ChatGPT (OAuth Plan)" },
      ...providers.map((p) => ({
        value: p.id,
        label: p.name,
        badge: p.enabled ? "Enabled" : undefined,
      })),
    ];
    return base;
  }, [providers]);

  const allModelRouteOptions = useMemo(() => {
    const list: { value: string; label: string; badge?: string }[] = [
      {
        value: "local::local",
        label: "Local On-Device · Hardware-matched GGUF",
        badge: "Offline",
      },
    ];
    if (ai?.chatgpt_email) {
      list.push({
        value: "chatgpt::gpt-4.1-mini",
        label: "ChatGPT OAuth · gpt-4.1-mini",
        badge: "Plan",
      });
    }
    for (const p of providers) {
      if (!p.enabled) continue;
      for (const m of p.models) {
        list.push({
          value: `${p.id}::${m.id}`,
          label: `${p.name} · ${m.id}`,
          badge: p.apiFormat === "anthropic" ? "Anthropic" : "OpenAI",
        });
      }
    }
    return list;
  }, [providers, ai?.chatgpt_email]);

  const currentDefaultComposite = useMemo(() => {
    const eng = str("ai.engine") || ai?.engine || "local";
    if (eng === "local") return "local::local";
    const mod =
      str("ai.byo_model") ||
      providers.find((p) => p.id === eng)?.defaultModel ||
      providers.find((p) => p.id === eng)?.models[0]?.id ||
      "gpt-4o-mini";
    return `${eng}::${mod}`;
  }, [ai?.engine, settings, providers]);

  const currentWritingComposite = useMemo(() => {
    const wEng = str("ai.writing_engine");
    const wMod = str("ai.writing_model");
    if (!wEng || wEng === "same") return "same";
    if (wEng === "local") return "local::local";
    return `${wEng}::${wMod || "gpt-4o-mini"}`;
  }, [settings]);

  const openGitHubIssueWithLogs = () => {
    const recentLogs = logs
      .slice(-15)
      .map((l) => `[${new Date(l.ts).toISOString()}] [${l.level.toUpperCase()}] ${l.msg}`)
      .join("\n");
    const title = encodeURIComponent(
      `[AI Provider Error] ${aiDiagnosticError?.provider ?? "Custom Endpoint"}: ${
        aiDiagnosticError?.message.slice(0, 70) ?? "Connection failure"
      }`,
    );
    const body = encodeURIComponent(
      `### Provider & Endpoint\n- **Provider**: ${
        aiDiagnosticError?.provider ?? "Unknown"
      }\n- **Base URL**: ${aiDiagnosticError?.endpoint ?? "N/A"}\n- **Error**: \`${
        aiDiagnosticError?.message ?? ""
      }\`\n\n### Recent Debug Logs\n\`\`\`text\n${
        recentLogs || "No recent logs recorded."
      }\n\`\`\``,
    );
    void ipc.openExternalUrl(
      `https://github.com/chirag-gamer/cracktivity/issues/new?title=${title}&body=${body}`,
    );
  };

  return (
    <div className="pane-list settings" data-testid="settings-view">
      <header className="settings-page-header">
        <div>
          <h2 className="settings-page-title">{S.settings}</h2>
          <p className="settings-page-subtitle">
            Configure on-device AI and Jan-style external model providers, custom
            OpenAI/Anthropic endpoints, MCP agent rules, appearance, and diagnostics.
          </p>
        </div>
      </header>

      <section aria-label={S.settingsAi}>
        <div className="jan-section-head">
          <h3>
            <Bot size={16} />
            <span>{S.settingsAi}</span>
          </h3>
          {ai && (
            <Switch
              checked={ai.enabled}
              label={S.aiMasterSwitch}
              data-testid="ai-master-switch"
              onChange={(_, d) => {
                ipc.aiSetEnabled(d.checked).then(reload).catch(() => {});
              }}
            />
          )}
        </div>

        {ai && (
          <>
            {/* 1. Local AI Model Row (with slide switch + tier + download/delete) */}
            <div className="jan-card" data-testid="local-ai-card">
              <div className="jan-card-header">
                <span className="jan-card-title">Local AI Model (On-Device)</span>
                <BadgePill variant={ai.model_downloaded ? "success" : "neutral"} size="xs">
                  <span data-testid="ai-model-state">
                    {ai.model_downloaded ? S.aiModelReady : S.aiModelDownload}
                  </span>
                </BadgePill>
              </div>

              <div className="jan-provider-row">
                <div className="jan-provider-left">
                  <span
                    className="jan-provider-avatar"
                    style={{
                      background: "rgba(59, 130, 246, 0.18)",
                      color: "#60a5fa",
                    }}
                  >
                    <Bot size={15} />
                  </span>
                  <div className="jan-provider-meta">
                    <div className="jan-provider-name">
                      llama.cpp Local Engine ({ai.tier.toUpperCase()} tier)
                    </div>
                    <div className="jan-provider-sub">
                      {ai.tier_reason} · {Math.round(ai.model_disk_bytes / 1e6)} MB on disk
                    </div>
                  </div>
                </div>

                <div className="jan-provider-actions">
                  {!ai.model_downloaded ? (
                    <UiButton
                      variant="primary"
                      size="xs"
                      onClick={download}
                      data-testid="ai-download"
                    >
                      {downloadNote ?? S.aiModelDownload}
                    </UiButton>
                  ) : (
                    <UiButton
                      variant="ghost"
                      size="xs"
                      onClick={() => ipc.aiDeleteModel().then(reload).catch(() => {})}
                    >
                      {S.aiModelDelete}
                    </UiButton>
                  )}
                  <UiSwitch
                    checked={ai.enabled && ai.engine === "local"}
                    label="Use Local AI Model"
                    testId="local-ai-switch"
                    onChange={(checked) => {
                      if (checked) {
                        void ipc.aiSetEnabled(true).then(() => {
                          set("ai.engine", "local");
                          reload();
                        });
                      } else if (ai.engine === "local") {
                        const firstActive = providers.find((p) => p.enabled)?.id ?? "openai";
                        set("ai.engine", firstActive);
                        reload();
                      }
                    }}
                  />
                </div>
              </div>

              <div className="jan-local-controls">
                <div className="jan-control-col">
                  <label htmlFor="ai-tier">
                    {S.aiTier} ({ai.tier_reason})
                  </label>
                  <UiSelect
                    id="ai-tier"
                    testId="ai-tier"
                    value={ai.tier_is_override ? ai.tier : "auto"}
                    options={[
                      { value: "auto", label: `${S.aiTierAuto} (${ai.tier})` },
                      ...TIERS.map((t) => ({
                        value: t,
                        label: `${t.toUpperCase()} tier`,
                      })),
                    ]}
                    onChange={(val) => {
                      set("ai.tier_override", val === "auto" ? null : val);
                      setTimeout(reload, 60);
                    }}
                  />
                </div>

                <div className="jan-control-col">
                  <label htmlFor="ai-engine">{S.aiEngine}</label>
                  <UiSelect
                    id="ai-engine"
                    testId="ai-engine"
                    value={ai.engine}
                    options={engineOptions}
                    onChange={(val) => {
                      set("ai.engine", val);
                      setAi((prev) => (prev ? { ...prev, engine: val } : prev));
                    }}
                  />
                </div>
              </div>
            </div>

            {/* 2. Default Model, Writing Model & Fallback Routing Card */}
            <div className="jan-card" data-testid="ai-routing-card">
              <div className="jan-card-header">
                <span className="jan-card-title">
                  Model Routing & Automatic Fallback
                </span>
                <Text size={200} className="hint">
                  Pick a default model for summarization, a dedicated model for writing/replies, and a fallback engine.
                </Text>
              </div>
              <div className="jan-routing-grid">
                <div className="jan-control-col">
                  <label htmlFor="ai-default-model">
                    Default Model (Summaries & General)
                  </label>
                  <UiSelect
                    id="ai-default-model"
                    testId="ai-default-model"
                    value={currentDefaultComposite}
                    options={allModelRouteOptions}
                    onChange={(composite) => {
                      const [engId, modId] = composite.split("::");
                      if (engId) {
                        set("ai.engine", engId);
                        setAi((prev) => (prev ? { ...prev, engine: engId } : prev));
                      }
                      if (modId && engId !== "local") {
                        set("ai.byo_model", modId);
                      }
                    }}
                  />
                </div>

                <div className="jan-control-col">
                  <label htmlFor="ai-writing-route">
                    Writing & Draft Reply Model
                  </label>
                  <UiSelect
                    id="ai-writing-route"
                    testId="ai-writing-route"
                    value={currentWritingComposite}
                    options={[
                      {
                        value: "same",
                        label: "Same as Default Model",
                        badge: "Auto",
                      },
                      ...allModelRouteOptions,
                    ]}
                    onChange={(composite) => {
                      if (composite === "same") {
                        set("ai.writing_engine", "same");
                        set("ai.writing_model", "");
                      } else {
                        const [engId, modId] = composite.split("::");
                        set("ai.writing_engine", engId || "same");
                        set("ai.writing_model", modId || "");
                      }
                    }}
                  />
                </div>

                <div className="jan-control-col">
                  <label htmlFor="ai-fallback-engine">
                    Fallback Engine (on API error)
                  </label>
                  <UiSelect
                    id="ai-fallback-engine"
                    testId="ai-fallback-engine"
                    value={str("ai.fallback_engine") || "local"}
                    options={[
                      {
                        value: "local",
                        label: "Local On-Device Model (llama.cpp)",
                        badge: "Recommended",
                      },
                      ...providers
                        .filter((p) => p.enabled)
                        .map((p) => ({
                          value: p.id,
                          label: `${p.name} (${p.defaultModel || p.models[0]?.id || "default"})`,
                        })),
                      { value: "none", label: "Disabled (never fall back)" },
                    ]}
                    onChange={(val) => set("ai.fallback_engine", val)}
                  />
                </div>
              </div>
            </div>

            {/* 3. External APIs Card (matching Jan's Model Providers list in image 1) */}
            <div className="jan-card" data-testid="external-apis-card">
              <div className="jan-card-header">
                <div>
                  <span className="jan-card-title">External APIs</span>
                  <p className="jan-card-desc">
                    Enable any provider and click the gear icon to configure its API Key, Base URL, and Models.
                  </p>
                </div>
                <UiButton
                  variant="secondary"
                  size="sm"
                  icon={<Plus size={14} />}
                  data-testid="add-provider-trigger"
                  onClick={() => {
                    setCustomError(null);
                    setAddProviderOpen(true);
                  }}
                >
                  Add Provider
                </UiButton>
              </div>

              <div className="jan-provider-list">
                {providers.map((p) => {
                  const hasKey = Boolean(ai.byo_configured[p.id]);
                  const isCurrentDefault = ai.engine === p.id;
                  return (
                    <div
                      key={p.id}
                      className={`jan-provider-row${p.enabled ? " is-enabled" : ""}`}
                      data-testid={`provider-row-${p.id}`}
                    >
                      <div className="jan-provider-left">
                        <span
                          className="jan-provider-avatar"
                          style={{
                            background: p.accentBg,
                            color: p.accentFg,
                          }}
                        >
                          {p.monogram}
                        </span>
                        <div className="jan-provider-meta">
                          <div className="jan-provider-name-line">
                            <span className="jan-provider-name">{p.name}</span>
                            {p.custom && (
                              <BadgePill variant="ai" size="xs">
                                Custom
                              </BadgePill>
                            )}
                            {hasKey && (
                              <BadgePill variant="success" size="xs">
                                {S.aiKeySet}
                              </BadgePill>
                            )}
                            {isCurrentDefault && (
                              <BadgePill variant="brand" size="xs">
                                Active Default
                              </BadgePill>
                            )}
                          </div>
                          <div className="jan-provider-sub">
                            {p.models.length} Models · <code>{p.baseUrl}</code>
                          </div>
                        </div>
                      </div>

                      <div className="jan-provider-actions">
                        {p.enabled && (
                          <IconButton
                            label={`Configure ${p.name}`}
                            size="sm"
                            variant="ghost"
                            data-testid={`provider-settings-btn-${p.id}`}
                            onClick={() => {
                              setProviderStatusNote(null);
                              setKeySavedNote(null);
                              setKeyRevealed(false);
                              setShowAddModelBar(false);
                              setActiveProviderId(p.id);
                            }}
                          >
                            <Settings size={15} />
                          </IconButton>
                        )}
                        <UiSwitch
                          checked={p.enabled}
                          label={`Enable ${p.name}`}
                          testId={`provider-toggle-${p.id}`}
                          onChange={(nextEnabled) => {
                            updateProvider(p.id, (prev) => ({
                              ...prev,
                              enabled: nextEnabled,
                            }));
                            if (nextEnabled) {
                              setProviderStatusNote(null);
                              setKeySavedNote(null);
                              setActiveProviderId(p.id);
                            }
                          }}
                        />
                      </div>
                    </div>
                  );
                })}

                {/* Sign in with ChatGPT OAuth Row */}
                <div
                  className="jan-provider-row"
                  data-testid="ai-chatgpt-row"
                >
                  <div className="jan-provider-left">
                    <span
                      className="jan-provider-avatar"
                      style={{
                        background: "rgba(16, 163, 127, 0.18)",
                        color: "#10a37f",
                      }}
                    >
                      <Sparkles size={14} />
                    </span>
                    <div className="jan-provider-meta">
                      <div className="jan-provider-name-line">
                        <span className="jan-provider-name">
                          ChatGPT Subscription (OAuth)
                        </span>
                        {ai.chatgpt_email && (
                          <BadgePill variant="success" size="xs">
                            Connected
                          </BadgePill>
                        )}
                      </div>
                      <div className="jan-provider-sub">
                        {ai.chatgpt_email
                          ? `${S.aiChatgptConnected} ${ai.chatgpt_email}`
                          : S.aiChatgptHint}
                      </div>
                    </div>
                  </div>
                  <div className="jan-provider-actions">
                    {ai.chatgpt_email ? (
                      <button
                        type="button"
                        className="link-button"
                        disabled={chatgptBusy}
                        onClick={() => {
                          setChatgptBusy(true);
                          ipc
                            .aiChatgptDisconnect()
                            .then(reload)
                            .catch(() => {})
                            .finally(() => setChatgptBusy(false));
                        }}
                        data-testid="ai-chatgpt-disconnect"
                      >
                        {S.aiChatgptDisconnect}
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="primary-button"
                        disabled={chatgptBusy}
                        onClick={() => {
                          setChatgptBusy(true);
                          ipc
                            .aiChatgptConnect()
                            .then(() => reload())
                            .catch((e) => setChatgptError(String(e)))
                            .finally(() => setChatgptBusy(false));
                        }}
                        data-testid="ai-chatgpt-connect"
                      >
                        {chatgptBusy ? S.aiChatgptBusy : S.aiChatgptConnect}
                      </button>
                    )}
                    <UiSwitch
                      checked={ai.engine === "chatgpt"}
                      label="Use ChatGPT OAuth"
                      onChange={(checked) => {
                        set("ai.engine", checked ? "chatgpt" : "local");
                        reload();
                      }}
                    />
                  </div>
                </div>
              </div>

              {chatgptError && (
                <div
                  className="error-text"
                  role="alert"
                  data-testid="ai-chatgpt-error"
                >
                  {chatgptError}
                </div>
              )}

              <Text size={200} className="hint" style={{ marginTop: 8, display: "block" }}>
                {S.aiCostNote}
              </Text>
            </div>

            {/* 4. Diagnostic Error & GitHub Issue Reporter Banner */}
            {aiDiagnosticError && (
              <div
                className="jan-diag-error-card"
                role="alert"
                data-testid="ai-error-diagnostic"
              >
                <div className="jan-diag-error-head">
                  <span className="jan-diag-error-title">
                    <AlertTriangle size={15} />
                    <span>
                      AI Endpoint Error ({aiDiagnosticError.provider})
                    </span>
                  </span>
                  <IconButton
                    label="Dismiss error"
                    size="xs"
                    onClick={() => setAiDiagnosticError(null)}
                  >
                    <X size={13} />
                  </IconButton>
                </div>
                <p className="jan-diag-error-msg">{aiDiagnosticError.message}</p>
                <div className="jan-diag-log-preview">
                  {logs.slice(-5).map((l, idx) => (
                    <div key={idx} className="jan-diag-log-line">
                      [{new Date(l.ts).toLocaleTimeString()}] {l.level}: {l.msg}
                    </div>
                  ))}
                </div>
                <div className="jan-diag-actions">
                  <UiButton
                    variant="secondary"
                    size="xs"
                    icon={diagCopied ? <Check size={13} /> : <Copy size={13} />}
                    onClick={() => {
                      const payload = `Provider: ${aiDiagnosticError.provider}\nEndpoint: ${
                        aiDiagnosticError.endpoint ?? ""
                      }\nError: ${aiDiagnosticError.message}\nLogs:\n${logs
                        .slice(-15)
                        .map((l) => `${new Date(l.ts).toISOString()} [${l.level}] ${l.msg}`)
                        .join("\n")}`;
                      void navigator.clipboard?.writeText(payload);
                      setDiagCopied(true);
                      setTimeout(() => setDiagCopied(false), 2000);
                    }}
                  >
                    {diagCopied ? "Copied Debug Logs" : "Copy Debug Logs"}
                  </UiButton>
                  <UiButton
                    variant="primary"
                    size="xs"
                    icon={<Bug size={13} />}
                    trailingIcon={<ExternalLink size={12} />}
                    data-testid="ai-report-issue-btn"
                    onClick={openGitHubIssueWithLogs}
                  >
                    Open Issue on GitHub
                  </UiButton>
                </div>
              </div>
            )}
          </>
        )}
      </section>

      {/* Provider Detail Popup Modal (matching Jan's image 2: API Key, Base URL, Models list) */}
      <Dialog
        open={Boolean(activeProvider)}
        onOpenChange={(_, d) => !d.open && setActiveProviderId(null)}
      >
        <DialogSurface className="jan-provider-modal-surface" data-testid="provider-detail-modal">
          {activeProvider && (
            <DialogBody>
              <DialogTitle>
                <div className="jan-modal-header">
                  <div className="jan-provider-left">
                    <span
                      className="jan-provider-avatar"
                      style={{
                        background: activeProvider.accentBg,
                        color: activeProvider.accentFg,
                      }}
                    >
                      {activeProvider.monogram}
                    </span>
                    <span>{activeProvider.name}</span>
                    {ai?.byo_configured[activeProvider.id] && (
                      <BadgePill variant="success" size="xs">
                        {S.aiKeySet}
                      </BadgePill>
                    )}
                  </div>
                  <div className="jan-provider-actions">
                    <UiSwitch
                      checked={activeProvider.enabled}
                      label={`Enable ${activeProvider.name}`}
                      onChange={(checked) =>
                        updateProvider(activeProvider.id, (prev) => ({
                          ...prev,
                          enabled: checked,
                        }))
                      }
                    />
                    <IconButton
                      label="Close provider settings"
                      size="xs"
                      data-testid="provider-modal-close"
                      onClick={() => setActiveProviderId(null)}
                    >
                      <X size={15} />
                    </IconButton>
                  </div>
                </div>
              </DialogTitle>
              <DialogContent className="jan-modal-body">
                {/* API Key & Base URL Card (exact layout from image 2) */}
                <div className="jan-detail-card">
                  <div className="jan-detail-block">
                    <div className="jan-detail-label">API Key</div>
                    <p className="jan-detail-desc">
                      The {activeProvider.name} API uses API keys for authentication.
                      {activeProvider.apiKeysUrl && (
                        <>
                          {" "}
                          Visit your{" "}
                          <a
                            href={activeProvider.apiKeysUrl}
                            className="jan-accent-link"
                            onClick={(e) => {
                              e.preventDefault();
                              void ipc.openExternalUrl(activeProvider.apiKeysUrl!);
                            }}
                          >
                            API Keys
                          </a>{" "}
                          page to retrieve the API key you&apos;ll use in your requests.
                        </>
                      )}
                    </p>
                    <div className="jan-secret-input-row">
                      <div className="jan-secret-input-wrap">
                        <input
                          type={keyRevealed ? "text" : "password"}
                          className="jan-secret-input"
                          placeholder={
                            ai?.byo_configured[activeProvider.id]
                              ? "•••••••••••••••• (Stored in Windows Credential Manager)"
                              : "Insert API Key"
                          }
                          value={keys[activeProvider.id] ?? ""}
                          data-testid="byo-key-input"
                          aria-label={S.aiKeyPlaceholder}
                          onChange={(e) =>
                            setKeys((k) => ({
                              ...k,
                              [activeProvider.id]: e.target.value,
                            }))
                          }
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              void handleSaveProviderKey(activeProvider.id);
                            }
                          }}
                        />
                        <div className="jan-secret-inline-btns">
                          <button
                            type="button"
                            className="jan-inline-icon-btn"
                            aria-label={keyRevealed ? "Hide API Key" : "Reveal API Key"}
                            onClick={() => setKeyRevealed((v) => !v)}
                          >
                            {keyRevealed ? <EyeOff size={14} /> : <Eye size={14} />}
                          </button>
                          <button
                            type="button"
                            className="jan-inline-icon-btn"
                            aria-label="Copy API Key"
                            disabled={!keys[activeProvider.id]}
                            onClick={() => {
                              const val = keys[activeProvider.id];
                              if (!val) return;
                              void navigator.clipboard?.writeText(val);
                              setKeyCopied(true);
                              setTimeout(() => setKeyCopied(false), 1800);
                            }}
                          >
                            {keyCopied ? <Check size={14} /> : <Copy size={14} />}
                          </button>
                        </div>
                      </div>
                      <UiButton
                        variant="primary"
                        size="sm"
                        data-testid="provider-save-key"
                        onClick={() => void handleSaveProviderKey(activeProvider.id)}
                      >
                        {S.aiSaveKey}
                      </UiButton>
                    </div>
                    {keySavedNote && (
                      <div className="jan-status-ok" role="status">
                        {keySavedNote}
                      </div>
                    )}
                  </div>

                  <div className="jan-detail-block">
                    <div className="jan-detail-label">Base URL</div>
                    <p className="jan-detail-desc">
                      The base endpoint to use.
                      {activeProvider.docsUrl && (
                        <>
                          {" "}
                          See the{" "}
                          <a
                            href={activeProvider.docsUrl}
                            className="jan-accent-link"
                            onClick={(e) => {
                              e.preventDefault();
                              void ipc.openExternalUrl(activeProvider.docsUrl!);
                            }}
                          >
                            {activeProvider.docsLabel ??
                              `${activeProvider.name} API documentation`}
                          </a>{" "}
                          for more information.
                        </>
                      )}
                    </p>
                    <div className="jan-secret-input-row">
                      <input
                        type="text"
                        className="jan-baseurl-input"
                        value={activeProvider.baseUrl}
                        placeholder="https://api.openai.com/v1"
                        data-testid="provider-baseurl-input"
                        onChange={(e) =>
                          updateProvider(activeProvider.id, (prev) => ({
                            ...prev,
                            baseUrl: e.target.value,
                          }))
                        }
                      />
                      <UiButton
                        variant="secondary"
                        size="sm"
                        loading={refreshingModels}
                        data-testid="provider-test-btn"
                        onClick={() => void handleRefreshModelsFromEndpoint(activeProvider)}
                      >
                        Test & Sync
                      </UiButton>
                    </div>
                    {providerStatusNote && (
                      <div className="jan-status-ok" role="status">
                        {providerStatusNote}
                      </div>
                    )}
                  </div>
                </div>

                {/* Models Card (exact layout from image 2: Refresh ↻, Add +, capabilities Wrench/Eye, Edit, Delete) */}
                <div className="jan-detail-card">
                  <div className="jan-models-header">
                    <span className="jan-detail-label">Models</span>
                    <div className="jan-models-header-actions">
                      <IconButton
                        label="Refresh models from endpoint"
                        size="xs"
                        variant="secondary"
                        data-testid="provider-refresh-models"
                        disabled={refreshingModels}
                        onClick={() => void handleRefreshModelsFromEndpoint(activeProvider)}
                      >
                        <RefreshCw
                          size={14}
                          className={refreshingModels ? "spin-icon" : ""}
                        />
                      </IconButton>
                      <IconButton
                        label="Add model ID"
                        size="xs"
                        variant="secondary"
                        data-testid="provider-add-model-toggle"
                        onClick={() => setShowAddModelBar((v) => !v)}
                      >
                        <Plus size={14} />
                      </IconButton>
                    </div>
                  </div>

                  {showAddModelBar && (
                    <div className="jan-add-model-bar">
                      <input
                        type="text"
                        className="jan-baseurl-input"
                        placeholder="Enter model ID (e.g. gpt-4.1, claude-sonnet-4-5, deepseek-chat)"
                        value={newModelIdInput}
                        data-testid="provider-new-model-input"
                        onChange={(e) => setNewModelIdInput(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && newModelIdInput.trim()) {
                            e.preventDefault();
                            const id = newModelIdInput.trim();
                            if (!activeProvider.models.some((m) => m.id === id)) {
                              updateProvider(activeProvider.id, (prev) => ({
                                ...prev,
                                models: [
                                  { id, capabilities: inferModelCapabilities(id) },
                                  ...prev.models,
                                ],
                              }));
                            }
                            setNewModelIdInput("");
                            setShowAddModelBar(false);
                          }
                        }}
                      />
                      <UiButton
                        variant="primary"
                        size="xs"
                        data-testid="provider-new-model-submit"
                        disabled={!newModelIdInput.trim()}
                        onClick={() => {
                          const id = newModelIdInput.trim();
                          if (!id) return;
                          if (!activeProvider.models.some((m) => m.id === id)) {
                            updateProvider(activeProvider.id, (prev) => ({
                              ...prev,
                              models: [
                                { id, capabilities: inferModelCapabilities(id) },
                                ...prev.models,
                              ],
                            }));
                          }
                          setNewModelIdInput("");
                          setShowAddModelBar(false);
                        }}
                      >
                        Add Model
                      </UiButton>
                    </div>
                  )}

                  <div className="jan-models-list">
                    {activeProvider.models.map((m) => {
                      const isDefault =
                        ai?.engine === activeProvider.id &&
                        (str("ai.byo_model") || activeProvider.defaultModel) === m.id;
                      const isWriting =
                        str("ai.writing_engine") === activeProvider.id &&
                        str("ai.writing_model") === m.id;
                      const isEditing = editingModelId === m.id;

                      return (
                        <div
                          key={m.id}
                          className="jan-model-row"
                          data-testid={`provider-model-${m.id}`}
                        >
                          <div className="jan-model-left">
                            {isEditing ? (
                              <input
                                type="text"
                                className="jan-baseurl-input"
                                value={editingModelDraft}
                                onChange={(e) => setEditingModelDraft(e.target.value)}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter" && editingModelDraft.trim()) {
                                    const nextId = editingModelDraft.trim();
                                    updateProvider(
                                      activeProvider.id,
                                      (prev) => ({
                                        ...prev,
                                        defaultModel:
                                          prev.defaultModel === m.id
                                            ? nextId
                                            : prev.defaultModel,
                                        models: prev.models.map((item) =>
                                          item.id === m.id
                                            ? {
                                                id: nextId,
                                                capabilities: inferModelCapabilities(nextId),
                                              }
                                            : item,
                                        ),
                                      }),
                                      {
                                        providerId: activeProvider.id,
                                        oldModelId: m.id,
                                        newModelId: nextId,
                                      },
                                    );
                                    setEditingModelId(null);
                                  } else if (e.key === "Escape") {
                                    setEditingModelId(null);
                                  }
                                }}
                              />
                            ) : (
                              <>
                                <span className="jan-model-id">{m.id}</span>
                                <span className="jan-model-caps">
                                  {m.capabilities.includes("tools") && (
                                    <span
                                      className="jan-cap-icon"
                                      title="Tool calling & structured output"
                                    >
                                      <Wrench size={13} />
                                    </span>
                                  )}
                                  {m.capabilities.includes("vision") && (
                                    <span
                                      className="jan-cap-icon"
                                      title="Vision & multimodal input"
                                    >
                                      <Eye size={13} />
                                    </span>
                                  )}
                                </span>
                                {isDefault && (
                                  <BadgePill variant="brand" size="xs">
                                    Default
                                  </BadgePill>
                                )}
                                {isWriting && (
                                  <BadgePill variant="ai" size="xs">
                                    Writing
                                  </BadgePill>
                                )}
                              </>
                            )}
                          </div>

                          <div className="jan-model-actions">
                            <button
                              type="button"
                              className={`jan-mini-pill-btn${isDefault ? " is-active" : ""}`}
                              title="Set as Default AI Model"
                              onClick={() => {
                                updateProvider(activeProvider.id, (prev) => ({
                                  ...prev,
                                  enabled: true,
                                  defaultModel: m.id,
                                }));
                                set("ai.engine", activeProvider.id);
                                set("ai.byo_model", m.id);
                                setAi((prev) =>
                                  prev ? { ...prev, engine: activeProvider.id } : prev,
                                );
                              }}
                            >
                              Default
                            </button>
                            <button
                              type="button"
                              className={`jan-mini-pill-btn${isWriting ? " is-active" : ""}`}
                              title="Use specifically for Writing / Draft Reply"
                              onClick={() => {
                                set("ai.writing_engine", activeProvider.id);
                                set("ai.writing_model", m.id);
                              }}
                            >
                              Writing
                            </button>
                            <IconButton
                              label={`Edit ${m.id}`}
                              size="xs"
                              variant="ghost"
                              data-testid={`provider-model-edit-${m.id}`}
                              onClick={() => {
                                setEditingModelId(m.id);
                                setEditingModelDraft(m.id);
                              }}
                            >
                              <Pencil size={13} />
                            </IconButton>
                            <IconButton
                              label={`Delete ${m.id}`}
                              size="xs"
                              variant="ghost"
                              data-testid={`provider-model-delete-${m.id}`}
                              onClick={() =>
                                updateProvider(
                                  activeProvider.id,
                                  (prev) => {
                                    const remaining = prev.models.filter(
                                      (item) => item.id !== m.id,
                                    );
                                    return {
                                      ...prev,
                                      defaultModel:
                                        prev.defaultModel === m.id
                                          ? remaining[0]?.id
                                          : prev.defaultModel,
                                      models: remaining,
                                    };
                                  },
                                  {
                                    providerId: activeProvider.id,
                                    oldModelId: m.id,
                                  },
                                )
                              }
                            >
                              <Trash2 size={13} />
                            </IconButton>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>

                {activeProvider.custom && (
                  <div className="jan-delete-provider-bar">
                    <UiButton
                      variant="danger"
                      size="xs"
                      icon={<Trash2 size={13} />}
                      data-testid="delete-custom-provider-btn"
                      onClick={() => {
                        const next = providers.filter((p) => p.id !== activeProvider.id);
                        saveProviders(next);
                        void ipc.aiSetKey(activeProvider.id, null);
                        setActiveProviderId(null);
                      }}
                    >
                      Delete Custom Provider
                    </UiButton>
                  </div>
                )}
              </DialogContent>
            </DialogBody>
          )}
        </DialogSurface>
      </Dialog>

      {/* Add Custom Provider Dialog (matching Jan's AddProviderDialog.tsx) */}
      <Dialog
        open={addProviderOpen}
        onOpenChange={(_, d) => !d.open && setAddProviderOpen(false)}
      >
        <DialogSurface className="jan-add-provider-surface" data-testid="add-provider-dialog">
          <DialogBody>
            <DialogTitle>
              <div className="jan-modal-header">
                <span>Add Custom AI Provider</span>
                <IconButton
                  label="Close dialog"
                  size="xs"
                  onClick={() => setAddProviderOpen(false)}
                >
                  <X size={15} />
                </IconButton>
              </div>
            </DialogTitle>
            <DialogContent className="jan-modal-body">
              <div className="jan-detail-block">
                <label className="jan-detail-label" htmlFor="custom-provider-name">
                  Provider Name
                </label>
                <input
                  id="custom-provider-name"
                  type="text"
                  className="jan-baseurl-input"
                  placeholder="e.g. Ollama, DeepSeek, Together AI, LM Studio, Azure"
                  value={customName}
                  data-testid="provider-name-input"
                  onChange={(e) => setCustomName(e.target.value)}
                />
              </div>

              <div className="jan-detail-block">
                <label className="jan-detail-label" htmlFor="custom-provider-format">
                  API Wire Format
                </label>
                <UiSelect
                  id="custom-provider-format"
                  testId="provider-format-select"
                  value={customFormat}
                  options={[
                    {
                      value: "openai",
                      label: "OpenAI-Compatible (/v1/chat/completions)",
                      badge: "Standard",
                    },
                    {
                      value: "anthropic",
                      label: "Anthropic Messages (/v1/messages)",
                      badge: "Claude",
                    },
                  ]}
                  onChange={(val) => setCustomFormat(val as "openai" | "anthropic")}
                />
              </div>

              <div className="jan-detail-block">
                <label className="jan-detail-label" htmlFor="custom-provider-url">
                  Base URL
                </label>
                <input
                  id="custom-provider-url"
                  type="text"
                  className="jan-baseurl-input"
                  placeholder={
                    customFormat === "anthropic"
                      ? "https://api.anthropic.com"
                      : "https://api.deepseek.com/v1 or http://127.0.0.1:11434/v1"
                  }
                  value={customBaseUrl}
                  data-testid="provider-base-url-input"
                  onChange={(e) => {
                    setCustomBaseUrl(e.target.value);
                    if (customError) setCustomError(null);
                  }}
                />
              </div>

              <div className="jan-detail-block">
                <label className="jan-detail-label" htmlFor="custom-provider-key">
                  API Key
                </label>
                <input
                  id="custom-provider-key"
                  type="password"
                  className="jan-baseurl-input"
                  placeholder="Insert API Key (optional for local 127.0.0.1 servers)"
                  value={customApiKey}
                  data-testid="provider-api-key-input"
                  onChange={(e) => setCustomApiKey(e.target.value)}
                />
              </div>

              <div className="jan-detail-block">
                <label className="jan-detail-label" htmlFor="custom-provider-model">
                  Default Model ID
                </label>
                <input
                  id="custom-provider-model"
                  type="text"
                  className="jan-baseurl-input"
                  placeholder="e.g. deepseek-chat, llama3.2, qwen2.5-coder"
                  value={customInitialModel}
                  data-testid="provider-initial-model-input"
                  onChange={(e) => setCustomInitialModel(e.target.value)}
                />
              </div>

              {customError && (
                <div className="dialog-error-banner" role="alert">
                  <Text size={200} className="error-text">
                    {customError}
                  </Text>
                </div>
              )}

              <div className="jan-dialog-footer">
                <UiButton
                  variant="ghost"
                  size="sm"
                  onClick={() => setAddProviderOpen(false)}
                >
                  {S.cancel}
                </UiButton>
                <UiButton
                  variant="primary"
                  size="sm"
                  data-testid="add-provider-submit"
                  disabled={!customName.trim() || !customBaseUrl.trim()}
                  onClick={() => void handleCreateCustomProvider()}
                >
                  Create Provider
                </UiButton>
              </div>
            </DialogContent>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <section aria-label={S.settingsMcp}>
        <h3>
          <KeyRound size={16} />
          <span>{S.settingsMcp}</span>
        </h3>
        <Switch
          checked={bool("mcp.allow_send")}
          label={S.mcpAllowSend}
          data-testid="mcp-allow-send"
          onChange={(_, d) => set("mcp.allow_send", d.checked)}
        />
        <div className="field-row">
          {MCP_CLIENTS.map((c) => (
            <button
              type="button"
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
        <h3>
          <Settings size={16} />
          <span>{S.settingsGeneral}</span>
        </h3>
        <Switch
          checked={bool("search.also_server")}
          label={S.settingsSearch}
          data-testid="search-also-server"
          onChange={(_, d) => set("search.also_server", d.checked)}
        />
        <div className="field-row">
          <label htmlFor="followup-days">{S.followUpDays}</label>
          <UiSelect
            id="followup-days"
            testId="followup-days"
            value={str("followup.days") || "3"}
            options={[1, 2, 3, 5, 7].map((d) => ({
              value: String(d),
              label: `${d} ${d === 1 ? "day" : "days"}`,
            }))}
            onChange={(val) =>
              ipc
                .setFollowupDays(Number(val))
                .then(reload)
                .catch(() => {})
            }
          />
        </div>
      </section>

      <section aria-label={S.settingsAppearance}>
        <h3>
          <Eye size={16} />
          <span>{S.settingsAppearance}</span>
        </h3>
        <div className="field-row">
          <label htmlFor="theme-choice">{S.themeChoice}</label>
          <UiSelect
            id="theme-choice"
            testId="theme-choice"
            value={choice}
            options={[
              { value: "system", label: S.themeSystem, badge: "Auto" },
              { value: "light", label: S.themeLight },
              { value: "dark", label: S.themeDark },
            ]}
            onChange={(val) => setChoice(val as ThemeChoice)}
          />
        </div>
        <div className="field-row">
          <label htmlFor="keymap-choice">{S.keymapChoice}</label>
          <UiSelect
            id="keymap-choice"
            testId="keymap-choice"
            value={keymap}
            options={KEYMAP_SCHEMES.map((k) => ({
              value: k.id,
              label: k.label,
            }))}
            onChange={(val) => {
              const scheme = val as KeymapScheme;
              writeKeymapScheme(scheme);
              setKeymap(scheme);
            }}
          />
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
          <button
            type="button"
            className="link-button"
            onClick={saveHotkey}
            data-testid="hotkey-save"
          >
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
        <h3>
          <History size={16} />
          <span>{S.diagnostics}</span>
        </h3>
        <div className="field-row" data-testid="diagnostics-hosts">
          <span>{S.diagnosticsHosts}:</span>
          {hosts && hosts.length > 0 ? (
            hosts.map((h) => (
              <span key={h} className="audit-detail">
                {h}
              </span>
            ))
          ) : (
            <Text size={200}>{S.noHosts}</Text>
          )}
        </div>
        <div className="field-row" data-testid="storage-usage">
          <span>{S.settingsStorage}:</span>
          {storage && (
            <>
              <span>
                {S.storageDb}: {Math.round(storage.db_bytes / 1e6)} MB
              </span>
              <span>
                {S.storageModels}: {Math.round(storage.models_bytes / 1e6)} MB
              </span>
            </>
          )}
        </div>
        <div className="field-row">
          <button
            type="button"
            className="link-button"
            data-testid="prune-cache"
            onClick={() =>
              ipc
                .pruneCache()
                .then((n) =>
                  setPruneNote(n > 0 ? S.prunedBytes(n) : S.pruneNothing),
                )
                .catch(() => setPruneNote(S.pruneFailed))
            }
          >
            {S.pruneCache}
          </button>
          {pruneNote && (
            <Text size={200} data-testid="prune-result">
              {pruneNote}
            </Text>
          )}
        </div>
        <h4>{S.logsTitle}</h4>
        <div
          className="audit"
          role="log"
          aria-label={S.logsTitle}
          data-testid="diag-logs"
        >
          {logs.length === 0 ? (
            <div className="audit-row">{S.logsEmpty}</div>
          ) : (
            logs.map((l, i) => (
              <div key={i} className="audit-row">
                <span className="audit-time">
                  {new Date(l.ts).toLocaleTimeString()}
                </span>
                <span className="audit-actor">{l.level}</span>
                <span className="audit-detail">{l.msg}</span>
              </div>
            ))
          )}
        </div>
        <div className="field-row">
          <button
            type="button"
            className="link-button"
            data-testid="logs-clear"
            onClick={() => {
              ipc
                .clearDiagnosticsLogs()
                .then(() => {
                  setLogsCleared(true);
                  reloadLogs();
                })
                .catch(() => {});
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
