import { describe, expect, it } from 'vitest';
import { drain, endpointPresence, isDarkPage } from '../math.js';
import { DEFAULT_PRESET } from '../preset.js';

const m = DEFAULT_PRESET.endpointMargin;

describe('endpointPresence', () => {
  it('is 0 exactly at both ends of a leg', () => {
    expect(endpointPresence(0, m)).toBe(0);
    expect(endpointPresence(1, m)).toBe(0);
  });

  it('is 1 from the end of each margin through the middle', () => {
    for (const p of [m, 0.25, 0.5, 0.75, 1 - m]) expect(endpointPresence(p, m)).toBe(1);
  });

  it('rises monotonically over the first margin and falls over the last', () => {
    let prev = 0;
    for (let i = 1; i <= 100; i++) {
      const v = endpointPresence((i / 100) * m, m);
      expect(v).toBeGreaterThan(prev);
      prev = v;
    }
    prev = 0;
    for (let i = 1; i <= 100; i++) {
      const v = endpointPresence(1 - (i / 100) * m, m);
      expect(v).toBeGreaterThan(prev);
      prev = v;
    }
  });

  it('is symmetric, so a leg played backward looks the same', () => {
    for (const p of [0.01, 0.03, 0.05, 0.07]) expect(endpointPresence(p, m)).toBeCloseTo(endpointPresence(1 - p, m), 12);
  });

  it('stays 0 outside the leg', () => {
    expect(endpointPresence(-0.2, m)).toBe(0);
    expect(endpointPresence(1.2, m)).toBe(0);
  });

  it('without a usable margin shows the full look inside the leg and the print at its ends, never NaN', () => {
    for (const margin of [undefined, NaN, 0, -1] as unknown as number[]) {
      expect(endpointPresence(0, margin)).toBe(0);
      expect(endpointPresence(1, margin)).toBe(0);
      for (const p of [0.01, 0.5, 0.99]) expect(endpointPresence(p, margin)).toBe(1);
    }
  });
});

describe('drain', () => {
  const FLOORS = [0, 0.35, 0.8, 1];

  it('is 1 exactly at both ends of a leg, so the canvas there is the whole print', () => {
    for (const f of FLOORS) { expect(drain(0, f)).toBe(1); expect(drain(1, f)).toBe(1); }
  });

  it('reaches the floor at the middle of the leg and nowhere else', () => {
    for (const f of FLOORS) expect(drain(0.5, f)).toBeCloseTo(f, 12);
    expect(drain(0.45, 0.35)).toBeGreaterThan(0.35);
    expect(drain(0.55, 0.35)).toBeGreaterThan(0.35);
  });

  it('is symmetric, so a leg played backward looks the same', () => {
    for (let i = 0; i <= 50; i++) expect(drain(i / 100, 0.35)).toBeCloseTo(drain(1 - i / 100, 0.35), 12);
  });

  it('falls on the first half and rises on the second, with no flat stretch', () => {
    for (const f of [0, 0.35]) for (let i = 1; i <= 50; i++) {
      expect(drain(i / 100, f)).toBeLessThan(drain((i - 1) / 100, f));
      expect(drain(1 - i / 100, f)).toBeLessThan(drain(1 - (i - 1) / 100, f));
    }
  });

  it('has a continuous slope, zero at the middle, so the film turns round without a corner', () => {
    const h = 1e-4, slope = (p: number) => (drain(p + h, 0.35) - drain(p - h, 0.35)) / (2 * h);
    expect(Math.abs(slope(0.5))).toBeLessThan(1e-6);
    expect(slope(0.5 - 1e-3)).toBeCloseTo(slope(0.5 - 2e-3) / 2, 2);
    expect(slope(0.5 - 1e-3)).toBeLessThan(0);
    expect(slope(0.5 + 1e-3)).toBeGreaterThan(0);
    expect(slope(0.5 + 1e-3)).toBeCloseTo(-slope(0.5 - 1e-3), 6);
    // no jump in the slope anywhere along the leg
    let prev = slope(0.01);
    for (let i = 2; i < 99; i++) { const s = slope(i / 100); expect(Math.abs(s - prev)).toBeLessThan(0.2); prev = s; }
  });

  it('stays inside [floor, 1], is 1 outside the leg like its nearest end, and ignores a floor that is not a number', () => {
    for (const p of [-0.5, 1.5]) expect(drain(p, 0.35)).toBe(1);
    expect(drain(0.5, 2)).toBe(1);
    expect(drain(0.5, -1)).toBe(0);
    expect(drain(0.5, Number.NaN)).toBe(1);
    expect(drain(0.5, undefined as unknown as number)).toBe(1);
  });
});

describe('isDarkPage', () => {
  const hex = (h: string) => { const n = parseInt(h.slice(1), 16); return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]; };

  it('takes dark and light page colours for what they are', () => {
    for (const h of ['#000000', '#1d201e', '#202a44', '#3a1d1d']) expect(isDarkPage(hex(h))).toBe(true);
    for (const h of ['#ffffff', '#f4efe6', '#dfe6d8', '#c8d0ff']) expect(isDarkPage(hex(h))).toBe(false);
  });

  it('weighs green above blue: a saturated blue is dark, a saturated green is not', () => {
    expect(isDarkPage([0, 0, 1])).toBe(true);
    expect(isDarkPage([0, 1, 0])).toBe(false);
  });
});
