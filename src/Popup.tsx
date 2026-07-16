import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeftRight,
  CornerDownLeft,
  Loader2,
  Mic,
  Paperclip,
  Settings2,
  Sparkles,
  Undo2,
  X,
} from "lucide-react";
import { IconButton } from "@/components/IconButton";
import { cn } from "@/lib/utils";
import {
  DEFAULT_SETTINGS,
  loadSettings,
  type Settings,
} from "@/lib/settings";
import { enhancePrompt, runAiStream, translatePrompt } from "@/lib/ai";
import { startRecording, transcribe } from "@/lib/voice";

type Attachment = {
  id: string;
  name: string;
  kind: "image" | "file";
  url?: string;
  /** Absolute filesystem path (present when picked via the native dialog). */
  path?: string;
};

const basename = (p: string) => p.split(/[\\/]/).pop() || p;
const isImagePath = (p: string) =>
  /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i.test(p);

const MAX_HEIGHT = 460;
const TEXTAREA_MAX = 320;

export default function Popup() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [busy, setBusy] = useState(false);
  const [recording, setRecording] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [undoText, setUndoText] = useState<string | null>(null);

  const taRef = useRef<HTMLTextAreaElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const recorderRef = useRef<Awaited<ReturnType<typeof startRecording>> | null>(
    null,
  );
  const cancelRef = useRef<(() => void) | null>(null);
  // True while our own file dialog is open — its blur shouldn't hide us.
  const suppressBlurRef = useRef(false);

  // Load settings and register the hotkey.
  useEffect(() => {
    (async () => {
      const s = await loadSettings();
      setSettings(s);
      invoke("set_hotkey", { accelerator: s.hotkey }).catch(() => {});
    })();
  }, []);

  // Focus the editor with the caret at the end.
  const focusEditor = useCallback(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.focus();
    const end = ta.value.length;
    ta.setSelectionRange(end, end);
  }, []);

  // Grow the textarea to fit content, up to a cap (then it scrolls).
  const autosize = useCallback(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, TEXTAREA_MAX) + "px";
  }, []);

  // On show, reload settings (Settings window edits them out-of-process) and
  // focus the editor — retry to beat the WM focus race.
  useEffect(() => {
    const un = listen("popup-shown", () => {
      loadSettings().then(setSettings);
      setStatus(null);
      autosize();
      [0, 60, 150, 250].forEach((d) => setTimeout(focusEditor, d));
    });
    focusEditor();
    return () => {
      un.then((f) => f());
    };
  }, [focusEditor, autosize]);

  useEffect(() => {
    autosize();
  }, [text, autosize]);

  // Resize the window to fit content.
  useEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const h = Math.min(el.scrollHeight, MAX_HEIGHT);
    getCurrentWindow()
      .setSize(new LogicalSize(settings.width, Math.max(settings.height, h)))
      .catch(() => {});
  }, [text, attachments, status, settings.width, settings.height]);

  const hide = useCallback(() => invoke("hide_window").catch(() => {}), []);

  // Optional: hide when the popup loses focus (click outside). Skipped for
  // our own file dialog and for helper apps (clipboard/file managers) —
  // those drop below the popup instead of closing it.
  useEffect(() => {
    if (!settings.hideOnBlur) return;
    const win = getCurrentWindow();
    const un = win.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        win.setAlwaysOnTop(true).catch(() => {});
        return;
      }
      if (suppressBlurRef.current) return;
      invoke<boolean>("focused_window_is_helper").then((isHelper) => {
        if (isHelper) win.setAlwaysOnTop(false).catch(() => {});
        else hide();
      });
    });
    return () => {
      un.then((f) => f());
    };
  }, [settings.hideOnBlur, hide]);

  const reset = useCallback(() => {
    setText("");
    setAttachments([]);
    setStatus(null);
    setUndoText(null);
  }, []);

  const insert = useCallback(async () => {
    // Append picked file paths so the target app receives them.
    const paths = attachments.map((a) => a.path).filter(Boolean) as string[];
    let t = text.trim();
    if (paths.length) t = (t ? t + "\n" : "") + paths.join("\n");
    if (!t) return;
    try {
      await invoke("inject_text", { text: t });
      reset();
    } catch (e) {
      setStatus(String(e));
    }
  }, [text, attachments, reset]);

  // --- AI (enhance / translate): stream tokens into the editor ---
  const streamInto = useCallback(
    async (messages: Parameters<typeof runAiStream>[1]) => {
      if (busy) {
        cancelRef.current?.();
        setBusy(false);
        return;
      }
      if (!settings.aiKey) {
        setStatus("Set an API key in Settings first.");
        return;
      }
      setUndoText(text);
      setBusy(true);
      setStatus(null);
      let acc = "";
      setText("");
      cancelRef.current = await runAiStream(settings, messages, {
        onToken: (tk) => {
          acc += tk;
          setText(acc);
        },
        onDone: () => setBusy(false),
        onError: (e) => {
          setBusy(false);
          setText(undoText ?? text);
          setStatus(e);
        },
      });
    },
    [busy, settings, text, undoText],
  );

  const enhance = () => text.trim() && streamInto(enhancePrompt(text.trim()));
  const translate = () =>
    text.trim() &&
    streamInto(
      translatePrompt(text.trim(), settings.translateFrom, settings.translateTo),
    );

  const swapLang = () =>
    setSettings((s) => ({
      ...s,
      translateFrom: s.translateTo,
      translateTo: s.translateFrom,
    }));

  const undo = () => {
    if (undoText !== null) {
      setText(undoText);
      setUndoText(null);
    }
  };

  // --- Voice ---
  const toggleVoice = async () => {
    if (recording) {
      const rec = recorderRef.current;
      recorderRef.current = null;
      setRecording(false);
      if (!rec) return;
      setBusy(true);
      try {
        const blob = await rec.stop();
        const t = await transcribe(blob, settings);
        setText((prev) => (prev ? prev + " " : "") + t);
      } catch (e) {
        setStatus(String(e));
      } finally {
        setBusy(false);
      }
      return;
    }
    try {
      recorderRef.current = await startRecording();
      setRecording(true);
      setStatus(null);
    } catch (e) {
      setStatus("Microphone unavailable: " + String(e));
    }
  };

  // --- Attachments ---
  // Native picker returns absolute paths (unlike <input type=file>).
  const pickFiles = async () => {
    suppressBlurRef.current = true;
    try {
      const sel = await openDialog({ multiple: true });
      const paths = sel == null ? [] : Array.isArray(sel) ? sel : [sel];
      if (paths.length) {
        setAttachments((a) => [
          ...a,
          ...paths.map((p) => ({
            id: crypto.randomUUID(),
            name: basename(p),
            kind: isImagePath(p) ? ("image" as const) : ("file" as const),
            path: p,
          })),
        ]);
      }
    } catch (e) {
      setStatus(String(e));
    } finally {
      suppressBlurRef.current = false;
      setTimeout(focusEditor, 0); // refocus editor after the dialog closes
    }
  };

  // Add pasted files as attachments; images get a data-URL preview.
  const addFiles = (files: File[]) => {
    for (const f of files) {
      const isImg = f.type.startsWith("image/");
      const att: Attachment = {
        id: crypto.randomUUID(),
        name: f.name,
        kind: isImg ? "image" : "file",
      };
      if (isImg) {
        const r = new FileReader();
        r.onload = () =>
          setAttachments((a) =>
            a.map((x) => (x.id === att.id ? { ...x, url: String(r.result) } : x)),
          );
        r.readAsDataURL(f);
      }
      setAttachments((a) => [...a, att]);
    }
  };

  const onPaste = (e: React.ClipboardEvent) => {
    const imgs = Array.from(e.clipboardData.items)
      .filter((i) => i.type.startsWith("image/"))
      .map((i) => i.getAsFile())
      .filter((f): f is File => f !== null);
    if (imgs.length) {
      e.preventDefault();
      addFiles(imgs);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      hide();
      return;
    }
    const mod = e.metaKey || e.ctrlKey;
    const submit =
      settings.submitKey === "enter"
        ? e.key === "Enter" && !e.shiftKey && !mod
        : e.key === "Enter" && mod;
    if (submit) {
      e.preventDefault();
      insert();
    }
  };

  const langLabel = (c: string) => (c === "vi" ? "VI" : "EN");

  return (
    <div className="flex h-screen w-screen items-start justify-center p-1.5">
      <div
        ref={cardRef}
        className="animate-pop flex w-full flex-col overflow-hidden rounded-2xl border border-border bg-surface shadow-2xl shadow-black/20"
      >
        {/* drag handle / header */}
        <div
          data-tauri-drag-region
          className="flex items-center justify-between px-3 py-1.5 select-none"
        >
          <span
            data-tauri-drag-region
            className="text-[11px] font-medium tracking-wide text-muted"
          >
            easyinput
          </span>
          <div className="flex items-center gap-0.5">
            <IconButton
              label="Settings"
              className="h-6 w-6"
              onClick={() => invoke("open_settings")}
            >
              <Settings2 size={14} />
            </IconButton>
            <IconButton label="Close (Esc)" className="h-6 w-6" onClick={hide}>
              <X size={14} />
            </IconButton>
          </div>
        </div>

        {/* editor */}
        <textarea
          ref={taRef}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          placeholder="Type here…  Enter to insert · Shift+Enter for newline · Esc to close"
          style={{ maxHeight: TEXTAREA_MAX }}
          className={cn(
            "min-h-[52px] w-full resize-none overflow-y-auto bg-transparent px-3.5 py-1 text-[15px] leading-relaxed",
            "text-fg placeholder:text-muted focus:outline-none",
          )}
          rows={1}
        />

        {/* attachments */}
        {attachments.length > 0 && (
          <div className="flex flex-wrap gap-2 px-3 pb-2">
            {attachments.map((a) => (
              <div
                key={a.id}
                className="group relative flex items-center gap-1.5 rounded-lg border border-border bg-accent px-2 py-1 text-xs text-fg"
              >
                {a.kind === "image" && a.url ? (
                  <img
                    src={a.url}
                    alt={a.name}
                    className="h-6 w-6 rounded object-cover"
                  />
                ) : (
                  <Paperclip size={12} />
                )}
                <span className="max-w-[120px] truncate">{a.name}</span>
                <button
                  onClick={() =>
                    setAttachments((x) => x.filter((y) => y.id !== a.id))
                  }
                  className="text-muted hover:text-fg"
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </div>
        )}

        {status && (
          <div className="px-3.5 pb-1 text-[11px] text-red-500">{status}</div>
        )}

        {/* toolbar */}
        <div className="flex items-center justify-between border-t border-border px-2 py-1.5">
          <div className="flex items-center gap-0.5">
            <IconButton label="Attach file / image" onClick={pickFiles}>
              <Paperclip size={16} />
            </IconButton>
            <IconButton
              label="Enhance & fix grammar (AI)"
              onClick={enhance}
              active={busy}
              disabled={!text.trim() && !busy}
            >
              {busy ? (
                <Loader2 size={16} className="animate-spin" />
              ) : (
                <Sparkles size={16} />
              )}
            </IconButton>
            <button
              type="button"
              onClick={translate}
              disabled={!text.trim()}
              title="Translate"
              className="inline-flex h-8 items-center gap-1 rounded-lg px-2 text-xs font-medium text-muted transition-colors hover:bg-accent hover:text-fg disabled:opacity-40"
            >
              {langLabel(settings.translateFrom)}
              <ArrowLeftRight
                size={13}
                onClick={(e) => {
                  e.stopPropagation();
                  swapLang();
                }}
                className="cursor-pointer hover:text-fg"
              />
              {langLabel(settings.translateTo)}
            </button>
            <IconButton
              label={recording ? "Stop recording" : "Voice to text"}
              onClick={toggleVoice}
              active={recording}
              className={recording ? "text-red-500" : ""}
            >
              <Mic size={16} />
            </IconButton>
            {undoText !== null && (
              <IconButton label="Undo AI change" onClick={undo}>
                <Undo2 size={16} />
              </IconButton>
            )}
          </div>

          <button
            type="button"
            onClick={insert}
            disabled={!text.trim()}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-lg bg-primary px-3 py-1.5 text-xs font-semibold text-primary-fg transition",
              "hover:opacity-90 disabled:opacity-40",
            )}
          >
            Insert
            <CornerDownLeft size={13} />
          </button>
        </div>

      </div>
    </div>
  );
}
