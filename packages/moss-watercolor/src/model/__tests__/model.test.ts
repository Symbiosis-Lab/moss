import { describe, expect, it } from 'vitest';
import { DEFAULT_PRESET } from '../../engine/preset.js';
import { smooth } from '../../engine/math.js';
import { DEFAULT_PHASE_BOUNDS, phaseOf, retarget, type PhaseBounds } from '../index.js';

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

// What the engine's play() puts on screen at p: the outgoing scene's recording
// at p / outEnd, the incoming scene's at (1 - p) / (1 - inStart), weighted by
// smooth(outEnd, inStart, p) toward the incoming one.
function picture<T extends string>(from: T, to: T, p: number, { outEnd, inStart }: PhaseBounds) {
  const k = smooth(outEnd, inStart, p);
  return {
    [from]: { u: Math.min(1, p / outEnd), weight: 1 - k },
    [to]: { u: Math.min(1, (1 - p) / (1 - inStart)), weight: k },
  } as Record<T, { u: number; weight: number }>;
}

const ASYMMETRIC: PhaseBounds = { outEnd: 0.3, inStart: 0.6 };

describe('retarget', () => {
  it('in the outgoing phase, keeps the source and swaps only the destination', () => {
    expect(retarget('A', 'B', 0, 'C')).toEqual({ from: 'A', to: 'C', p: 0 });
    expect(retarget('A', 'B', 0.2, 'C')).toEqual({ from: 'A', to: 'C', p: 0.2 });
    expect(retarget('A', 'B', DEFAULT_PHASE_BOUNDS.outEnd, 'C')).toEqual({ from: 'A', to: 'C', p: DEFAULT_PHASE_BOUNDS.outEnd });
  });

  it('in the first half of the mix, keeps the source and swaps the destination', () => {
    expect(retarget('A', 'B', 0.45, 'C')).toEqual({ from: 'A', to: 'C', p: 0.45 });
  });

  it('in the second half of the mix, the old destination becomes the source, fully dispersed', () => {
    const result = retarget('A', 'B', 0.55, 'C');
    expect(result.from).toBe('B'); expect(result.to).toBe('C');
    expect(result.p).toBeCloseTo(DEFAULT_PHASE_BOUNDS.outEnd, 10);
  });

  it('splits the mix where the blend crosses one half, wherever the bounds put it', () => {
    // smooth() is symmetric, so the split is the midpoint of the mix window.
    const mid = (ASYMMETRIC.outEnd + ASYMMETRIC.inStart) / 2;
    expect(retarget('A', 'B', mid - 0.01, 'C', ASYMMETRIC)).toEqual({ from: 'A', to: 'C', p: mid - 0.01 });
    expect(retarget('A', 'B', mid + 0.01, 'C', ASYMMETRIC).from).toBe('B');
  });

  it('in the incoming phase, promotes the old destination to the source and flips p', () => {
    const { inStart } = DEFAULT_PHASE_BOUNDS;
    let result = retarget('A', 'B', inStart, 'C');
    expect(result.from).toBe('B'); expect(result.to).toBe('C'); expect(result.p).toBeCloseTo(1 - inStart, 10);
    result = retarget('A', 'B', 0.9, 'C');
    expect(result.from).toBe('B'); expect(result.to).toBe('C'); expect(result.p).toBeCloseTo(0.1, 10);
    expect(retarget('A', 'B', 1, 'C')).toEqual({ from: 'B', to: 'C', p: 0 });
  });

  it('in the incoming phase with asymmetric bounds, keeps the old destination at the recording position it was showing', () => {
    // The incoming side shows its recording at (1 - p) / (1 - inStart), and the
    // outgoing side reaches the same position at p' / outEnd.
    for (const p of [0.6, 0.7, 0.85, 1]) {
      const before = picture('A', 'B', p, ASYMMETRIC);
      const next = retarget('A', 'B', p, 'C', ASYMMETRIC);
      expect(next.from).toBe('B');
      expect(picture(next.from, next.to, next.p, ASYMMETRIC).B.u).toBeCloseTo(before.B.u, 10);
      expect(picture(next.from, next.to, next.p, ASYMMETRIC).B.weight).toBeCloseTo(1, 10);
    }
  });

  it('retargeting back to the origin plays the reverse transition from the same picture', () => {
    for (const bounds of [DEFAULT_PHASE_BOUNDS, ASYMMETRIC]) {
      for (const p of [0, 0.1, 0.25, 0.4, 0.5, 0.58, 0.7, 0.9, 1]) {
        const before = picture('A', 'B', p, bounds);
        const back = retarget('A', 'B', p, 'A', bounds);
        expect(back.from).toBe('B'); expect(back.to).toBe('A');
        const after = picture(back.from, back.to, back.p, bounds);
        for (const scene of ['A', 'B'] as const) {
          expect(after[scene].weight).toBeCloseTo(before[scene].weight, 10);
          if (before[scene].weight > 1e-9) expect(after[scene].u).toBeCloseTo(before[scene].u, 10);
        }
      }
    }
  });

  it('in the middle of a symmetric leg, going back is just 1 - p', () => {
    expect(retarget('A', 'B', 0.3, 'A').p).toBeCloseTo(0.7, 10);
    expect(retarget('A', 'B', 0.5, 'A').p).toBeCloseTo(0.5, 10);
  });

  it('honors custom bounds the same way phaseOf does', () => {
    const bounds: PhaseBounds = { outEnd: 0.2, inStart: 0.8 };
    expect(retarget('A', 'B', 0.45, 'C', bounds)).toEqual({ from: 'A', to: 'C', p: 0.45 });
    const result = retarget('A', 'B', 0.8, 'C', bounds);
    expect(result.from).toBe('B'); expect(result.to).toBe('C'); expect(result.p).toBeCloseTo(0.2, 10);
  });
});
