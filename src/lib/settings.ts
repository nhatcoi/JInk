import { load, type Store } from "@tauri-apps/plugin-store";

export type Settings = {
  aiBaseUrl: string;
  aiModel: string;
  aiKey: string;
  hotkey: string;
  width: number;
  height: number;
  submitKey: "enter" | "mod-enter";
  translateFrom: "vi" | "en";
  translateTo: "vi" | "en";
  hideOnBlur: boolean;
};

export const DEFAULT_SETTINGS: Settings = {
  aiBaseUrl: "https://api.openai.com/v1",
  aiModel: "gpt-4o-mini",
  aiKey: "",
  hotkey: "Alt+Space",
  width: 700,
  height: 150,
  submitKey: "enter",
  translateFrom: "vi",
  translateTo: "en",
  hideOnBlur: false,
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
  return { ...DEFAULT_SETTINGS, ...saved };
}

export async function saveSettings(next: Settings): Promise<void> {
  const s = await getStore();
  await s.set("settings", next);
  await s.save();
}
