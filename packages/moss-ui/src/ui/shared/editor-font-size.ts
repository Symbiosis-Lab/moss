/**
 * Editor font size — ONE dial shared by the aA stepper (Settings ▸ Editor,
 * shell webview) and ⌘+/−/0 (editor webview).
 *
 * The dial is a preset px value (11–18, default 13) persisted app-level in
 * localStorage['moss-editor-font-size'] — the same key (and the same 11–16
 * values) the old body-px mechanism used, so an existing preference carries
 * over unchanged. What the value DRIVES is new: instead of writing
 * `body.style.fontSize` (which the rem-based editor tokens ignored, so body
 * text grew while the heading stood still), it sets ONE unitless ratio,
 * `--moss-editor-font-scale`, on the editor document root. The editor tokens
 * multiply by it (`--moss-editor-reading-size` in editor-tokens.css), so the
 * CM6 body, the in-document headings, the inline title, and the 42× measure
 * all scale coherently.
 *
 * Two writers, one value: both surfaces persist through save/load here and
 * broadcast EDITOR_FONT_SIZE_EVENT (Tauri event, crosses webviews) so the
 * other surface follows live. Receivers apply without re-broadcasting, so
 * there is no echo loop.
 */

export const EDITOR_FONT_SIZE_STEPS = [11, 12, 13, 14, 15, 16, 17, 18];
export const EDITOR_FONT_SIZE_DEFAULT = 13;
export const EDITOR_FONT_SIZE_KEY = 'moss-editor-font-size';

/** Tauri event carrying `{ px: number }` — emitted on every change, from
 * whichever webview made it. */
export const EDITOR_FONT_SIZE_EVENT = 'moss:editor-font-size';

export interface EditorFontSizePayload {
  px: number;
}

/** Snap an arbitrary number to the nearest preset step. */
export function snapEditorFontSize(px: number): number {
  if (!Number.isFinite(px)) return EDITOR_FONT_SIZE_DEFAULT;
  let nearest = EDITOR_FONT_SIZE_STEPS[0];
  for (const step of EDITOR_FONT_SIZE_STEPS) {
    if (Math.abs(step - px) < Math.abs(nearest - px)) nearest = step;
  }
  return nearest;
}

/** Load the persisted size, snapped to a valid step; default on any error. */
export function loadEditorFontSize(): number {
  try {
    const stored = localStorage.getItem(EDITOR_FONT_SIZE_KEY);
    if (!stored) return EDITOR_FONT_SIZE_DEFAULT;
    return snapEditorFontSize(parseFloat(stored));
  } catch {
    return EDITOR_FONT_SIZE_DEFAULT;
  }
}

/** Persist a size. The default clears the key (same behavior as the old ⌘0). */
export function saveEditorFontSize(px: number): void {
  try {
    if (px === EDITOR_FONT_SIZE_DEFAULT) {
      localStorage.removeItem(EDITOR_FONT_SIZE_KEY);
    } else {
      localStorage.setItem(EDITOR_FONT_SIZE_KEY, String(px));
    }
  } catch {
    // localStorage blocked — the live-applied scale still works this session
  }
}

/**
 * Step to the adjacent preset (+1 / −1), clamped at the ends.
 * delta=0 resets to the default.
 */
export function stepEditorFontSize(current: number, delta: number): number {
  if (delta === 0) return EDITOR_FONT_SIZE_DEFAULT;
  const idx = EDITOR_FONT_SIZE_STEPS.indexOf(snapEditorFontSize(current));
  const next = Math.max(0, Math.min(EDITOR_FONT_SIZE_STEPS.length - 1, idx + delta));
  return EDITOR_FONT_SIZE_STEPS[next];
}

/** The unitless ratio a px preset means: default 13 → 1. */
export function editorFontScale(px: number): number {
  return Math.round((px / EDITOR_FONT_SIZE_DEFAULT) * 10000) / 10000;
}

/**
 * Apply a preset to a document root (the editor webview's documentElement).
 * The default removes the property so the stylesheet's `var(…, 1)` fallback
 * rules and nothing inline lingers.
 */
export function applyEditorFontScale(root: HTMLElement, px: number): void {
  if (px === EDITOR_FONT_SIZE_DEFAULT) {
    root.style.removeProperty('--moss-editor-font-scale');
  } else {
    root.style.setProperty('--moss-editor-font-scale', String(editorFontScale(px)));
  }
}
// The cross-webview broadcast glue (Tauri emit/listen) lives in
// utils/editor-font-scale.ts, since shared/ holds only pure modules.
