/**
 * Cross-webview glue for the editor font-size dial.
 *
 * The pure logic (presets, persistence, the --moss-editor-font-scale write)
 * lives in shared/editor-font-size.ts; this module adds the Tauri event
 * traffic that keeps the two writers — ⌘+/−/0 in the editor webview and the
 * aA stepper in the shell's Settings ▸ Editor tab — showing one value, live.
 *
 * Contract: every change goes through setEditorFontSize (persist first, then
 * broadcast); receivers apply/reflect WITHOUT re-broadcasting, so a change
 * settles in one hop and never echoes.
 */

import {
  EDITOR_FONT_SIZE_EVENT,
  type EditorFontSizePayload,
  applyEditorFontScale,
  loadEditorFontSize,
  saveEditorFontSize,
  snapEditorFontSize,
} from '../shared/editor-font-size';
import { host } from '../host';

/**
 * Make a change from THIS webview: persist, apply to `root` (the editor
 * webview passes its documentElement; the shell passes none — its document
 * consumes no editor tokens), broadcast. Returns the snapped preset.
 */
export function setEditorFontSize(px: number, root?: HTMLElement): number {
  const snapped = snapEditorFontSize(px);
  saveEditorFontSize(snapped);
  if (root) applyEditorFontScale(root, snapped);
  host.bus.emit(EDITOR_FONT_SIZE_EVENT, { px: snapped } satisfies EditorFontSizePayload);
  return snapped;
}

/** Follow changes broadcast by any webview (including this one — idempotent). */
export function onEditorFontSizeChange(handler: (px: number) => void): () => void {
  return host.bus.on<EditorFontSizePayload>(EDITOR_FONT_SIZE_EVENT, (payload) => {
    handler(snapEditorFontSize(Number(payload?.px)));
  });
}

/**
 * Editor-webview bootstrap: apply the persisted preset immediately (before
 * CM6 mounts) and keep following broadcasts for the life of the webview.
 */
export function initEditorFontScale(root: HTMLElement): void {
  applyEditorFontScale(root, loadEditorFontSize());
  void onEditorFontSizeChange((px) => {
    saveEditorFontSize(px);
    applyEditorFontScale(root, px);
  });
}
