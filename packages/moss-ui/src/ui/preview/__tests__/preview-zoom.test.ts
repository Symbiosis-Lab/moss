/**
 * Tests for preview-zoom.ts
 * Covers step math, persistence, and iframe transform application.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  PREVIEW_ZOOM_STEPS,
  PREVIEW_ZOOM_DEFAULT,
  stepZoom,
  loadPreviewZoom,
  savePreviewZoom,
  applyZoomToIframe,
  PreviewZoomController,
} from '../preview-zoom';

// ── stepZoom ──────────────────────────────────────────────────────────────────

describe('stepZoom', () => {
  it('returns default on delta=0 regardless of current factor', () => {
    expect(stepZoom(0.8, 0)).toBe(PREVIEW_ZOOM_DEFAULT);
    expect(stepZoom(1.5, 0)).toBe(PREVIEW_ZOOM_DEFAULT);
    expect(stepZoom(1.0, 0)).toBe(PREVIEW_ZOOM_DEFAULT);
  });

  it('steps up from default (1.0) to next step', () => {
    const result = stepZoom(1.0, 1);
    const defaultIdx = PREVIEW_ZOOM_STEPS.indexOf(1.0);
    expect(result).toBe(PREVIEW_ZOOM_STEPS[defaultIdx + 1]);
  });

  it('steps down from default (1.0) to previous step', () => {
    const result = stepZoom(1.0, -1);
    const defaultIdx = PREVIEW_ZOOM_STEPS.indexOf(1.0);
    expect(result).toBe(PREVIEW_ZOOM_STEPS[defaultIdx - 1]);
  });

  it('clamps at the minimum step (0.8)', () => {
    expect(stepZoom(0.8, -1)).toBe(PREVIEW_ZOOM_STEPS[0]);
  });

  it('clamps at the maximum step (1.5)', () => {
    const max = PREVIEW_ZOOM_STEPS[PREVIEW_ZOOM_STEPS.length - 1];
    expect(stepZoom(max, 1)).toBe(max);
  });

  it('stepping up through all steps produces the full ascending sequence', () => {
    let factor = PREVIEW_ZOOM_DEFAULT;
    const visited: number[] = [factor];
    for (let i = 0; i < PREVIEW_ZOOM_STEPS.length; i++) {
      const next = stepZoom(factor, 1);
      if (next === factor) break;
      visited.push(next);
      factor = next;
    }
    // Every step above the default index should have been visited.
    const defaultIdx = PREVIEW_ZOOM_STEPS.indexOf(PREVIEW_ZOOM_DEFAULT);
    expect(visited.length).toBe(PREVIEW_ZOOM_STEPS.length - defaultIdx);
  });

  it('stepping down through all steps produces the full descending sequence', () => {
    let factor = PREVIEW_ZOOM_DEFAULT;
    const visited: number[] = [factor];
    for (let i = 0; i < PREVIEW_ZOOM_STEPS.length; i++) {
      const next = stepZoom(factor, -1);
      if (next === factor) break;
      visited.push(next);
      factor = next;
    }
    const defaultIdx = PREVIEW_ZOOM_STEPS.indexOf(PREVIEW_ZOOM_DEFAULT);
    expect(visited.length).toBe(defaultIdx + 1);
  });

  it('snaps an out-of-table factor to the nearest step then advances', () => {
    // 1.05 is between 1.0 and 1.1; nearest = 1.0; +1 → 1.1
    const result = stepZoom(1.05, 1);
    expect(result).toBe(1.1);
  });
});

// ── persistence ───────────────────────────────────────────────────────────────

describe('loadPreviewZoom / savePreviewZoom', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns default when nothing is stored', () => {
    expect(loadPreviewZoom()).toBe(PREVIEW_ZOOM_DEFAULT);
  });

  it('round-trips a non-default factor', () => {
    savePreviewZoom(1.25);
    expect(loadPreviewZoom()).toBe(1.25);
  });

  it('removes the key when saving default (1.0)', () => {
    savePreviewZoom(0.8);
    savePreviewZoom(PREVIEW_ZOOM_DEFAULT);
    expect(localStorage.getItem('moss-preview-zoom')).toBeNull();
    expect(loadPreviewZoom()).toBe(PREVIEW_ZOOM_DEFAULT);
  });

  it('returns default for corrupt stored value', () => {
    localStorage.setItem('moss-preview-zoom', 'NaN');
    expect(loadPreviewZoom()).toBe(PREVIEW_ZOOM_DEFAULT);
  });
});

// ── applyZoomToIframe ─────────────────────────────────────────────────────────

function makeIframe(): HTMLIFrameElement {
  return document.createElement('iframe');
}

describe('applyZoomToIframe — desktop mode', () => {
  it('clears transform, transformOrigin and width at factor=1.0', () => {
    const iframe = makeIframe();
    iframe.style.transform = 'scale(0.8)';
    iframe.style.transformOrigin = '0 0';
    iframe.style.width = '125%';
    applyZoomToIframe(iframe, 1.0, false);
    expect(iframe.style.transform).toBe('');
    expect(iframe.style.transformOrigin).toBe('');
    expect(iframe.style.width).toBe('');
  });

  it('sets scale transform and compensated width for factor < 1', () => {
    const iframe = makeIframe();
    applyZoomToIframe(iframe, 0.8, false);
    expect(iframe.style.transform).toContain('scale(0.8)');
    // 100/0.8 = 125
    expect(iframe.style.width).toBe('125%');
  });

  it('sets scale transform and compensated width for factor > 1', () => {
    const iframe = makeIframe();
    applyZoomToIframe(iframe, 1.25, false);
    expect(iframe.style.transform).toContain('scale(1.25)');
    // 100/1.25 = 80
    expect(iframe.style.width).toBe('80%');
  });

  it('sets transform-origin to top-left (0 0) for non-default zoom', () => {
    // The iframe is position:absolute with left:var(--action-panel-width).
    // Default transform-origin 50% 50% would create a left-edge gap at every
    // non-default zoom. Origin must be "0 0" so scale anchors at the content corner.
    const iframe = makeIframe();
    applyZoomToIframe(iframe, 0.8, false);
    expect(iframe.style.transformOrigin).toBe('0 0');

    const iframe2 = makeIframe();
    applyZoomToIframe(iframe2, 1.25, false);
    expect(iframe2.style.transformOrigin).toBe('0 0');
  });

  it('width×scale = 100% at every zoom step — no gap, no overflow', () => {
    // Mathematical invariant: compensated_width * factor = 100%.
    // Verify for every step in the table (excluding 1.0, reset path).
    const steps = PREVIEW_ZOOM_STEPS.filter(f => f !== PREVIEW_ZOOM_DEFAULT);
    for (const factor of steps) {
      const iframe = makeIframe();
      applyZoomToIframe(iframe, factor, false);
      const widthPct = parseFloat(iframe.style.width); // e.g. 125 for "125%"
      const product = widthPct * factor;
      expect(product).toBeCloseTo(100, 3);
    }
  });
});

describe('applyZoomToIframe — mobile mode', () => {
  it('keeps translate(-50%,-50%) at factor=1.0', () => {
    const iframe = makeIframe();
    applyZoomToIframe(iframe, 1.0, true);
    expect(iframe.style.transform).toBe('translate(-50%, -50%)');
    expect(iframe.style.width).toBe('');
  });

  it('composes scale onto translate for non-default factor', () => {
    const iframe = makeIframe();
    applyZoomToIframe(iframe, 1.25, true);
    expect(iframe.style.transform).toBe('translate(-50%, -50%) scale(1.25)');
    expect(iframe.style.width).toBe('');
  });

  it('does not override iframe width in mobile mode', () => {
    const iframe = makeIframe();
    iframe.style.width = '390px';
    applyZoomToIframe(iframe, 0.9, true);
    // Width should be cleared (not set to a percentage)
    expect(iframe.style.width).toBe('');
  });
});

// ── PreviewZoomController ─────────────────────────────────────────────────────

describe('PreviewZoomController', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('starts at persisted factor', () => {
    savePreviewZoom(1.25);
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    expect(ctrl.getFactor()).toBe(1.25);
  });

  it('step(1) increments and applies transform', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(1); // 1.0 → 1.1
    expect(ctrl.getFactor()).toBe(1.1);
    expect(iframe.style.transform).toContain('scale(1.1)');
  });

  it('step(-1) decrements and applies transform', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(-1); // 1.0 → 0.9
    expect(ctrl.getFactor()).toBe(0.9);
    expect(iframe.style.transform).toContain('scale(0.9)');
  });

  it('step(0) resets to default and clears transform', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(1); // advance first
    ctrl.step(0); // reset
    expect(ctrl.getFactor()).toBe(PREVIEW_ZOOM_DEFAULT);
    expect(iframe.style.transform).toBe('');
  });

  it('persists factor to localStorage on step', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(1);
    expect(loadPreviewZoom()).toBe(ctrl.getFactor());
  });

  it('setMobileMode re-applies current factor with mobile transform', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(1); // now at 1.1, desktop transform
    ctrl.setMobileMode(true);
    expect(iframe.style.transform).toBe('translate(-50%, -50%) scale(1.1)');
  });

  it('destroy clears the transform', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(1);
    ctrl.destroy();
    expect(iframe.style.transform).toBe('');
  });

  it('handles null iframe without throwing', () => {
    const ctrl = new PreviewZoomController(null);
    expect(() => ctrl.step(1)).not.toThrow();
    expect(() => ctrl.destroy()).not.toThrow();
  });

  // ── Persisted factor applied on construction (not just stored) ──────

  it('applies persisted non-default factor to iframe immediately on construction', () => {
    // Simulate app restart with a stored non-default zoom.
    savePreviewZoom(1.25);
    const iframe = makeIframe();
    // Before the fix the constructor only stored the factor but never called
    // applyNow(), so the iframe would stay at 100% until a keypress.
    const ctrl = new PreviewZoomController(iframe);
    expect(ctrl.getFactor()).toBe(1.25);
    // The transform must be applied immediately — not deferred to a keypress.
    expect(iframe.style.transform).toContain('scale(1.25)');
  });

  it('does NOT apply transform when persisted factor is default (1.0)', () => {
    // At default the transform should be cleared, not set to scale(1).
    const iframe = makeIframe();
    iframe.style.transform = 'leftover'; // simulate stale style
    const ctrl = new PreviewZoomController(iframe);
    expect(ctrl.getFactor()).toBe(PREVIEW_ZOOM_DEFAULT);
    // applyNow() is NOT called at default — iframe style is untouched.
    // (We do not clear a pre-existing transform in the constructor; that is
    // handled by destroy(). The key invariant is: no spurious scale(1.0) applied.)
    expect(iframe.style.transform).not.toContain('scale(1');
  });

  // ── Device-mode change re-applies zoom ─────────────────────────────

  it('setMobileMode re-applies zoom when switching from desktop to mobile', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.step(1); // 1.0 → 1.1, desktop transform applied
    expect(iframe.style.transform).toContain('scale(1.1)');
    expect(iframe.style.transformOrigin).toBe('0 0'); // desktop origin

    // User clicks the device-toggle pill → mode changes to mobile.
    ctrl.setMobileMode(true);
    // The zoom controller must re-apply with the mobile branch (translate+scale).
    expect(iframe.style.transform).toBe('translate(-50%, -50%) scale(1.1)');
    // The stale desktop top-left origin must be cleared so centering is correct.
    expect(iframe.style.transformOrigin).toBe('');
  });

  it('setMobileMode re-applies zoom when switching from mobile back to desktop', () => {
    const iframe = makeIframe();
    const ctrl = new PreviewZoomController(iframe);
    ctrl.setMobileMode(true);
    ctrl.step(1); // 1.0 → 1.1 in mobile mode
    expect(iframe.style.transform).toBe('translate(-50%, -50%) scale(1.1)');

    // Switch back to desktop.
    ctrl.setMobileMode(false);
    expect(iframe.style.transform).toContain('scale(1.1)');
    expect(iframe.style.transformOrigin).toBe('0 0');
  });
});
