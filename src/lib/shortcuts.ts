// Accelerator strings use the same "Mod+Mod+Code" format the Rust side's
// tauri-plugin-global-shortcut parses (e.g. "CmdOrCtrl+Shift+KeyE"). The key
// segment is `KeyboardEvent.code` verbatim — it already matches the
// `keyboard-types::Code` names the Rust global-hotkey crate expects, so no
// translation table is needed between JS and Rust.

export type ActionId =
  | "insert"
  | "close"
  | "attachFile"
  | "enhance"
  | "translate"
  | "explain"
  | "voice"
  | "clear"
  | "undo"
  | "redo"
  | "openSettings";

export const ACTION_ORDER: ActionId[] = [
  "insert",
  "close",
  "attachFile",
  "enhance",
  "translate",
  "explain",
  "voice",
  "clear",
  "undo",
  "redo",
  "openSettings",
];

export const ACTION_LABELS: Record<ActionId, string> = {
  insert: "Insert text",
  close: "Close popup",
  attachFile: "Attach file / image",
  enhance: "AI enhance",
  translate: "Translate",
  explain: "AI explain",
  voice: "Voice to text",
  clear: "Clear all",
  undo: "Undo",
  redo: "Redo",
  openSettings: "Open settings",
};

export const DEFAULT_SHORTCUTS: Record<ActionId, string> = {
  insert: "Enter",
  close: "Escape",
  attachFile: "CmdOrCtrl+KeyO",
  enhance: "CmdOrCtrl+KeyE",
  translate: "CmdOrCtrl+KeyT",
  explain: "CmdOrCtrl+KeyD",
  voice: "CmdOrCtrl+KeyM",
  clear: "CmdOrCtrl+Backspace",
  undo: "CmdOrCtrl+KeyZ",
  redo: "CmdOrCtrl+Shift+KeyZ",
  openSettings: "CmdOrCtrl+Backquote",
};

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "CapsLock",
]);

// Keys that don't produce text, safe to bind on their own (no modifier
// needed). Anything else — letters, digits, punctuation — must be paired
// with a modifier, or the shortcut would fire on every keystroke while
// typing in the editor.
const SAFE_STANDALONE =
  /^(Escape|Enter|Tab|Space|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Arrow(Up|Down|Left|Right)|F([1-9]|1\d|2[0-4]))$/;

/**
 * Build an accelerator string from a raw keydown event, or null while only
 * modifier keys are held (caller should keep listening).
 */
export function acceleratorFromEvent(
  e: KeyboardEvent | React.KeyboardEvent,
): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const mods: string[] = [];
  if (e.ctrlKey || e.metaKey) mods.push("CmdOrCtrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  return [...mods, e.code].join("+");
}

// Reserved for the textarea's own newline — never assignable to an action.
const RESERVED = new Set(["Shift+Enter"]);

/**
 * Returns why `accel` can't be bound, or null if it's fine: bare printable
 * keys (letters/digits/punctuation) need a modifier, and a couple of combos
 * are reserved for the editor itself.
 */
export function accelIssue(accel: string): string | null {
  if (RESERVED.has(accel)) return "Reserved for inserting a newline.";
  const parts = accel.split("+");
  const key = parts[parts.length - 1];
  const hasModifier = parts.length > 1;
  if (!hasModifier && !SAFE_STANDALONE.test(key)) {
    return "Add Ctrl/Alt/Shift — a bare key would fire while typing.";
  }
  return null;
}

/** True if `e` is exactly the combo `accel` describes (no extra modifiers). */
export function matchesAccelerator(
  e: KeyboardEvent | React.KeyboardEvent,
  accel: string,
): boolean {
  if (!accel) return false;
  const parts = accel.split("+");
  const key = parts[parts.length - 1];
  const wantCtrl = parts.includes("CmdOrCtrl");
  const wantAlt = parts.includes("Alt");
  const wantShift = parts.includes("Shift");
  const hasCtrl = e.ctrlKey || e.metaKey;
  return (
    e.code === key &&
    hasCtrl === wantCtrl &&
    e.altKey === wantAlt &&
    e.shiftKey === wantShift
  );
}

function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
}

function formatKeyCode(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return "Num" + code.slice(6);
  if (code.startsWith("Arrow")) return code.slice(5);
  return code; // "Space", "Enter", "Escape", "F1", "Comma", …
}

/** Human-readable label, e.g. "CmdOrCtrl+Shift+KeyE" -> "Ctrl+Shift+E". */
export function formatAccelerator(accel: string): string {
  if (!accel) return "Not set";
  return accel
    .split("+")
    .map((part) => {
      if (part === "CmdOrCtrl") return isMac() ? "Cmd" : "Ctrl";
      if (part === "Alt") return isMac() ? "Option" : "Alt";
      if (part === "Shift") return "Shift";
      return formatKeyCode(part);
    })
    .join("+");
}
