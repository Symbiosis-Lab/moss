import { describe, expect, it, vi } from 'vitest';
import { advance, type ClockState } from '../host.js';
import { DEFAULT_PRESET, type WatercolorPreset } from '../preset.js';

function fakeSim() {
  return { reset: vi.fn(), step: vi.fn() };
}

describe('advance', () => {
  it('steps the clock forward in dt increments toward goal, within budget', () => {
    const sim = fakeSim();
    const state: ClockState = { t: 0, drawn: -1 };
    const paint = vi.fn();
    const preset: WatercolorPreset = { ...DEFAULT_PRESET, stepsPerFrame: 1000 };
    const goal = 0.01;
    const result = advance(sim, state, goal, true, 0, 0, 0, paint, preset);
    expect(result.caughtUp).toBe(true);
    expect(state.t).toBeGreaterThanOrEqual(goal - preset.dt);
    expect(sim.step).toHaveBeenCalled();
    expect(paint).toHaveBeenCalledTimes(1);
    expect(paint).toHaveBeenCalledWith(state.t);
  });

  it('stops advancing at most one dt past preset.tTotal even if goal is further out', () => {
    // The loop checks `t < tTotal` before stepping, then adds dt, so the last
    // step that started just under tTotal can land up to one dt past it.
    const sim = fakeSim();
    const state: ClockState = { t: 0, drawn: -1 };
    const preset: WatercolorPreset = { ...DEFAULT_PRESET, stepsPerFrame: 100000 };
    advance(sim, state, preset.tTotal + 5, true, 0, 0, 0, () => {}, preset);
    expect(state.t).toBeLessThanOrEqual(preset.tTotal + preset.dt + 1e-9);
  });

  it('spends at most stepsPerFrame steps in one call, so a distant goal needs several calls', () => {
    const sim = fakeSim();
    const state: ClockState = { t: 0, drawn: -1 };
    const preset: WatercolorPreset = { ...DEFAULT_PRESET, stepsPerFrame: 5 };
    const goal = 1.0; // far beyond what 5 steps of dt=1/120 can reach
    const first = advance(sim, state, goal, true, 0, 0, 0, () => {}, preset);
    expect(first.count).toBe(5);
    expect(first.caughtUp).toBe(false);
    expect(sim.step).toHaveBeenCalledTimes(5);
  });

  it('paints only once per newly reached t, not again on a repeated call at the same goal', () => {
    const sim = fakeSim();
    const state: ClockState = { t: 0, drawn: -1 };
    const paint = vi.fn();
    const preset: WatercolorPreset = { ...DEFAULT_PRESET, stepsPerFrame: 1000 };
    const goal = 0.01;
    advance(sim, state, goal, true, 0, 0, 0, paint, preset);
    advance(sim, state, goal, true, 0, 0, 0, paint, preset);
    expect(paint).toHaveBeenCalledTimes(1);
  });

  it('resets and restarts from zero when the goal moves back more than one step', () => {
    const sim = fakeSim();
    const state: ClockState = { t: 0.5, drawn: 0.5 };
    const preset: WatercolorPreset = { ...DEFAULT_PRESET, stepsPerFrame: 1000 };
    advance(sim, state, 0.01, true, 0, 0, 0, () => {}, preset);
    expect(sim.reset).toHaveBeenCalledTimes(1);
    expect(state.t).toBeGreaterThanOrEqual(0);
    expect(state.t).toBeLessThan(0.5);
  });

  it('passes fwd, stir, tilt and relift through to every step call', () => {
    const sim = fakeSim();
    const state: ClockState = { t: 0, drawn: -1 };
    const preset: WatercolorPreset = { ...DEFAULT_PRESET, stepsPerFrame: 3 };
    advance(sim, state, 1, false, 0.4, 0.2, 0.1, () => {}, preset);
    expect(sim.step).toHaveBeenCalledTimes(3);
    for (const call of sim.step.mock.calls) {
      const [fwd, , , stir, tilt, relift] = call;
      expect(fwd).toBe(false);
      expect(stir).toBe(0.4);
      expect(tilt).toBe(0.2);
      expect(relift).toBe(0.1);
    }
  });
});
