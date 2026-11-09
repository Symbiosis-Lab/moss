/**
 * Preview Zoom Controller
 *
 * Manages zoom level for the preview iframe via CSS transform: scale().
 * The toolbar chrome (titlebar, pill, control panel) is in the shell and is NOT
 * scaled — only the iframe content is affected.
 *
 * Independence from the per-site Aa font-scale pill (moss-font-scale):
 * - This zoom is a SHELL-layer transform on the iframe element itself.
 * - The site's Aa pill (`moss-font-scale`) adjusts --moss-reading-size inside
 *   the page CSS, which is baked into the published HTML/CSS.
 * - The two layers are fully independent: shell zoom + site Aa are both applied
 *   simultaneously and do not interfere. Shell zoom is not persisted to the site;
 *   site Aa preference is not visible here.
 *
 * Implementation notes:
 * - Uses CSS transform: scale() on the <iframe> element (cross-origin safe).
 * - Width is compensated to 100/factor% so layout still fills the pane.
 * - In mobile mode the iframe already uses translate(-50%, -50%); we preserve
 *   that by composing scale onto the existing transform string rather than
 *   replacing it. Mobile mode is detected via the container class.
 * - Persisted in localStorage['moss-preview-zoom'] as a factor string.
 * - Discrete step table: 0.8, 0.9, 1.0, 1.1, 1.25, 1.5.
 */

import { t } from "../i18n/index";

export const PREVIEW_ZOOM_STEPS = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];
export const PREVIEW_ZOOM_DEFAULT = 1.0;
const PREVIEW_ZOOM_KEY = 'moss-preview-zoom';

/** Round to 4 decimal places to avoid float noise in localStorage. */
function round(n: number): number {
  return Math.round(n * 10000) / 10000;
}

/** Load the persisted zoom factor, falling back to 1.0 on any error. */
export function loadPreviewZoom(): number {
  try {
    const stored = localStorage.getItem(PREVIEW_ZOOM_KEY);
    if (!stored) return PREVIEW_ZOOM_DEFAULT;
    const val = parseFloat(stored);
    return isNaN(val) ? PREVIEW_ZOOM_DEFAULT : val;
  } catch {
    return PREVIEW_ZOOM_DEFAULT;
  }
}

/** Persist a zoom factor. */
export function savePreviewZoom(factor: number): void {
  try {
    if (factor === PREVIEW_ZOOM_DEFAULT) {
      localStorage.removeItem(PREVIEW_ZOOM_KEY);
    } else {
      localStorage.setItem(PREVIEW_ZOOM_KEY, String(round(factor)));
    }
  } catch {
    // localStorage blocked (private mode) — no-op
  }
}

/**
 * Compute the next zoom factor after applying delta (+1/-1/0).
 * delta=0 resets to PREVIEW_ZOOM_DEFAULT.
 * Clamps to the step table boundaries.
 */
export function stepZoom(current: number, delta: number): number {
  if (delta === 0) return PREVIEW_ZOOM_DEFAULT;
  // Find nearest step index to current value
  let nearestIdx = 0;
  let nearestDist = Infinity;
  for (let i = 0; i < PREVIEW_ZOOM_STEPS.length; i++) {
    const dist = Math.abs(PREVIEW_ZOOM_STEPS[i] - current);
    if (dist < nearestDist) { nearestDist = dist; nearestIdx = i; }
  }
  const newIdx = Math.max(0, Math.min(PREVIEW_ZOOM_STEPS.length - 1, nearestIdx + delta));
  return PREVIEW_ZOOM_STEPS[newIdx];
}

/**
 * Apply a zoom factor to a preview iframe element.
 *
 * In desktop mode: sets transform=scale(factor) and width=100/factor%.
 * In mobile mode: the iframe already has transform=translate(-50%,-50%), so we
 * compose the scale as "translate(-50%,-50%) scale(factor)" to preserve centering.
 * The container's mobile-mode class is the signal.
 *
 * @param iframe - the preview iframe element
 * @param factor - zoom factor (e.g. 1.0 = 100%, 1.25 = 125%)
 * @param isMobile - true when the container has mobile-mode class
 */
