import { useCallback, useEffect, useReducer, useRef } from "react";

// Own undo stack: programmatic writes (AI stream, voice) wipe the webview's.

type Snap = { text: string; start: number; end: number };

/** Edits closer together than this collapse into one entry. */
const COALESCE_MS = 400;

export function useUndo(
  text: string,
  setText: (t: string) => void,
  taRef: React.RefObject<HTMLTextAreaElement | null>,
) {
  const past = useRef<Snap[]>([]);
  const future = useRef<Snap[]>([]);
  const last = useRef<Snap>({ text, start: text.length, end: text.length });
  const lastAt = useRef(0);
  // Our own write of `text` — not a new edit.
  const applying = useRef(false);
  const paused = useRef(false);
  const [, bump] = useReducer((n: number) => n + 1, 0);

  const snap = useCallback(
    (t: string): Snap => {
      const ta = taRef.current;
      return {
        text: t,
        start: ta?.selectionStart ?? t.length,
        end: ta?.selectionEnd ?? t.length,
      };
    },
    [taRef],
  );

  useEffect(() => {
    if (applying.current) {
      applying.current = false;
      return;
    }
    if (paused.current) return;
    if (text === last.current.text) return;
    const now = Date.now();
    if (now - lastAt.current > COALESCE_MS) past.current.push(last.current);
    future.current = [];
    last.current = snap(text);
    lastAt.current = now;
    bump();
  }, [text, snap]);

  const restore = useCallback(
    (s: Snap) => {
      applying.current = true;
      last.current = s;
      lastAt.current = 0;
      setText(s.text);
      requestAnimationFrame(() => {
        taRef.current?.focus();
        taRef.current?.setSelectionRange(s.start, s.end);
      });
      bump();
    },
    [setText, taRef],
  );

  const undo = useCallback(() => {
    const prev = past.current.pop();
    if (!prev) return;
    future.current.push(last.current);
    restore(prev);
  }, [restore]);

  const redo = useCallback(() => {
    const next = future.current.pop();
    if (!next) return;
    past.current.push(last.current);
    restore(next);
  }, [restore]);

  const pause = useCallback(() => {
    paused.current = true;
  }, []);

  /** Resume, filing the paused rewrite as one entry back to `baseline`. */
  const resumeWith = useCallback(
    (baseline: string, current: string) => {
      // Cancel resumes early; a late onDone must not file it twice.
      if (!paused.current) return;
      paused.current = false;
      lastAt.current = 0;
      if (baseline !== current) {
        past.current.push({
          text: baseline,
          start: baseline.length,
          end: baseline.length,
        });
        future.current = [];
      }
      last.current = { text: current, start: current.length, end: current.length };
      bump();
    },
    [],
  );

  const clear = useCallback((t: string) => {
    past.current = [];
    future.current = [];
    last.current = { text: t, start: t.length, end: t.length };
    lastAt.current = 0;
    applying.current = true;
    bump();
  }, []);

  return {
    undo,
    redo,
    canUndo: past.current.length > 0,
    canRedo: future.current.length > 0,
    pause,
    resumeWith,
    clear,
  };
}
