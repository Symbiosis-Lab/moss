/**
 * Media-agnostic drag-to-resize controller for block media widgets (images
 * today; shared across video/pdf/iframe/
 * folder placeholder cards). Owns the lower-right corner-bracket handle, a live
 * percentage badge, and the pointer gesture.
 *
 * It performs ZERO CodeMirror transactions during the drag — the badge text and
 * the wrapper width are direct DOM writes — so the host block widget is never
 * rebuilt mid-gesture (CM6's "content next to a widget changes" rebuild path is
 * never triggered, and pointer capture can't be lost). Exactly one commit fires
 * on release. Move/up listeners live on `window`, not the handle, so the gesture
 * survives even if the widget DOM is recreated for an unrelated reason.
 */

import { t } from "../i18n/index";

/** Corner-bracket "」": down the right edge, then left across the bottom.
 *  `currentColor` stroke so CSS recolors it on grab; CSS adds the contrast halo. */
const HANDLE_SVG =
  '<svg viewBox="0 0 12 12" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4 L11 11 L4 11"/></svg>';

/** Px the pointer must travel before a gesture counts as a resize (vs a click). */
const MOVE_THRESHOLD = 3;
/** Max ms between two no-move clicks to count as a double-click reset. */
const DBLCLICK_MS = 300;

/**
 * Pure drag-math: wrapper width at gesture start (`startW`, px) + horizontal
 * pointer delta (`dx`, px) over the content-column width (`contentW`, px) →
 * integer percent of the content column, clamped to [10, 100]. The 10% floor
 * keeps the handle grabbable; 100% = full content width.
 */
export function clampPercent(startW: number, dx: number, contentW: number): number {
  if (!(contentW > 0)) return 10; // degenerate (collapsed/unlaid-out) → safe floor
  const pct = Math.round(((startW + dx) / contentW) * 100);
  return Math.max(10, Math.min(100, pct));
}

export interface ResizeControllerOptions {
  /** Element whose `style.width` is set live during the drag (the figure/wrapper). */
  wrapper: HTMLElement;
  /** Content-column width in px, evaluated at gesture start (percent basis). */
  getContentWidth: () => number;
  /** Commit on release: `"NN%"` sets the width, `null` resets. At most once per gesture. */
  onCommit: (width: string | null) => void;
  /** Accessible label for the resize handle. Default: localized "Resize image". */
  ariaLabel?: string;
  /** Class-name prefix for the handle/badge (e.g. 'cm-lp-image'). Default 'cm-lp-image'
   *  so existing image CSS keeps working; media-embed cards reuse the same CSS. */
  classPrefix?: string;
}

export interface ResizeController {
  /** Append this into the wrapper; it is the grab target. */
  handle: HTMLElement;
  /** Remove window listeners + transient DOM. Call on widget destroy. */
  destroy(): void;
}

export function createResizeController(opts: ResizeControllerOptions): ResizeController {
  const { wrapper, getContentWidth, onCommit } = opts;
  const ariaLabel = opts.ariaLabel ?? t('editmisc.resize_image');
  const classPrefix = opts.classPrefix ?? 'cm-lp-image';

  const handle = document.createElement('div');
  handle.className = `${classPrefix}-handle`;
  handle.setAttribute('aria-label', ariaLabel);
  handle.innerHTML = HANDLE_SVG;

  let startX = 0;
  let startW = 0;
  let contentW = 0;
  let dragging = false;
  let moved = false;
  let badge: HTMLElement | null = null;
  let lastClickTime = 0;
  /** The wrapper's own `max-width`, held while a drag overrides it. */
  let parkedMaxWidth: string | null = null;

  const setBadge = (pct: number) => {
    if (!badge) {
      badge = document.createElement('div');
      badge.className = `${classPrefix}-badge`;
      wrapper.appendChild(badge);
    }
    badge.textContent = `${pct}%`;
  };
  const clearBadge = () => { if (badge) { badge.remove(); badge = null; } };

  const onMove = (e: PointerEvent) => {
    if (!dragging) return;
    const dx = e.clientX - startX;
    if (Math.abs(dx) >= MOVE_THRESHOLD) moved = true;
    const pct = clampPercent(startW, dx, contentW);
    wrapper.style.width = `${pct}%`; // live visual — no doc edit, no rebuild
    setBadge(pct);
  };

  const stop = () => {
    // Give the wrapper its own max-width back. A live drag writes
    // `style.width` directly, so any max-width on the same element silently
    // contradicts it — the pointer and the badge keep moving while the picture
    // stops growing, which reads as a stuck handle. The editor's image height
    // cap is exactly such a max-width, and a resize is the one moment
    // the user is deliberately asking to see the size, so the cap yields for
    // the length of the gesture and returns when the widget rebuilds.
    if (parkedMaxWidth !== null) {
      wrapper.style.maxWidth = parkedMaxWidth;
      parkedMaxWidth = null;
    }
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onUp);
  };

  const onUp = (e: PointerEvent) => {
    if (!dragging) return;
    dragging = false;
    stop();
    clearBadge();
    if (moved) {
      onCommit(`${clampPercent(startW, e.clientX - startX, contentW)}%`);
      lastClickTime = 0;
    } else {
      const now = Date.now();
      if (now - lastClickTime <= DBLCLICK_MS) { onCommit(null); lastClickTime = 0; }
      else { lastClickTime = now; } // first click of a possible double — no commit
    }
  };

  const onDown = (e: PointerEvent) => {
    // Stop CM6 from treating this as a click (cursor placement → source reveal →
    // mid-gesture rebuild). Window listeners keep the gesture alive regardless.
    e.preventDefault();
    e.stopPropagation();
    dragging = true;
    moved = false;
    startX = e.clientX;
    startW = wrapper.getBoundingClientRect().width;
    contentW = getContentWidth();
    parkedMaxWidth = wrapper.style.maxWidth;
    wrapper.style.maxWidth = 'none';
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onUp);
  };

  handle.addEventListener('pointerdown', onDown);

  return {
    handle,
    // destroy() aborts the gesture and cleans up listeners/badge, but does NOT
    // revert any live wrapper.style.width (the host widget DOM is torn down anyway).
    destroy() { stop(); clearBadge(); },
  };
}