export function applyZoomToIframe(
  iframe: HTMLIFrameElement,
  factor: number,
  isMobile: boolean,
): void {
  if (isMobile) {
    // Mobile centering relies on the default 50% 50% transform-origin; clear any
    // top-left origin left over from a prior desktop zoom so the
    // translate(-50%, -50%) centering is not disrupted.
    iframe.style.transformOrigin = '';
    // Mobile mode: compose scale onto the existing centering translate.
    // Width/height are fixed (390×844px) — no compensation needed.
    if (factor === 1.0) {
      iframe.style.transform = 'translate(-50%, -50%)';
    } else {
      iframe.style.transform = `translate(-50%, -50%) scale(${round(factor)})`;
    }
    // Width stays as set by mobile CSS — do not override.
    iframe.style.width = '';
  } else {
    // Desktop mode: scale + compensate width so layout fills the pane.
    // Without width compensation the iframe shrinks visually (scale shrinks
    // the physical pixels) and leaves a gap on the right.
    //
    // transform-origin is set to "0 0" (top-left) so the scaled content
    // anchors at the content origin. The iframe has left:var(--action-panel-width)
    // in the shell CSS; without top-left origin the default 50% 50% pivot
    // creates a left-edge gap at every non-default zoom level.
    //
    // Width math: iframe width = 100/factor%, scaled by factor → 100% of
    // container. e.g. factor=0.8 → width=125%, 125%×0.8=100% perceived.
    // e.g. factor=1.25 → width=80%, 80%×1.25=100% perceived. No gap, no overflow.
    if (factor === 1.0) {
      iframe.style.transform = '';
      iframe.style.transformOrigin = '';
      iframe.style.width = '';
    } else {
      iframe.style.transformOrigin = '0 0';
      iframe.style.transform = `scale(${round(factor)})`;
      // Expand the logical width so the scaled-down content still fills the pane.
      // e.g. scale(0.8) → width:125% → 125%×0.8=100% perceived.
      const compensated = round(100 / factor);
      iframe.style.width = `${compensated}%`;
    }
  }
}

/**
 * PreviewZoomController — owns zoom state and applies it to the iframe.
 *
 * Designed to be owned by PreviewManager. Separated for testability.
 */
export class PreviewZoomController {
  private factor: number;
  private iframe: HTMLIFrameElement | null;
  private isMobileMode = false;

  constructor(iframe: HTMLIFrameElement | null) {
    this.iframe = iframe;
    this.factor = loadPreviewZoom();
    // Apply any persisted non-default zoom immediately so the preview does not
    // render at 100% on startup before the user presses a key. The shell iframe
    // is a static element (navigation only mutates its src, not the element), so
    // the inline transform persists across navigations with no re-apply needed.
    if (this.factor !== PREVIEW_ZOOM_DEFAULT) {
      this.applyNow();
    }
  }

  /** Returns the current zoom factor. */
  getFactor(): number { return this.factor; }

  /** Called by PreviewManager when mobile mode changes. */
  setMobileMode(mobile: boolean): void {
    this.isMobileMode = mobile;
    // Re-apply current zoom with updated mode context.
    this.applyNow();
  }

  /**
   * Apply a step delta (+1 = zoom in, -1 = zoom out, 0 = reset).
   * Persists and applies immediately.
   */
  step(delta: number): void {
    const next = stepZoom(this.factor, delta);
    if (next === this.factor && delta !== 0) return; // already at boundary
    this.factor = next;
    savePreviewZoom(this.factor);
    this.applyNow();
  }

  /** Apply the current factor to the iframe (call after iframe navigation too). */
  applyNow(): void {
    if (!this.iframe) return;
    if (this.factor === PREVIEW_ZOOM_DEFAULT) {
      // Reset to no-transform state.
      applyZoomToIframe(this.iframe, PREVIEW_ZOOM_DEFAULT, this.isMobileMode);
      return;
    }
    applyZoomToIframe(this.iframe, this.factor, this.isMobileMode);
  }

  /** Destroy and remove zoom transform (e.g. when PreviewManager is destroyed). */
  destroy(): void {
    if (this.iframe) {
      applyZoomToIframe(this.iframe, PREVIEW_ZOOM_DEFAULT, false);
    }
    this.iframe = null;
  }
}

/**
 * Transient zoom badge — "125%" over the preview corner on every zoom change,
 * auto-hiding after a beat; clicking it resets to 100%. Styled in style.css
 * (`.moss-zoom-badge`); the fade is CSS-only, the timeout hides it regardless.
 */
const BADGE_VISIBLE_MS = 1200;

export class ZoomBadge {
  private el: HTMLButtonElement | null;
  private hideTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(container: HTMLElement, onReset: () => void) {
    this.el = document.createElement('button');
    this.el.type = 'button';
    this.el.className = 'moss-zoom-badge';
    this.el.setAttribute('aria-label', t('preview.zoom.reset'));
    // Clicking resets to 100% — same as the ⌘0 zoom key the badge reacts to;
    // the tooltip teaches it (design-vocabulary: tooltips teach the shortcut).
    this.el.setAttribute('data-tooltip', t('preview.zoom.reset'));
    this.el.setAttribute('data-tooltip-kbd', '⌘0');
    this.el.addEventListener('click', () => onReset());
    container.appendChild(this.el);
  }

  /** Show the factor as a percentage and (re)start the auto-hide clock. */
  show(factor: number): void {
    if (!this.el) return;
    this.el.textContent = `${Math.round(factor * 100)}%`;
    this.el.classList.add('visible');
    if (this.hideTimer !== null) clearTimeout(this.hideTimer);
    this.hideTimer = setTimeout(() => {
      this.el?.classList.remove('visible');
      this.hideTimer = null;
    }, BADGE_VISIBLE_MS);
  }

  destroy(): void {
    if (this.hideTimer !== null) clearTimeout(this.hideTimer);
    this.hideTimer = null;
    this.el?.remove();
    this.el = null;
  }
}
