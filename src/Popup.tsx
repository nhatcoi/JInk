import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { readImage } from "@tauri-apps/plugin-clipboard-manager";
import {
  ArrowLeftRight,
  BookOpen,
  CornerDownLeft,
  Loader2,
  Mic,
  Paperclip,
  Redo2,
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
import {
  enhancePrompt,
  explainPrompt,
  friendlyAiError,
  runAiStream,
  translatePrompt,
} from "@/lib/ai";
import { startRecording, transcribe } from "@/lib/voice";
import { formatAccelerator, matchesAccelerator } from "@/lib/shortcuts";
import { useUndo } from "@/lib/useUndo";

type Attachment = {
  id: string;
  name: string;
  kind: "image" | "file";
  url?: string;
  /** Absolute filesystem path (present when picked via the native dialog). */
  path?: string;
  /** For images: the N in its `[#ImageN]` text token, placing it inline. */
  index?: number;
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
  // Whether `status` is an AI-config error (bad base URL/key/model) — shows a
  // link into Settings instead of just the raw message.
  const [statusIsAiError, setStatusIsAiError] = useState(false);

  const taRef = useRef<HTMLTextAreaElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const recorderRef = useRef<Awaited<ReturnType<typeof startRecording>> | null>(
    null,
  );
  const cancelRef = useRef<(() => void) | null>(null);
  // Text from before the running AI stream replaced it.
  const streamBaseRef = useRef("");
  // True while our own file dialog is open — its blur shouldn't hide us.
  const suppressBlurRef = useRef(false);
  // Monotonic image counter for `[#ImageN]` tokens; reset per compose.
  const imageSeqRef = useRef(0);

  const history = useUndo(text, setText, taRef);

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
      setStatusIsAiError(false);
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
    setStatusIsAiError(false);
    imageSeqRef.current = 0;
    history.clear("");
  }, [history]);

  const insert = useCallback(async () => {
    // Images ride inline via their `[#ImageN]` tokens; non-image files append
    // their paths as text lines.
    const files = attachments
      .filter((a) => a.kind === "file" && a.path)
      .map((a) => a.path as string);
    let t = text;
    if (files.length) t = (t.trim() ? t + "\n" : "") + files.join("\n");
    const images = attachments
      .filter((a) => a.kind === "image" && a.path && a.index != null)
      .map((a) => ({ index: a.index as number, path: a.path as string }));
    if (!t.trim() && images.length === 0) return;
    try {
      await invoke("inject_text", { text: t, images });
      reset();
    } catch (e) {
      setStatusIsAiError(false);
      setStatus(String(e));
    }
  }, [text, attachments, reset]);

  // Insert an image's `[#ImageN]` token at the caret, keeping the caret after it.
  const insertToken = (token: string) => {
    const ta = taRef.current;
    const at = ta ? ta.selectionStart : text.length;
    setText((prev) => {
      const pos = Math.min(at, prev.length);
      return prev.slice(0, pos) + token + prev.slice(pos);
    });
    requestAnimationFrame(() => {
      if (!ta) return;
      const pos = at + token.length;
      ta.focus();
      ta.setSelectionRange(pos, pos);
    });
  };

  // --- AI (enhance / translate): stream tokens into the editor ---
  const streamInto = useCallback(
    async (messages: Parameters<typeof runAiStream>[1]) => {
      if (busy) {
        cancelRef.current?.();
        setBusy(false);
        history.resumeWith(streamBaseRef.current, text);
        return;
      }
      // if (!settings.aiKey) {
      //   setStatus("Set an API key in Settings first.");
      //   return;
      // }
      // Token-by-token rewrite is one undo step, not hundreds.
      const before = text;
      streamBaseRef.current = before;
      history.pause();
      setBusy(true);
      setStatus(null);
      setStatusIsAiError(false);
      let acc = "";
      setText("");
      cancelRef.current = await runAiStream(settings, messages, {
        onToken: (tk) => {
          acc += tk;
          setText(acc);
        },
        onDone: () => {
          setBusy(false);
          history.resumeWith(before, acc);
        },
        onError: (e) => {
          setBusy(false);
          setText(before);
          history.resumeWith(before, before);
          setStatus(friendlyAiError(e));
          setStatusIsAiError(true);
        },
      });
    },
    [busy, settings, text, history],
  );

  const enhance = () => text.trim() && streamInto(enhancePrompt(text.trim()));
  const explain = () => text.trim() && streamInto(explainPrompt(text.trim()));
  const translate = () =>
    text.trim() &&
    streamInto(
      translatePrompt(text.trim(), settings.translateFrom, settings.translateTo),
    );

  const swapLang = () => {
    // "auto" only makes sense as a source — swapping it into "to" is meaningless.
    if (settings.translateFrom === "auto") return;
    setSettings((s) => ({
      ...s,
      translateFrom: s.translateTo,
      translateTo: s.translateFrom,
    }));
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
        setStatus(friendlyAiError(String(e)));
        setStatusIsAiError(true);
      } finally {
        setBusy(false);
      }
      return;
    }
    try {
      recorderRef.current = await startRecording(settings.micDeviceId || undefined);
      setRecording(true);
      setStatus(null);
      setStatusIsAiError(false);
    } catch (e) {
      setStatusIsAiError(false);
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
      for (const p of paths) {
        if (isImagePath(p)) {
          const index = ++imageSeqRef.current;
          setAttachments((a) => [
            ...a,
            { id: crypto.randomUUID(), name: basename(p), kind: "image", path: p, index },
          ]);
          insertToken(`[#Image${index}]`);
        } else {
          setAttachments((a) => [
            ...a,
            { id: crypto.randomUUID(), name: basename(p), kind: "file", path: p },
          ]);
        }
      }
    } catch (e) {
      setStatusIsAiError(false);
      setStatus(String(e));
    } finally {
      suppressBlurRef.current = false;
      setTimeout(focusEditor, 0); // refocus editor after the dialog closes
    }
  };

  // Add an image attachment with a preview, then persist its bytes to a temp
  // file so `insert` can hand the target app a real path (not just a preview).
  const attachImage = async (bytes: Uint8Array, previewUrl: string, name: string) => {
    const id = crypto.randomUUID();
    const index = ++imageSeqRef.current;
    setAttachments((a) => [...a, { id, name, kind: "image", url: previewUrl, index }]);
    insertToken(`[#Image${index}]`);
    try {
      const path = await invoke<string>("save_temp_image", {
        bytes: Array.from(bytes),
        ext: "png",
      });
      setAttachments((a) => a.map((x) => (x.id === id ? { ...x, path } : x)));
    } catch (e) {
      setStatusIsAiError(false);
      setStatus(String(e));
    }
  };

  // Add pasted files as attachments; images get a preview + temp path.
  const addFiles = (files: File[]) => {
    for (const f of files) {
      if (f.type.startsWith("image/")) {
        f.arrayBuffer().then((buf) =>
          attachImage(new Uint8Array(buf), URL.createObjectURL(f), f.name),
        );
      } else {
        setAttachments((a) => [
          ...a,
          { id: crypto.randomUUID(), name: f.name, kind: "file" },
        ]);
      }
    }
  };

  // WebKitGTK often omits clipboard images from the JS paste event — read the
  // OS clipboard directly and add it as a data-URL attachment.
  const pasteNativeImage = async () => {
    try {
      const img = await readImage();
      const { width, height } = await img.size();
      const rgba = await img.rgba();
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.putImageData(
        new ImageData(new Uint8ClampedArray(rgba), width, height),
        0,
        0,
      );
      const blob = await new Promise<Blob | null>((res) =>
        canvas.toBlob(res, "image/png"),
      );
      if (!blob) return;
      const bytes = new Uint8Array(await blob.arrayBuffer());
      attachImage(bytes, URL.createObjectURL(blob), "pasted-image.png");
    } catch {
      // no image on the clipboard — nothing to paste
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
      return;
    }
    if (!e.clipboardData.getData("text/plain")) {
      pasteNativeImage();
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    // Shift+Enter is always a newline — never overridable by a bound shortcut.
    if (e.code === "Enter" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
      return;
    }
    const sc = settings.shortcuts;
    if (matchesAccelerator(e, sc.close)) {
      e.preventDefault();
      hide();
    } else if (matchesAccelerator(e, sc.insert)) {
      e.preventDefault();
      insert();
    } else if (matchesAccelerator(e, sc.attachFile)) {
      e.preventDefault();
      pickFiles();
    } else if (matchesAccelerator(e, sc.enhance)) {
      e.preventDefault();
      enhance();
    } else if (matchesAccelerator(e, sc.translate)) {
      e.preventDefault();
      translate();
    } else if (matchesAccelerator(e, sc.explain)) {
      e.preventDefault();
      explain();
    } else if (matchesAccelerator(e, sc.voice)) {
      e.preventDefault();
      toggleVoice();
    } else if (matchesAccelerator(e, sc.undo)) {
      // Own stack — native undo must not also fire.
      e.preventDefault();
      history.undo();
    } else if (matchesAccelerator(e, sc.redo)) {
      e.preventDefault();
      history.redo();
    } else if (matchesAccelerator(e, sc.openSettings)) {
      e.preventDefault();
      invoke("open_settings");
    }
  };

  const langLabel = (c: string) => c.toUpperCase();

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
            JInk
          </span>
          <div className="flex items-center gap-0.5">
            <IconButton
              label={`Settings (${formatAccelerator(settings.shortcuts.openSettings)})`}
              className="h-6 w-6"
              onClick={() => invoke("open_settings")}
            >
              <Settings2 size={14} />
            </IconButton>
            <IconButton
              label={`Close (${formatAccelerator(settings.shortcuts.close)})`}
              className="h-6 w-6"
              onClick={hide}
            >
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
          placeholder={`Type here…  ${formatAccelerator(settings.shortcuts.insert)} to insert · Shift+Enter for newline · ${formatAccelerator(settings.shortcuts.close)} to close`}
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
                <span className="max-w-[120px] truncate">
                  {a.kind === "image" && a.index != null ? `#Image${a.index}` : a.name}
                </span>
                <button
                  onClick={() => {
                    if (a.kind === "image" && a.index != null) {
                      setText((t) => t.replace(`[#Image${a.index}]`, ""));
                    }
                    setAttachments((x) => x.filter((y) => y.id !== a.id));
                  }}
                  className="text-muted hover:text-fg"
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </div>
        )}

        {status && (
          <div className="flex items-center gap-1.5 px-3.5 pb-1 text-[11px] text-red-500">
            <span>{status}</span>
            {statusIsAiError && (
              <button
                onClick={() => invoke("open_settings")}
                className="underline underline-offset-2 hover:text-red-400"
              >
                Open settings
              </button>
            )}
          </div>
        )}

        {/* toolbar */}
        <div className="flex items-center justify-between border-t border-border px-2 py-1.5">
          <div className="flex items-center gap-0.5">
            <IconButton
              label={`Attach file / image (${formatAccelerator(settings.shortcuts.attachFile)})`}
              onClick={pickFiles}
            >
              <Paperclip size={16} />
            </IconButton>
            <IconButton
              label={`Enhance & fix grammar (AI) (${formatAccelerator(settings.shortcuts.enhance)})`}
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
              title={`Translate (${formatAccelerator(settings.shortcuts.translate)})`}
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
              label={`Explain (AI) (${formatAccelerator(settings.shortcuts.explain)})`}
              onClick={explain}
              disabled={!text.trim() && !busy}
            >
              <BookOpen size={16} />
            </IconButton>
            <IconButton
              label={`${recording ? "Stop recording" : "Voice to text"} (${formatAccelerator(settings.shortcuts.voice)})`}
              onClick={toggleVoice}
              active={recording}
              className={recording ? "text-red-500" : ""}
            >
              <Mic size={16} />
            </IconButton>
            <IconButton
              label={`Undo (${formatAccelerator(settings.shortcuts.undo)})`}
              onClick={history.undo}
              disabled={!history.canUndo}
            >
              <Undo2 size={16} />
            </IconButton>
            <IconButton
              label={`Redo (${formatAccelerator(settings.shortcuts.redo)})`}
              onClick={history.redo}
              disabled={!history.canRedo}
            >
              <Redo2 size={16} />
            </IconButton>
          </div>

          <button
            type="button"
            onClick={insert}
            disabled={!text.trim()}
            title={`Insert (${formatAccelerator(settings.shortcuts.insert)})`}
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
