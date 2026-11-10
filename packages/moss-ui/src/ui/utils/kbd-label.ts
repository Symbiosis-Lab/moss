// What to print on a keyboard-shortcut chip, on the platform it is printed on.
//
// A literal glyph chip (`'⌘0'`) is harmless for a shortcut the macOS-only app
// menu registers: on Linux the accelerator does not exist either. Undo/redo
// are different. Their keymap is registered inside CodeMirror, so Ctrl+Z
// genuinely works on Linux and Windows — and a chip reading `⌘Z` there
// misnames a working key, on the two platforms where the keymap is the ONLY
// path to it (there is no Edit menu).

import { uiPlatform } from './platform';

/** True on macOS, where modifiers render as glyphs and Mod is Command. */
export function isMacPlatform(): boolean {
  return uiPlatform() === 'mac';
}

/**
 * Render a CodeMirror-style shortcut spec for display.
 *
 * Takes the same `Mod`/`Shift`/`Alt` vocabulary CM6's keymap uses, so the chip
 * and the binding can be written from one string and cannot drift:
 *
 *   kbdLabel(UNDO_KEY)  → '⌘Z'   on macOS, 'Ctrl+Z'       elsewhere
 *   kbdLabel(REDO_KEY)  → '⇧⌘Z'  on macOS, 'Ctrl+Shift+Z' elsewhere
 *
 * macOS is glyphs with no separator, in Apple's FIXED ⌃⌥⇧⌘ order rather than
 * the order the spec happens to be written in — `⌘⇧Z` reads as a different
 * shortcut. Everywhere else it is `+`-joined words, which is what those
 * platforms' own menus print.
 */
export function kbdLabel(spec: string): string {
  const parts = spec.split('-');
  const key = parts.pop() ?? '';
  // CM6's full modifier vocabulary, not just the three moss uses today: an
  // unrecognized name would silently degrade to a bare key with no modifier at
  // all, which is a worse label than a wrong one.
  const ALIASES: Record<string, string> = {
    mod: 'mod', cmd: 'mod', meta: 'mod',
    ctrl: 'ctrl', control: 'ctrl',
    alt: 'alt', option: 'alt',
    shift: 'shift',
  };
  const names = new Set(parts.map((p) => ALIASES[p.toLowerCase()]).filter(Boolean));
  const has = (name: string) => names.has(name);
  if (isMacPlatform()) {
    return (
      (has('ctrl') ? '⌃' : '') +
      (has('alt') ? '⌥' : '') +
      (has('shift') ? '⇧' : '') +
      (has('mod') ? '⌘' : '') +
      key.toUpperCase()
    );
  }
  return wordsLabel(has, key.toUpperCase());
}

/** Windows and Linux: `+`-joined words in Ctrl, Alt, Shift order. */
function wordsLabel(has: (name: string) => boolean, key: string): string {
  return [
    has('mod') || has('ctrl') ? 'Ctrl' : '',
    has('alt') ? 'Alt' : '',
    has('shift') ? 'Shift' : '',
    key,
  ].filter(Boolean).join('+');
}

const GLYPH_MODS: Record<string, string> = { '⌃': 'ctrl', '⌥': 'alt', '⇧': 'shift', '⌘': 'mod' };
const GLYPH_KEYS: Record<string, string> = { '⌫': 'Backspace', '⌦': 'Delete', '↩': 'Enter', '⎋': 'Esc', '⇥': 'Tab' };

/**
 * A chip written in macOS glyphs (`'⇧⌘N'`, `'⌘⌫'`, `'↩'`), as it reads on the
 * platform it is shown on. Every chip renderer passes its text through here, so
 * a chip literal can stay in the one notation the keymap comments use and still
 * print `Ctrl+Shift+N` on Windows and Linux.
 */
export function kbdChipText(chip: string): string {
  if (isMacPlatform()) return chip;
  const chars = [...chip];
  if (!chars.some((c) => c in GLYPH_MODS || c in GLYPH_KEYS)) return chip;
  const mods = new Set(chars.filter((c) => c in GLYPH_MODS).map((c) => GLYPH_MODS[c]));
  const rest = chars.filter((c) => !(c in GLYPH_MODS)).map((c) => GLYPH_KEYS[c] ?? c).join('');
  return wordsLabel((name) => mods.has(name), rest.length === 1 ? rest.toUpperCase() : rest);
}

/**
 * The undo/redo bindings, in one place.
 *
 * The chip and the CodeMirror keymap are registered from THESE constants, in
 * two different places — the editor's state cluster renders them, its
 * CodeMirror setup binds them. Two literals with nothing tying them together can drift, and a
 * shortcut chip that names a key nothing is bound to is the exact failure this
 * module exists to prevent.
 */
export const UNDO_KEY = 'Mod-z';
export const REDO_KEY = 'Mod-Shift-z';

