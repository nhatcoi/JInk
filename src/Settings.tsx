import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check } from "lucide-react";
import {
  DEFAULT_SETTINGS,
  loadSettings,
  saveSettings,
  type Settings as S,
} from "@/lib/settings";
import { cn } from "@/lib/utils";
import { Combobox } from "@/components/Combobox";

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-sm font-medium text-fg">{label}</span>
      {children}
      {hint && <span className="text-xs text-muted">{hint}</span>}
    </label>
  );
}

const inputCls =
  "h-9 w-full rounded-lg border border-border bg-input px-3 text-sm text-fg outline-none focus:ring-2 focus:ring-ring";

export default function Settings() {
  const [s, setS] = useState<S>(DEFAULT_SETTINGS);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    loadSettings().then(setS);
  }, []);

  const set = <K extends keyof S>(k: K, v: S[K]) =>
    setS((prev) => ({ ...prev, [k]: v }));

  const save = async () => {
    await saveSettings(s);
    await invoke("set_hotkey", { accelerator: s.hotkey }).catch(() => {});
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  return (
    <div className="h-screen overflow-y-auto bg-bg text-fg">
      <div className="mx-auto max-w-xl space-y-6 p-6">
        <div>
          <h1 className="text-lg font-semibold">Settings</h1>
          <p className="text-sm text-muted">Configure AI, hotkey and popup.</p>
        </div>

        <section className="space-y-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-muted">
            AI (OpenAI-compatible)
          </h2>
          <Field label="Base URL" hint="e.g. https://api.openai.com/v1 or your local endpoint">
            <input
              className={inputCls}
              value={s.aiBaseUrl}
              onChange={(e) => set("aiBaseUrl", e.target.value)}
            />
          </Field>
          <Field label="Model">
            <input
              className={inputCls}
              value={s.aiModel}
              onChange={(e) => set("aiModel", e.target.value)}
            />
          </Field>
          <Field label="API Key" hint="Stored locally in settings.json">
            <input
              type="password"
              className={inputCls}
              value={s.aiKey}
              onChange={(e) => set("aiKey", e.target.value)}
            />
          </Field>
        </section>

        <section className="space-y-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-muted">
            Shortcut & Popup
          </h2>
          <Field label="Global hotkey" hint="e.g. Alt+Space, CmdOrCtrl+Shift+Space">
            <input
              className={inputCls}
              value={s.hotkey}
              onChange={(e) => set("hotkey", e.target.value)}
            />
          </Field>
          <Field label="Submit key">
            <Combobox
              value={s.submitKey}
              onChange={(v) => set("submitKey", v as S["submitKey"])}
              options={[
                { value: "enter", label: "Enter inserts (Shift+Enter = newline)" },
                { value: "mod-enter", label: "Ctrl/Cmd+Enter inserts" },
              ]}
            />
          </Field>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={s.hideOnBlur}
              onChange={(e) => set("hideOnBlur", e.target.checked)}
              className="h-4 w-4 rounded border-border accent-primary"
            />
            <span className="text-sm font-medium text-fg">
              Click outside to hide popup
            </span>
          </label>
          <div className="grid grid-cols-2 gap-4">
            <Field label="Width (px)">
              <input
                type="number"
                className={inputCls}
                value={s.width}
                onChange={(e) => set("width", Number(e.target.value))}
              />
            </Field>
            <Field label="Min height (px)">
              <input
                type="number"
                className={inputCls}
                value={s.height}
                onChange={(e) => set("height", Number(e.target.value))}
              />
            </Field>
          </div>
        </section>

        <section className="space-y-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-muted">
            Translate default
          </h2>
          <div className="grid grid-cols-2 gap-4">
            <Field label="From">
              <Combobox
                value={s.translateFrom}
                onChange={(v) => set("translateFrom", v as "vi" | "en")}
                options={[
                  { value: "vi", label: "Vietnamese" },
                  { value: "en", label: "English" },
                ]}
              />
            </Field>
            <Field label="To">
              <Combobox
                value={s.translateTo}
                onChange={(v) => set("translateTo", v as "vi" | "en")}
                options={[
                  { value: "en", label: "English" },
                  { value: "vi", label: "Vietnamese" },
                ]}
              />
            </Field>
          </div>
        </section>

        <div className="flex items-center gap-3 pt-2">
          <button
            onClick={save}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-lg bg-primary px-4 py-2 text-sm font-semibold text-primary-fg hover:opacity-90",
            )}
          >
            {saved ? <Check size={15} /> : null}
            {saved ? "Saved" : "Save"}
          </button>
          <button
            onClick={() => getCurrentWindow().hide()}
            className="rounded-lg px-4 py-2 text-sm text-muted hover:bg-accent"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
