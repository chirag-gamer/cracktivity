/**
 * Mail store hook: the single source of UI truth. Loads from the local
 * encrypted cache via IPC (FR-8: fully offline reads) and refreshes on the
 * Rust-emitted `store-changed` / `account-status` events.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../shared/ipc";
import type {
  AccountStatus,
  AccountSummary,
  ImportanceFilter,
  MessageRow,
  ThreadRow,
  ViewScope,
} from "../shared/types";

export interface MailState {
  accounts: AccountSummary[];
  statuses: AccountStatus[];
  threads: ThreadRow[];
  selectedThrid: string | null;
  messages: MessageRow[];
  scope: ViewScope;
  filter: ImportanceFilter;
  loading: boolean;
  setScope: (s: ViewScope) => void;
  setFilter: (f: ImportanceFilter) => void;
  select: (thrid: string | null) => void;
  refresh: () => void;
}

export function useMail(): MailState {
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [statuses, setStatuses] = useState<AccountStatus[]>([]);
  const [threads, setThreads] = useState<ThreadRow[]>([]);
  const [messages, setMessages] = useState<MessageRow[]>([]);
  const [selectedThrid, setSelectedThrid] = useState<string | null>(null);
  const [scope, setScopeState] = useState<ViewScope>({ kind: "unified" });
  const [filter, setFilterState] = useState<ImportanceFilter>("all");
  const [loading, setLoading] = useState(true);
  const tick = useRef(0);

  const reloadLists = useCallback(async () => {
    const id = ++tick.current;
    const [acc, sts, list] = await Promise.all([
      ipc.listAccounts(),
      ipc.accountStatuses(),
      ipc.threadList(scope.kind === "account" ? scope.id : null, filter),
    ]);
    if (id !== tick.current) return; // a newer load superseded this one
    // `?? []`: a failed/partial load never poisons the lists with null.
    setAccounts(acc ?? []);
    setStatuses(sts ?? []);
    setThreads(list ?? []);
    setLoading(false);
  }, [scope, filter]);

  // Initial load + reload on every store change / view switch.
  useEffect(() => {
    setLoading(true);
    reloadLists().catch(() => setLoading(false));
  }, [reloadLists]);

  // Thread contents follow the selection.
  useEffect(() => {
    if (!selectedThrid) {
      setMessages([]);
      return;
    }
    let live = true;
    ipc
      .threadMessages(selectedThrid)
      .then((m) => live && setMessages(m))
      .catch(() => live && setMessages([]));
    return () => {
      live = false;
    };
  }, [selectedThrid, threads]);

  useEffect(() => {
    // listen() rejects outside a Tauri webview (tests, plain browser):
    // degrade to polling-free static reads instead of crashing.
    const unsubs: Promise<(() => void) | null>[] = [
      listen("store-changed", () => reloadLists().catch(() => {})),
      listen("account-status", () => {
        // Status events arrive per-account; a cheap full refetch keeps this
        // simple (payload identical to account_statuses()).
        ipc.accountStatuses().then(setStatuses).catch(() => {});
      }),
      listen("sync-now", () => ipc.syncNow().catch(() => {})),
    ].map((p) => p.catch(() => null));
    return () =>
      unsubs.forEach((u) =>
        // The unlisten fns are async under the hood; in test environments
        // (no event bridge) they reject. never let that escape.
        u.then((f) => void Promise.resolve(f?.()).catch(() => {})),
      );
  }, [reloadLists]);

  const setScope = useCallback((s: ViewScope) => {
    setScopeState(s);
    setSelectedThrid(null);
  }, []);

  const select = useCallback((thrid: string | null) => setSelectedThrid(thrid), []);

  return {
    accounts,
    statuses,
    threads,
    selectedThrid,
    messages,
    scope,
    filter,
    loading,
    setScope,
    setFilter: setFilterState,
    select,
    refresh: () => reloadLists().catch(() => {}),
  };
}
