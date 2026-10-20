// The three-phase leg, as pure functions of a position p in [0, 1]: a
// transition's outgoing scene plays its own recorded dispersion forward
// through [0, outEnd], the two washes mix as optical thickness through
// (outEnd, inStart), and the incoming scene consolidates by playing its own
// dispersion backward through [inStart, 1]. `legAt` gives what the engine's
// `playSides` draws at p, and `turnAt` turns a leg toward a third scene
// without a change on screen; neither needs the engine, so the phase logic
// is unit-tested directly.
import { DEFAULT_PRESET } from '../engine/preset.js';
import { clamp01, smooth } from '../engine/math.js';

export interface PhaseBounds {
  /** The outgoing side plays through this fraction of p. */
  outEnd: number;
  /** The incoming side starts consolidating from this fraction of p. */
  inStart: number;
}

/** The engine's default `playOut`/`playIn`, read from its preset so the two cannot drift apart. */
export const DEFAULT_PHASE_BOUNDS: PhaseBounds = { outEnd: DEFAULT_PRESET.playOut, inStart: DEFAULT_PRESET.playIn };

export type Phase = 'out' | 'mix' | 'in';

/** Which of the three phases `p` falls in. */
export function phaseOf(p: number, bounds: PhaseBounds = DEFAULT_PHASE_BOUNDS): Phase {
  if (p <= bounds.outEnd) return 'out';
  if (p < bounds.inStart) return 'mix';
  return 'in';
}

/** What `playSides` draws at `p`: the outgoing side's dissolve `ux`, the incoming side's `uy`, and the mix `k` between them. */
export function legAt(p: number, bounds: PhaseBounds = DEFAULT_PHASE_BOUNDS): { ux: number; uy: number; k: number } {
  const { outEnd, inStart } = bounds;
  return { ux: clamp01(p / outEnd), uy: clamp01((1 - p) / (1 - inStart)), k: smooth(outEnd, inStart, p) };
}

/**
 * Turns a leg toward a third scene from where it stands at `p`, so the frame on screen does not change. The new
 * leg's outgoing side is `from`, and it continues at the returned `p` toward the new scene:
 *
 * - `'x'` while nothing of the incoming side is on screen yet: the leg keeps its outgoing side and `p`.
 * - `'y'` while the incoming side gathers alone: it becomes the outgoing side, dissolving again from as far as it
 *   had gathered.
 * - `'mix'` in between: the host bakes the mix `legAt(p)` describes (`bake`, at `p`) and the leg starts from it,
 *   at its mixed end.
 *
 * The bounds must be symmetric (`outEnd = 1 - inStart`).
 */
export function turnAt(p: number, bounds: PhaseBounds = DEFAULT_PHASE_BOUNDS): { from: 'x' | 'y' | 'mix'; p: number } {
  const { outEnd, inStart } = bounds, phase = phaseOf(p, bounds);
  // the envelope is symmetric in p, so a side handed over at 1 - p keeps it only when the bounds are too
  if (Math.abs(outEnd - (1 - inStart)) > 1e-9) throw new Error(`turnAt: bounds must be symmetric (outEnd ${outEnd}, inStart ${inStart})`);
  if (phase === 'out') return { from: 'x', p };
  if (phase === 'in') return { from: 'y', p: (outEnd * (1 - p)) / (1 - inStart) };
  return { from: 'mix', p: outEnd };
}
