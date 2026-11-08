// Blocks stray control characters from being inserted into any plain
// <input>/<textarea> in this webview.
//
// ── Why this exists ──────────────────────────────────────────────────────
// On macOS, an UNFIXED wry bug surfaces under Tauri v2's `unstable` multiwebview
// configuration (tauri#10194): a webview's `keyDown:` forwards arrow keys into
// AppKit's `interpretKeyEvents:` -> `insertText:`, which types the arrow key's
// legacy control code (Left 0x1C, Right 0x1D, Up 0x1E, Down 0x1F) into the
// field instead of only moving the caret. Those controls have no glyph, so
// they render as "tofu" boxes, and they persist into saved frontmatter. moss
// is exposed because it enables `unstable` and hosts the editor as a child
// webview via `window.add_child` (see tauri#10194 / wry#1177 below).
//
// The bug does not affect the CodeMirror editor because CM6 preventDefaults the
// arrow keydown and moves the caret itself — which is exactly the mitigation
// upstream recommends. For plain fields we cannot preventDefault the arrow
// keydown (it would break caret navigation), so instead we cancel the erroneous
// single-char `insertText` at `beforeinput`. In WebKit (Safari implements Input
// Events Level 2, where every inputType is cancelable) `preventDefault()` on an
// `insertText` `beforeinput` reliably drops the character while leaving caret
// movement, copy/paste, and IME composition untouched. Multi-char paths (paste,
// replacement, autofill) are cleaned by the `input` fallback instead, so their
// legitimate text survives — see `shouldBlockInsertion` and `installControlCharGuard`.
//
// ── References ───────────────────────────────────────────────────────────
//   Canonical open issue (our exact symptom):
//     https://github.com/tauri-apps/tauri/issues/10194
//   Unguarded child-webview keyDown:
//     https://github.com/tauri-apps/wry/blob/dev/src/wkwebview/class/wry_web_view_parent.rs
//   Related multiwebview key-routing issue:
//     https://github.com/tauri-apps/wry/issues/1177
//   `beforeinput` is cancelable for insertText (Input Events Level 2):
//     https://webkit.org/blog/7358/enhanced-editing-with-input-events/
//
// A version bump does NOT fix this and there is no macOS webview config
// toggle — so guarding at moss's own input boundary is the fix until the
// upstream child-webview keyDown path is patched. Rust mirrors this on the
// frontmatter write boundary.

// Code-point ranges that are never valid in a text field: control codes
// (0x00-0x1F) EXCEPT TAB (0x09), LF (0x0A), CR (0x0D); DEL (0x7F); and
// controls (0x80-0x9F). Expressed numerically so no literal control byte or
// fragile \u escape lives in the source.
const CONTROL_RANGES: ReadonlyArray<readonly [number, number]> = [
  [0x00, 0x08],
  [0x0b, 0x0c],
  [0x0e, 0x1f],
  [0x7f, 0x9f],
];

function isControlCode(code: number): boolean {
  for (const [lo, hi] of CONTROL_RANGES) {
    if (code >= lo && code <= hi) return true;
  }
  return false;
}

/** True if `s` contains any control char (excluding TAB/LF/CR). */
export function containsControlChars(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    if (isControlCode(s.charCodeAt(i))) return true;
  }
  return false;
}

/** Remove all control chars (excluding TAB/LF/CR) from `s`. */
export function stripControlChars(s: string): string {
  let out = '';
  for (let i = 0; i < s.length; i++) {
    if (!isControlCode(s.charCodeAt(i))) out += s[i];
  }
  return out;
}

/** Count of control chars in `s.slice(0, n)` — keeps the caret stable on strip. */
function countControlCharsUpTo(s: string, n: number): number {
  let count = 0;
  const end = Math.min(n, s.length);
  for (let i = 0; i < end; i++) {
    if (isControlCode(s.charCodeAt(i))) count++;
  }
  return count;
}

/**
 * Pure decision for the beforeinput guard: should this insertion be cancelled?
 *
 * We cancel ONLY single-key text insertion (`insertText`) — that is the exact
 * vector of the arrow-key bug, and cancelling it is lossless (there is nothing
 * to keep). Paste (`insertFromPaste`) and replacement (`insertReplacementText`)
 * are deliberately NOT cancelled here: dropping a whole paste just because it
 * contains one stray control char would also discard the clean text with it.
 * Those multi-char paths are handled by the `input` strip fallback instead,
 * which removes only the control chars and keeps the rest. IME commit
 * (`insertCompositionText`) is non-cancelable and never touched.
 */
export function shouldBlockInsertion(
  inputType: string,
  data: string,
  isComposing: boolean,
): boolean {
  if (isComposing) return false;
  if (inputType !== 'insertText') return false;
  return containsControlChars(data);
}

function isTextField(el: EventTarget | null): el is HTMLInputElement | HTMLTextAreaElement {
  return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
}

/** Strip control chars from a field's value, preserving the caret position. */
function sanitizeField(el: HTMLInputElement | HTMLTextAreaElement): void {
  const before = el.value;
  const after = stripControlChars(before);
  if (before === after) return;
  const start = el.selectionStart ?? after.length;
  const end = el.selectionEnd ?? after.length;
  const removedBeforeStart = countControlCharsUpTo(before, start);
  const removedBeforeEnd = countControlCharsUpTo(before, end);
  el.value = after;
  try {
    el.setSelectionRange(start - removedBeforeStart, end - removedBeforeEnd);
  } catch {
    // setSelectionRange throws on inputs that don't support text selection
    // (e.g. type="number"/"date"); the value strip already took effect.
  }
}

/**
 * Install a webview-wide guard against control-character insertion.
 *
 * Primary path: cancel a bad single-char `insertText` at `beforeinput` (the
 * arrow-key bug vector; reliably cancelable in WebKit).
 * Fallback: strip on `input` (and on `compositionend`) for everything else —
 * paste, replacement, autofill, spellcheck, and any non-cancelable path — so
 * only the control chars are removed and legitimate text is preserved. Both
 * run in the CAPTURE phase so a cleaned value reaches the app's own bubble-
 * phase `input` listeners (e.g. the frontmatter store) before they read it.
 *
 * Safe to call once per webview document. Returns an uninstaller.
 */
export function installControlCharGuard(target: Document = document): () => void {
  const onBeforeInput = (e: Event): void => {
    const ie = e as InputEvent;
    // Only insertText carries its data here; paste/replacement are left to the
    // input fallback (see shouldBlockInsertion) so clean text is not discarded.
    if (shouldBlockInsertion(ie.inputType, ie.data ?? '', ie.isComposing)) {
      e.preventDefault();
    }
  };

  const onInput = (e: Event): void => {
    if (!isTextField(e.target)) return;
    // Don't rewrite the value mid-composition — it aborts the IME session.
    if ((e as InputEvent).isComposing) return;
    sanitizeField(e.target);
  };

  const onCompositionEnd = (e: Event): void => {
    if (isTextField(e.target)) sanitizeField(e.target);
  };

  target.addEventListener('beforeinput', onBeforeInput, { capture: true });
  target.addEventListener('input', onInput, { capture: true });
  target.addEventListener('compositionend', onCompositionEnd, { capture: true });

  return () => {
    target.removeEventListener('beforeinput', onBeforeInput, { capture: true });
    target.removeEventListener('input', onInput, { capture: true });
    target.removeEventListener('compositionend', onCompositionEnd, { capture: true });
  };
}
