/**
 * Editor layout toggle button.
 *
 * Toggles between split (editor + preview) and full-width (editor only) mode.
 * Communicates with app.ts via Tauri events:
 *   - Emits: `editor:request-full-width` / `editor:request-split`
 *   - Listens: `editor:full-width-changed` to update icon state
 */

import { applyCaptionOwnerClass } from "../utils/fullscreen-class";
import { host as uiHost } from "../host";

// SVG icons (Lucide)
const EXPAND_ICON = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/></svg>`;

const COLUMNS_ICON = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2"/><path d="M12 3v18"/></svg>`;

let isEditorFullWidth = false;

/**
 * Wire the layout toggle button in the editor webview.
 * Call once after editor DOM is ready.
 */
export function wireLayoutButton(): void {
  const btn = document.getElementById('editor-layout-btn');
  if (!btn) return;

  // Start in split mode: hidden (user drags divider to enter full-width).
  // Shown only in full-width mode with columns icon (exit affordance).
  // Safe to hide/show because app.ts awaits the Rust command before
  // emitting the state-change event — no race between icon hiding
  // and preview repositioning.
  btn.innerHTML = EXPAND_ICON;
  btn.style.display = 'none';
  isEditorFullWidth = false;

  btn.addEventListener('click', () => {
    if (isEditorFullWidth) {
      uiHost.bus.emit('editor:request-split');
    } else {
      uiHost.bus.emit('editor:request-full-width');
    }
  });

  // ⌘⇧E / Ctrl+Shift+E — the keyboard twin the button's tooltip chip
  // advertises. app.ts binds the same shortcut, but keydown never crosses
  // webviews: while the user is typing in the editor (the only place the
  // button is visible) the shell handler can't fire, so the shortcut must
  // also live here. Both paths emit the same request events.
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === 'E') {
      e.preventDefault();
      uiHost.bus.emit(isEditorFullWidth ? 'editor:request-split' : 'editor:request-full-width');
    }
  });

  // Listen for state changes from app.ts.
  // Show button in full-width (exit affordance), hide in split (user drags to expand).
  uiHost.bus.on<{ fullWidth: boolean }>('editor:full-width-changed', (payload) => {
    isEditorFullWidth = payload.fullWidth;
    btn.innerHTML = isEditorFullWidth ? COLUMNS_ICON : EXPAND_ICON;
    btn.style.display = isEditorFullWidth ? '' : 'none';
    // Windows: this webview only owns the caption row while full-width (the
    // preview is hidden then, so it has none of its own).
    applyCaptionOwnerClass(payload.fullWidth);
  });
}
