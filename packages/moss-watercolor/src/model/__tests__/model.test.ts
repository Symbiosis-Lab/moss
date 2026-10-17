import { describe, expect, it } from 'vitest';
import { DEFAULT_PRESET } from '../../engine/preset.js';
import { DEFAULT_PHASE_BOUNDS, legAt, phaseOf, turnAt, type PhaseBounds } from '../index.js';

describe('phaseOf', () => {
  it('is "out" at p=0 and up to and including outEnd', () => {
    expect(phaseOf(0)).toBe('out');
    expect(phaseOf(0.1)).toBe('out');
    expect(phaseOf(DEFAULT_PHASE_BOUNDS.outEnd)).toBe('out');
  });

  it('is "mix" strictly between outEnd and inStart', () => {
    expect(phaseOf(DEFAULT_PHASE_BOUNDS.outEnd + 0.001)).toBe('mix');
    expect(phaseOf(0.5)).toBe('mix');
    expect(phaseOf(DEFAULT_PHASE_BOUNDS.inStart - 0.001)).toBe('mix');
  });

  it('is "in" from inStart through p=1', () => {
    expect(phaseOf(DEFAULT_PHASE_BOUNDS.inStart)).toBe('in');
    expect(phaseOf(0.9)).toBe('in');
    expect(phaseOf(1)).toBe('in');
  });

  it('honors custom bounds', () => {
    const bounds: PhaseBounds = { outEnd: 0.2, inStart: 0.8 };
    expect(phaseOf(0.2, bounds)).toBe('out');
    expect(phaseOf(0.21, bounds)).toBe('mix');
    expect(phaseOf(0.8, bounds)).toBe('in');
  });
});

describe('DEFAULT_PHASE_BOUNDS', () => {
  it('is the engine preset\'s own playOut/playIn', () => {
    expect(DEFAULT_PHASE_BOUNDS).toEqual({ outEnd: DEFAULT_PRESET.playOut, inStart: DEFAULT_PRESET.playIn });
  });
});

describe('legAt', () => {
  it('is the outgoing print at 0, the incoming at 1, and an even mix midway between symmetric bounds', () => {
    expect(legAt(0)).toEqual({ ux: 0, uy: 1, k: 0 });
    expect(legAt(1)).toEqual({ ux: 1, uy: 0, k: 1 });
    expect(legAt(0.5, { outEnd: 0.1, inStart: 0.9 })).toEqual({ ux: 1, uy: 1, k: 0.5 });
  });
});

describe('turnAt', () => {
  const sweep = Array.from({ length: 101 }, (_, i) => i / 100);
  // what is on screen: only k = 0 (x alone at ux) and k = 1 (y alone at uy) are single sides
  for (const bounds of [DEFAULT_PHASE_BOUNDS, { outEnd: 0.1, inStart: 0.9 }]) {
    it(`keeps the frame on screen at every p (bounds ${bounds.outEnd}/${bounds.inStart})`, () => {
      for (const p of sweep) {
        const before = legAt(p, bounds), turn = turnAt(p, bounds), after = legAt(turn.p, bounds);
        expect(after.k).toBe(0);
        if (turn.from === 'x') { expect(before.k).toBe(0); expect(after.ux).toBeCloseTo(before.ux, 12); }
        else if (turn.from === 'y') { expect(before.k).toBe(1); expect(after.ux).toBeCloseTo(before.uy, 12); }
        // the baked mix is a whole side, its dissolve not read; the leg resumes no later than the envelope it was baked at
        else { expect(before.k).toBeGreaterThan(0); expect(before.k).toBeLessThan(1); expect(turn.p).toBeLessThanOrEqual(Math.min(p, 1 - p)); }
      }
    });
  }
  it('refuses bounds that are not symmetric, where a handed-over side would not keep its envelope', () => {
    expect(() => turnAt(0.5, { outEnd: 0.3, inStart: 0.9 })).toThrow();
  });
});
