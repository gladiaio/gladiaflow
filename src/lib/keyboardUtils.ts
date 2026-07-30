const MODIFIER_MAP: Record<string, string> = {
  MetaLeft: "Cmd",
  MetaRight: "Cmd",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
  ControlLeft: "Ctrl",
  ControlRight: "Ctrl",
  AltLeft: "Alt",
  AltRight: "Alt",
};

/** Stable logical names for non-printable keys from KeyboardEvent.key */
const LOGICAL_KEY_FROM_EVENT_KEY: Record<string, string> = {
  " ": "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Escape: "Escape",
};

const SPECIAL_KEY_MAP: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

const MODIFIER_SYMBOLS: Record<string, string> = {
  Cmd: "⌘",
  Shift: "⇧",
  Ctrl: "⌃",
  Alt: "⌥",
};

// Side-qualified single-modifier tokens, captured when the user binds a lone
// modifier (e.g. Right Command). Maps the browser KeyboardEvent.code → token,
// and each token → its base modifier for display/fallback.
const MODIFIER_CODE_TO_SIDE_TOKEN: Record<string, string> = {
  MetaLeft: "CmdLeft",
  MetaRight: "CmdRight",
  ControlLeft: "CtrlLeft",
  ControlRight: "CtrlRight",
  AltLeft: "AltLeft",
  AltRight: "AltRight",
  ShiftLeft: "ShiftLeft",
  ShiftRight: "ShiftRight",
};

const SIDE_TOKEN_TO_BASE: Record<string, string> = {
  CmdLeft: "Cmd",
  CmdRight: "Cmd",
  CtrlLeft: "Ctrl",
  CtrlRight: "Ctrl",
  AltLeft: "Alt",
  AltRight: "Alt",
  ShiftLeft: "Shift",
  ShiftRight: "Shift",
};

/** Returns the side-qualified token for a modifier KeyboardEvent.code, else null. */
export function modifierSideToken(code: string): string | null {
  return MODIFIER_CODE_TO_SIDE_TOKEN[code] ?? null;
}

/** True for a hotkey that is a single modifier key (lone, possibly side-qualified). */
export function isSingleModifierToken(token: string): boolean {
  return token === "Fn" || token in SIDE_TOKEN_TO_BASE;
}

const MODIFIER_SORT_ORDER = ["Ctrl", "Alt", "Shift", "Cmd"];

/** Modifier token from a physical KeyboardEvent.code (left/right collapsed). */
export function normalizeModifierFromCode(code: string): string | null {
  return MODIFIER_MAP[code] ?? null;
}

/**
 * Logical non-modifier key token from KeyboardEvent.key.
 * Returns null for modifiers, dead keys, IME composition, or unsupported keys.
 */
export function normalizeCapturedKey(event: KeyboardEvent): string | null {
  const { key } = event;

  if (key === "Dead" || key === "Process" || key === "Unidentified") {
    return null;
  }

  const named = LOGICAL_KEY_FROM_EVENT_KEY[key];
  if (named) return named;

  if (/^F\d{1,2}$/i.test(key)) {
    return key.toUpperCase();
  }

  if (key.length === 1) {
    if (/[a-zA-Z]/.test(key)) return key.toUpperCase();
    return key;
  }

  return null;
}

/** Compact debug string for a captured KeyboardEvent (physical code + layout key). */
export function formatKeyboardEventForLog(event: KeyboardEvent): string {
  const mods = [
    event.metaKey && "meta",
    event.ctrlKey && "ctrl",
    event.shiftKey && "shift",
    event.altKey && "alt",
  ]
    .filter(Boolean)
    .join("+");
  const modifierSuffix = mods ? ` modifiers=${mods}` : "";
  return `code=${event.code} key=${JSON.stringify(event.key)} location=${event.location}${modifierSuffix}`;
}

/** Modifier or logical key token for hotkey capture tracking. */
export function captureKeyToken(event: KeyboardEvent): string | null {
  const modifier = normalizeModifierFromCode(event.code);
  if (modifier) return modifier;
  return normalizeCapturedKey(event);
}

/** @deprecated Physical-code normalization; prefer captureKeyToken for hotkey capture. */
export function normalizeKey(code: string): string {
  if (MODIFIER_MAP[code]) return MODIFIER_MAP[code];
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (SPECIAL_KEY_MAP[code]) return SPECIAL_KEY_MAP[code];
  if (/^F\d{1,2}$/.test(code)) return code;
  return code;
}

export function isModifier(key: string): boolean {
  return ["Cmd", "Shift", "Ctrl", "Alt"].includes(key);
}

export function formatKeySymbol(key: string): string {
  const base = SIDE_TOKEN_TO_BASE[key];
  if (base) {
    const side = key.endsWith("Right") ? "R" : "L";
    return `${MODIFIER_SYMBOLS[base]}${side}`;
  }
  return MODIFIER_SYMBOLS[key] ?? key;
}

export function sortKeys(keys: string[]): string[] {
  const mods = keys
    .filter(isModifier)
    .sort(
      (a, b) => MODIFIER_SORT_ORDER.indexOf(a) - MODIFIER_SORT_ORDER.indexOf(b),
    );
  const others = keys.filter((k) => !isModifier(k));
  return [...mods, ...others];
}

export function formatHotkeyLabel(hotkey: string): string {
  if (hotkey === "Fn") return "Fn";
  const parts = hotkey.split("+");
  return sortKeys(parts).map(formatKeySymbol).join(" ");
}

export function parseHotkeyParts(hotkey: string): string[] {
  if (hotkey === "Fn") return ["Fn"];
  return hotkey.split("+");
}
