import { Fragment, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { appDataDir, join } from "@tauri-apps/api/path";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openPath } from "@tauri-apps/plugin-opener";
import { Check, FileJson, RefreshCw, RotateCcw, Undo2 } from "lucide-react";
import {
  DEFAULT_SETTINGS,
  loadSettings,
  saveSettings,
  type Settings as S,
} from "@/lib/settings";
import { ACTION_LABELS, ACTION_ORDER } from "@/lib/shortcuts";
import { AUTO_DETECT, LANGUAGES } from "@/lib/languages";
import { cn } from "@/lib/utils";
import { Combobox } from "@/components/Combobox";
import { KeyCapture } from "@/components/KeyCapture";
import { MicTest } from "@/components/MicTest";
import { Switch } from "@/components/Switch";

type LocalProvider = { name: string; base_url: string; models: string[]; running: boolean };

const localOptionValue = (baseUrl: string, model: string) => `${baseUrl}|||${model}`;

// Small, multilingual-friendly Ollama models worth suggesting when nothing
// local is installed yet — not exhaustive, just a sane starting point.
const RECOMMENDED_MODELS = [
  { name: "qwen2.5:7b", note: "best multilingual incl. Vietnamese, good default" },
  { name: "llama3.2:3b", note: "fast & light, weaker at Vietnamese" },
  { name: "gemma2:9b", note: "strong quality, needs more RAM" },
];

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
  // Last-saved snapshot — lets "Undo changes" revert edits made since open/save.
  const [savedSnapshot, setSavedSnapshot] = useState<S>(DEFAULT_SETTINGS);
  const [saved, setSaved] = useState(false);
  const [localProviders, setLocalProviders] = useState<LocalProvider[]>([]);
  const [scanning, setScanning] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [starting, setStarting] = useState<string | null>(null);
  const [startError, setStartError] = useState<string | null>(null);

  const set = <K extends keyof S>(k: K, v: S[K]) =>
    setS((prev) => ({ ...prev, [k]: v }));

  // `checkAgainst` avoids reading stale `s` from a closure captured before
  // `loadSettings()` resolves (the mount-time call needs the freshly loaded
  // settings, not the initial-render default).
  const scanLocal = async (autoFillIfUnconfigured: boolean, checkAgainst: S = s) => {
    setScanning(true);
    try {
      const found = await invoke<LocalProvider[]>("detect_local_ai");
      setLocalProviders(found);
      setScanned(true);
      const running = found.find((p) => p.running);
      const first = running?.models[0];
      // Only auto-pick when the user hasn't set up anything yet (still on
      // the default remote URL, no key) — never clobber an existing config.
      const unconfigured =
        checkAgainst.aiBaseUrl === DEFAULT_SETTINGS.aiBaseUrl && checkAgainst.aiKey === "";
      if (autoFillIfUnconfigured && unconfigured && running && first) {
        setS((prev) => ({
          ...prev,
          aiBaseUrl: running.base_url,
          aiModel: first,
          aiKey: "",
        }));
      }
    } finally {
      setScanning(false);
    }
  };

  const startLocal = async (name: string) => {
    setStarting(name);
    setStartError(null);
    try {
      await invoke<string>("start_local_ai", { name });
      await scanLocal(false);
    } catch (e) {
      setStartError(String(e));
    } finally {
      setStarting(null);
    }
  };

  // Silent scan once on open — only applies a pick if nothing is configured yet.
  useEffect(() => {
    loadSettings().then((loaded) => {
      setS(loaded);
      setSavedSnapshot(loaded);
      scanLocal(true, loaded);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Close only hides this window, so a re-open lands on a stale form. Local AI
  // isn't rescanned — that's the rescan button's job.
  useEffect(() => {
    const un = listen("settings-shown", () => {
      loadSettings().then((loaded) => {
        setS(loaded);
        setSavedSnapshot(loaded);
      });
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const save = async () => {
    await saveSettings(s);
    await invoke("set_hotkey", { accelerator: s.hotkey }).catch(() => {});
    setSavedSnapshot(s);
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  const resetDefaults = () => setS(DEFAULT_SETTINGS);
  const undoChanges = () => setS(savedSnapshot);
  const dirty = JSON.stringify(s) !== JSON.stringify(savedSnapshot);

  const openSettingsFile = async () => {
    const path = await join(await appDataDir(), "settings.json");
    await openPath(path);
  };

  // The global hotkey is OS-grabbed even when the popup is unfocused, so it
  // would swallow any in-popup shortcut bound to the same combo. Flag it.
  const entries: [string, string][] = [
    ["Global hotkey", s.hotkey],
    ...ACTION_ORDER.map((id): [string, string] => [ACTION_LABELS[id], s.shortcuts[id]]),
  ];
  const conflictsFor = (label: string, accel: string): string | null => {
    if (!accel) return null;
    const others = entries
      .filter(([l, a]) => l !== label && a === accel)
      .map(([l]) => l);
    return others.length ? `Same as ${others.join(", ")}` : null;
  };

  return (
    <div className="h-screen overflow-y-auto bg-bg text-fg">
      <div className="mx-auto max-w-xl space-y-6 p-6">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h1 className="text-lg font-semibold">Settings</h1>
            <p className="text-sm text-muted">Configure AI, hotkey and popup.</p>
          </div>
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={openSettingsFile}
              title="Open settings.json"
              className="rounded-lg p-2 text-muted hover:bg-accent hover:text-fg"
            >
              <FileJson size={16} />
            </button>
            <button
              type="button"
              onClick={undoChanges}
              disabled={!dirty}
              title="Undo changes (revert to last saved)"
              className="rounded-lg p-2 text-muted hover:bg-accent hover:text-fg disabled:opacity-40"
            >
              <Undo2 size={16} />
            </button>
            <button
              type="button"
              onClick={resetDefaults}
              title="Reset to defaults"
              className="rounded-lg p-2 text-muted hover:bg-accent hover:text-fg"
            >
              <RotateCcw size={16} />
            </button>
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

        <section className="space-y-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-muted">
            AI
          </h2>
          <Field label="Base URL" hint="e.g. https://api.openai.com/v1, or a local server's URL">
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
          <Field label="API Key" hint="Stored locally in settings.json — leave empty for local AI">
            <input
              type="password"
              className={inputCls}
              value={s.aiKey}
              onChange={(e) => set("aiKey", e.target.value)}
            />
          </Field>

          <div className="space-y-2 rounded-lg border border-border/60 p-3">
            <div className="flex items-center justify-between">
              <span className="text-xs text-muted">
                {scanning
                  ? "Scanning localhost for Ollama, LM Studio, llama.cpp…"
                  : localProviders.length > 0
                    ? `Local AI found: ${localProviders.map((p) => p.name).join(", ")}.`
                    : scanned
                      ? "No local AI found on this machine."
                      : ""}
              </span>
              <button
                type="button"
                onClick={() => scanLocal(false)}
                disabled={scanning}
                className="inline-flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs font-medium text-fg hover:bg-accent disabled:opacity-50"
              >
                <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
                Rescan local
              </button>
            </div>

            {localProviders.some((p) => !p.running) && (
              <div className="space-y-1.5">
                {localProviders
                  .filter((p) => !p.running)
                  .map((p) => (
                    <div key={p.name} className="flex items-center justify-between text-xs">
                      <span className="text-muted">
                        {p.name} — {p.models.length} model{p.models.length === 1 ? "" : "s"}{" "}
                        installed, not running
                      </span>
                      <button
                        type="button"
                        onClick={() => startLocal(p.name)}
                        disabled={starting === p.name}
                        className="rounded-lg border border-border px-2.5 py-1 text-xs font-medium text-fg hover:bg-accent disabled:opacity-50"
                      >
                        {starting === p.name ? "Starting…" : "Start"}
                      </button>
                    </div>
                  ))}
                {startError && <span className="block text-xs text-red-500">{startError}</span>}
              </div>
            )}

            {localProviders.some((p) => p.running) && (
              <Field label="Use a detected local model">
                <Combobox
                  value={localOptionValue(s.aiBaseUrl, s.aiModel)}
                  onChange={(v) => {
                    const [baseUrl, model] = v.split("|||");
                    set("aiBaseUrl", baseUrl);
                    set("aiModel", model);
                    set("aiKey", "");
                  }}
                  searchable
                  placeholder="Pick a local model…"
                  options={localProviders
                    .filter((p) => p.running)
                    .flatMap((p) =>
                      p.models.map((m) => ({
                        value: localOptionValue(p.base_url, m),
                        label: `${p.name} — ${m}`,
                      })),
                    )}
                />
              </Field>
            )}

            {scanned && localProviders.length === 0 && (
              <div className="space-y-1.5">
                <span className="text-xs text-muted">
                  Nothing installed. Install Ollama (ollama.com), then pull a model:
                </span>
                {RECOMMENDED_MODELS.map((m) => (
                  <div key={m.name} className="flex items-center justify-between gap-2 text-xs">
                    <span>
                      <code className="rounded bg-input px-1.5 py-0.5">
                        ollama pull {m.name}
                      </code>
                      <span className="ml-2 text-muted">{m.note}</span>
                    </span>
                    <button
                      type="button"
                      onClick={() => navigator.clipboard.writeText(`ollama pull ${m.name}`)}
                      className="shrink-0 text-muted hover:text-fg"
                    >
                      Copy
                    </button>
                  </div>
                ))}
              </div>
            )}
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
                onChange={(v) => set("translateFrom", v)}
                searchable
                options={[AUTO_DETECT, ...LANGUAGES].map((l) => ({
                  value: l.code,
                  label: l.label,
                }))}
              />
            </Field>
            <Field label="To">
              <Combobox
                value={s.translateTo}
                onChange={(v) => set("translateTo", v)}
                searchable
                options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
              />
            </Field>
          </div>
        </section>

        <section className="space-y-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-muted">
            Voice
          </h2>
          <MicTest deviceId={s.micDeviceId} onDeviceChange={(v) => set("micDeviceId", v)} />
        </section>

        <section className="space-y-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-muted">
            Shortcut & Popup
          </h2>
          <Field
            label="Global hotkey"
            hint="Click, then press a key combo. Works even when easyinput isn't focused."
          >
            <KeyCapture value={s.hotkey} onChange={(v) => set("hotkey", v)} />
            {conflictsFor("Global hotkey", s.hotkey) && (
              <span className="text-xs text-red-500">
                {conflictsFor("Global hotkey", s.hotkey)}
              </span>
            )}
          </Field>

          <div className="space-y-3 rounded-lg border border-border/60 p-3">
            <span className="text-xs font-medium text-muted">
              Popup shortcuts — only active while the popup is open
            </span>
            <div className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-2.5">
              {ACTION_ORDER.map((id) => {
                const conflict = conflictsFor(ACTION_LABELS[id], s.shortcuts[id]);
                return (
                  <Fragment key={id}>
                    <span className="text-sm font-medium text-fg">
                      {ACTION_LABELS[id]}
                    </span>
                    <div className="flex flex-col items-end gap-1">
                      <KeyCapture
                        value={s.shortcuts[id]}
                        onChange={(v) =>
                          set("shortcuts", { ...s.shortcuts, [id]: v })
                        }
                        className="w-48"
                      />
                      {conflict && (
                        <span className="text-xs text-red-500">{conflict}</span>
                      )}
                    </div>
                  </Fragment>
                );
              })}
            </div>
            <span className="block text-xs text-muted">
              Shift+Enter always inserts a newline, regardless of the Insert shortcut.
            </span>
          </div>

          <Switch
            checked={s.hideOnBlur}
            onChange={(v) => set("hideOnBlur", v)}
            label="Click outside to hide popup"
          />
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
      </div>
    </div>
  );
}
