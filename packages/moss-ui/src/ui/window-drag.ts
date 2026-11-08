/**
 * Window drag regions — the shared allowlist model for moss's window chrome
 * (the preview shell titlebar, the editor breadcrumb, and the action-panel
 * surfaces: composer, matters bar, login chrome, launcher App Settings).
 *
 * A frameless Tauri window is moved by grabbing a "drag region". We make drag
 * OPT-IN: only elements explicitly marked with `data-tauri-drag-region` move the
 * window. Tauri matches the EXACT mousedown target, so a marked container makes
 * only its own bare surface draggable — child buttons/inputs are their own
 * targets and stay clickable with NO opt-out needed.
 *
 * This is deliberately the inverse of the old `-webkit-app-region: drag` +
 * `no-drag` carve-out model, which was a denylist over a geometric, lazily
 * recomputed region: every new/overflowing/dynamically-rendered descendant
 * re-leaked the window-drag and had to be patched with another `no-drag`. An
 * allowlist can't leak — an element that was never marked simply never drags.
 *
 * Use `markDragRegion` on the bare surface that should move the window (a
 * titlebar background, a breadcrumb strip), and `createDragSpacer` for a
 * flex-grow filler that both pushes controls apart and is itself draggable.
 */

/** The attribute Tauri's built-in handler looks for on the mousedown target. */
export const DRAG_REGION_ATTR = "data-tauri-drag-region";

/** Opt an element into window dragging (allowlist). Its interactive children
 *  remain clickable without any opt-out, because Tauri checks the exact
 *  mousedown target — a child button is its own target, not this element. */
export function markDragRegion(el: HTMLElement): void {
  el.setAttribute(DRAG_REGION_ATTR, "");
}

/** A flex-grow spacer that fills the gap between titlebar controls and is
 *  itself a drag handle (the spacer, not its container, is the mousedown
 *  target over that gap — so it must carry the attribute directly). */
export function createDragSpacer(className: string): HTMLDivElement {
  const el = document.createElement("div");
  el.className = className;
  markDragRegion(el);
  return el;
}
