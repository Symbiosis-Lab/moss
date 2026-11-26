import { describe, expect, it } from 'vitest';
import { DEFAULT_SPHERE_BOUNDS as bounds, sphereLegAt } from '../index.js';

describe('sphereLegAt', () => {
  it('clamps to the original print endpoints', () => {
    for (const p of [-1, 0]) expect(sphereLegAt(p)).toEqual({ ux: 0, uy: 1, k: 0, lift: 0, grain: 0 });
    for (const p of [1, 2]) expect(sphereLegAt(p)).toEqual({ ux: 1, uy: 0, k: 1, lift: 0, grain: 0 });
  });
  it('lifts monotonically, holds unmixed pigment, and releases monotonically', () => {
    let previous = 0;
    for (let i = 0; i <= 100; i++) {
      const p = bounds.outEnd + (bounds.holdStart - bounds.outEnd) * i / 100;
      const state = sphereLegAt(p);
      expect(state.lift).toBeGreaterThanOrEqual(previous);
      if (state.lift > 0) expect(state.grain).toBe(1);
      previous = state.lift;
    }
    expect(previous).toBe(1);
    for (let i = 0; i <= 100; i++) {
      const p = bounds.holdEnd + (bounds.inStart - bounds.holdEnd) * i / 100;
      const state = sphereLegAt(p);
      expect(state.lift).toBeLessThanOrEqual(previous);
      if (state.lift > 0) expect(state.grain).toBe(1);
      previous = state.lift;
    }
    expect(previous).toBe(0);
    expect(sphereLegAt(.5)).toMatchObject({ ux: .55, uy: .55, lift: 1, grain: 1 });
    expect(sphereLegAt(.5).k).toBeCloseTo(.5, 12);
  });
  it('is continuous at every phase and handover boundary', () => {
    const edges = [bounds.outEnd, (bounds.outEnd + bounds.holdStart) / 2, bounds.holdStart,
      bounds.holdEnd, (bounds.holdEnd + bounds.inStart) / 2, bounds.inStart];
    for (const p of edges) for (const key of ['ux', 'uy', 'k', 'lift', 'grain'] as const) {
      expect(sphereLegAt(p - 1e-8)[key]).toBeCloseTo(sphereLegAt(p + 1e-8)[key], 6);
    }
  });
  it('accepts ordered custom bounds and rejects degenerate phases', () => {
    const custom = { outEnd: .1, holdStart: .2, holdEnd: .8, inStart: .9 };
    expect(sphereLegAt(.05, custom).ux).toBeCloseTo(.5);
    expect(sphereLegAt(.95, custom).uy).toBeCloseTo(.5);
    expect(() => sphereLegAt(.5, { ...custom, holdStart: .1 })).toThrow(/bounds/);
    expect(() => sphereLegAt(NaN)).toThrow(/finite/);
  });
});
