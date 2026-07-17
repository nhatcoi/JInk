import { load, type Store } from "@tauri-apps/plugin-store";
import { DEFAULT_SHORTCUTS, type ActionId } from "@/lib/shortcuts";

export type Settings = {
  aiBaseUrl: string;
  aiModel: string;
  aiKey: string;
  /** Global hotkey that shows/hides the popup — works even when unfocused. */
  hotkey: string;
  width: number;
  height: number;
  /** In-popup action shortcuts (insert, close, enhance, …) — only fire while focused. */
  shortcuts: Record<ActionId, string>;
  translateFrom: string;
  translateTo: string;
  hideOnBlur: boolean;
  /** MediaDeviceInfo.deviceId of the mic to record with — "" means system default. */
  micDeviceId: string;
};

export const DEFAULT_SETTINGS: Settings = {
  aiBaseUrl: "",
  aiModel: "",
  aiKey: "",
  hotkey: "Alt+Space",
  width: 700,
  height: 150,
  shortcuts: DEFAULT_SHORTCUTS,
  translateFrom: "auto",
  translateTo: "en",
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
  };
}

export async function saveSettings(next: Settings): Promise<void> {
  const s = await getStore();
  await s.set("settings", next);
  await s.save();
}