/**
 * Does this keydown match a CM6-style spec? Same vocabulary as `kbdLabel`.
 *
 * A CM6 keymap only sees keys whose event path runs through its `contentDOM`,
 * so the CodeMirror binding answers for the body and nothing else. The
 * editor also needs to answer with focus on a chip, the heading, or nothing at
 * all — and on Linux and Windows there is no Edit menu to catch those. Rather
 * than hand-compare modifiers at that second call site (where a mismatch would
 * be a shortcut that works in one half of the pane and not the other), both
 * sides resolve the SAME spec string through this module.
 *
 * `Mod` is Command on macOS and Control elsewhere, exactly as CM6 defines it.
 * `shift` is compared only when the spec asks for it — `Mod-z` must NOT match
 * ⇧⌘Z, or undo would swallow redo.
 *
 * Two behaviours here exist to MATCH CM6 rather than to be reasoned from first
 * principles, because the whole point is that both sides answer alike:
 *
 *   - the `keyCode` fallback (see `latinFallback`), without which ⌃Z works in
 *     the body and nowhere else on a non-Latin layout;
 *   - refusing to match mid-composition, so an IME's own undo-conversion keeps
 *     its keystroke.
 */
export function matchesKeySpec(spec: string, e: KeyboardEvent): boolean {
  if (e.isComposing) return false;
  const parts = spec.split('-');
  const key = (parts.pop() ?? '').toLowerCase();
  const want = new Set(parts.map((p) => p.toLowerCase()));
  const mac = isMacPlatform();
  const wantsMod = want.has('mod') || want.has('cmd') || want.has('meta');
  const wantsCtrl = want.has('ctrl') || want.has('control');
  // Mod is Command on macOS, Control elsewhere. Both platform modifiers are
  // compared, not just the wanted one: ⌃⌘Z is somebody else's shortcut, and a
  // spec that ignored the modifier it did not ask for would claim it.
  const wantMeta = mac ? wantsMod : false;
  const wantCtrl = wantsCtrl || (!mac && wantsMod);
  if (e.metaKey !== wantMeta) return false;
  if (e.ctrlKey !== wantCtrl) return false;
  if (want.has('alt') !== e.altKey) return false;
  if (want.has('shift') !== e.shiftKey) return false;
  if (e.key.toLowerCase() === key) return true;
  // On a Russian, Greek, Hebrew or Thai layout ⌃Z reports `e.key === 'я'` — the
  // letter the key prints — so a name comparison alone finds no shortcut at
  // all. CM6 handles this with a documented fallback to the US-layout name via
  // `keyCode` whenever a modifier is held (`runHandlers`, @codemirror/view), and
  // this must do the same or the shortcut works inside the editor body and
  // nowhere else in the pane, which is precisely the per-surface divergence the
  // caller exists to remove. Only with a modifier down, as CM6 has it: bare `z`
  // on such a layout is a letter the user is typing.
  if (!(wantMeta || wantCtrl || want.has('alt'))) return false;
  // …and only when the key does not already NAME a Latin character. This is the
  // difference between the fallback and a second, competing set of bindings.
  // `keyCode` is derived from the physical key through a US table, so on a
  // German layout the key that prints `y` sits where US has `z` and reports
  // keyCode 90 — a fallback that ran anyway would answer "this is ⌘Z" to a
  // ⌘Y the user pressed, and since callers ask about undo before redo, ⌘Y
  // would UNDO on every QWERTZ machine. The fallback is for a key whose name is
  // not in the Latin alphabet at all; a key that names `y` has already said
  // what it is.
  //
  // The length check is the other half, and it is the same rule read from the
  // other side: `Dead`, `Process`, `Unidentified` and every named key are not
  // one character, and re-reading them as whatever letter the physical key sits
  // under turns a compose sequence into a shortcut. So the fallback runs for
  // exactly one shape of input — a single character that is not in the Latin
  // alphabet, which is what "this layout prints something else" looks like.
  //
  // No test pins the length check on its own, and that is not an oversight:
  // every key NAME the DOM defines contains a Latin letter or digit, so the
  // alphabet test already rejects all of them and no real event can tell the
  // two halves apart. It stays because it states the actual rule — a name is
  // not a character — and the day a name arrives without a Latin letter in it,
  // the alphabet test would wave it straight through.
  if (e.key.length !== 1 || /[a-z0-9]/i.test(e.key)) return false;
  return latinFallback(e) === key;
}

/**
 * The US-layout name of the physical key, from the legacy `keyCode`. Letters
 * and digits only — that is the whole range specs use, and it is where layouts
 * disagree. Empty string for anything else, which matches nothing.
 */
function latinFallback(e: KeyboardEvent): string {
  const c = e.keyCode;
  if (c >= 65 && c <= 90) return String.fromCharCode(c).toLowerCase();
  if (c >= 48 && c <= 57) return String.fromCharCode(c);
  return '';
}
