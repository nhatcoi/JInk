import { load, type Store } from "@tauri-apps/plugin-store";
import { DEFAULT_SHORTCUTS, type ActionId } from "@/lib/shortcuts";

export type AiHistoryEntry = {
  baseUrl: string;
  model: string;
  key: string;
  usedAt: number;
};

export type Settings = {
  aiBaseUrl: string;
  aiModel: string;
  aiKey: string;
  /** Past AI configs, most recent first — lets the user switch back without retyping. */
  aiHistory: AiHistoryEntry[];
  /** Tavily API key — enables the web-search tool for Explain. Empty = off. */
  searchKey: string;
  /** Global hotkey that shows/hides the popup — works even when unfocused. */
  hotkey: string;
  width: number;
  height: number;
  /** In-popup action shortcuts (insert, close, enhance, …) — only fire while focused. */
  shortcuts: Record<ActionId, string>;
  translateFrom: string;
  translateTo: string;
  /** Language the AI answers Explain in; falls back to English if unsupported. */
  mainLanguage: string;
  hideOnBlur: boolean;
  /** MediaDeviceInfo.deviceId of the mic to record with — "" means system default. */
  micDeviceId: string;
};

export const DEFAULT_SETTINGS: Settings = {
  aiBaseUrl: "",
  aiModel: "",
  aiKey: "",
  aiHistory: [],
  searchKey: "",
  hotkey: "Alt+Space",
  width: 700,
  height: 150,
  shortcuts: DEFAULT_SHORTCUTS,
  translateFrom: "auto",
  translateTo: "en",
  mainLanguage: "en",
  hideOnBlur: true,
  micDeviceId: "",
};

let store: Store | null = null;

async function getStore(): Promise<Store> {
  if (!store)
    store = await load("settings.json", { autoSave: true, defaults: {} });
  return store;
}

export async function loadSettings(): Promise<Settings> {
  const s = await getStore();
  const saved = (await s.get<Partial<Settings>>("settings")) ?? {};
  // Undo was Ctrl+Alt+Z before it owned Ctrl+Z — drop the stale binding.
  const savedShortcuts: Partial<Record<ActionId, string>> = {
    ...saved.shortcuts,
  };
  if (savedShortcuts.undo === "CmdOrCtrl+Alt+KeyZ") delete savedShortcuts.undo;
  return {
    ...DEFAULT_SETTINGS,
    ...saved,
    // Shallow-spread would drop any default action missing from an older
    // settings.json (e.g. one saved before a new action was added).
    shortcuts: { ...DEFAULT_SHORTCUTS, ...savedShortcuts },
    aiHistory: saved.aiHistory ?? DEFAULT_SETTINGS.aiHistory,
  };
}

export async function saveSettings(next: Settings): Promise<void> {
  const s = await getStore();
  await s.set("settings", next);
  await s.save();
}

const AI_HISTORY_MAX = 10;

/** Move (or add) a config to the front of the history, deduped by baseUrl+model. */
export function recordAiHistory(
  history: AiHistoryEntry[],
  entry: Omit<AiHistoryEntry, "usedAt">,
): AiHistoryEntry[] {
  if (!entry.baseUrl || !entry.model) return history;
  const rest = history.filter(
    (h) => !(h.baseUrl === entry.baseUrl && h.model === entry.model),
  );
  return [{ ...entry, usedAt: Date.now() }, ...rest].slice(0, AI_HISTORY_MAX);
}
