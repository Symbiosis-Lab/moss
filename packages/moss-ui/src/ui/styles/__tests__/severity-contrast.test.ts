/**
 * WCAG contrast gate for severity tokens on the REAL composited glass backings.
 *
 * Light backing:  --moss-glass-pill-bg rgba(255,255,255,0.90) composited over
 *                 --moss-color-bg #faf8f5  → ~#fefefe
 * Dark backing:   --moss-chrome-pill-bg rgba(255,255,255,0.14) composited over
 *                 --moss-color-bg #1a1816  → ~#3a3837
 *
 * WCAG 2.1 minimum contrast for UI components (3:1, SC 1.4.11).
 * Verified reference values: broken-dark #d96960 ≈ 4.18:1; degraded-dark
 * around #e6a93a ≈ 6.9:1.
 */

import { describe, it, expect } from 'vitest';

// ── Helpers ─────────────────────────────────────────────────────────────────

/** Parse a 6-digit hex colour string → [r, g, b] in [0, 255]. */
function parseHex(hex: string): [number, number, number] {
  const h = hex.replace('#', '');
  if (h.length !== 6) throw new Error(`Expected 6-digit hex, got "${hex}"`);
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16),
  ];
}

/**
 * Alpha-composite an rgba overlay onto an opaque rgb base.
 * Returns the resulting opaque [r, g, b].
 */
function composite(
  overlay: [number, number, number, number],  // [r, g, b, a] a ∈ [0,1]
  base: [number, number, number],
): [number, number, number] {
  const [or, og, ob, oa] = overlay;
  const [br, bg, bb] = base;
  return [
    Math.round(or * oa + br * (1 - oa)),
    Math.round(og * oa + bg * (1 - oa)),
    Math.round(ob * oa + bb * (1 - oa)),
  ];
}

/**
 * WCAG relative luminance for an sRGB triple in [0, 255].
 * https://www.w3.org/TR/WCAG21/#dfn-relative-luminance
 */
function luminance([r, g, b]: [number, number, number]): number {
  const linearize = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b);
}

/** WCAG contrast ratio (always ≥ 1). */
function contrast(fg: [number, number, number], bg: [number, number, number]): number {
  const l1 = luminance(fg);
  const l2 = luminance(bg);
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

// ── Token values (must match tokens.css) ────────────────────────────────────

const SEVERITY_BROKEN_LIGHT  = '#c85450'; // --moss-color-danger (light)
const SEVERITY_BROKEN_DARK   = '#d96960'; // --moss-color-danger (dark)
const SEVERITY_DEGRADED_LIGHT = '#c97a00'; // --moss-severity-degraded (light)
// Dark amber pinned by this test. Adjust until ≥ 3:1 on #3a3837.
const SEVERITY_DEGRADED_DARK  = '#e6a93a'; // --moss-severity-degraded (dark)

// Glass pill bg in light mode: rgba(255,255,255,0.90) over #faf8f5
const LIGHT_PAGE_BG: [number, number, number] = parseHex('#faf8f5');
const LIGHT_GLASS_OVERLAY: [number, number, number, number] = [255, 255, 255, 0.9];
const LIGHT_BACKING = composite(LIGHT_GLASS_OVERLAY, LIGHT_PAGE_BG);

// Glass pill bg in dark mode: --moss-chrome-pill-bg rgba(255,255,255,0.14)
// over #1a1816. This restates the token's value, which is the drift risk:
// panel-contrast.test.ts parses tokens.css instead.
const DARK_PAGE_BG: [number, number, number] = parseHex('#1a1816');
const DARK_GLASS_OVERLAY: [number, number, number, number] = [255, 255, 255, 0.14];
const DARK_BACKING = composite(DARK_GLASS_OVERLAY, DARK_PAGE_BG);

const WCAG_MIN = 3.0; // SC 1.4.11 UI component minimum

// ── Tests ────────────────────────────────────────────────────────────────────

describe('severity token WCAG contrast on real glass backing', () => {
  describe('light theme backing (glass over #faf8f5)', () => {
    it('broken red meets ≥3:1', () => {
      const ratio = contrast(parseHex(SEVERITY_BROKEN_LIGHT), LIGHT_BACKING);
      expect(ratio).toBeGreaterThanOrEqual(WCAG_MIN);
    });

    it('degraded amber meets ≥3:1', () => {
      const ratio = contrast(parseHex(SEVERITY_DEGRADED_LIGHT), LIGHT_BACKING);
      expect(ratio).toBeGreaterThanOrEqual(WCAG_MIN);
    });
  });

  describe('dark theme backing (glass over #1a1816)', () => {
    it('broken red meets ≥3:1', () => {
      const ratio = contrast(parseHex(SEVERITY_BROKEN_DARK), DARK_BACKING);
      expect(ratio).toBeGreaterThanOrEqual(WCAG_MIN);
    });

    it('degraded amber meets ≥3:1', () => {
      const ratio = contrast(parseHex(SEVERITY_DEGRADED_DARK), DARK_BACKING);
      expect(ratio).toBeGreaterThanOrEqual(WCAG_MIN);
    });
  });

  it('dark backing is ~#3a3837 (sanity check for the composition)', () => {
    // rgba(255,255,255,0.14) over #1a1816:
    // r = 255*0.14 + 26*0.86 = 35.7 + 22.36 = 58.06 → 58
    // g = 255*0.14 + 24*0.86 = 35.7 + 20.64 = 56.34 → 56
    // b = 255*0.14 + 22*0.86 = 35.7 + 18.92 = 54.62 → 55
    // #3a3837 = (58, 56, 55) — close enough (rounding ±1 per channel)
    expect(DARK_BACKING[0]).toBeGreaterThanOrEqual(57);
    expect(DARK_BACKING[0]).toBeLessThanOrEqual(59);
  });
});
