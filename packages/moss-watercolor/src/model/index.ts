// The three-phase leg, as pure functions of a position p in [0, 1]: a
// transition's outgoing scene plays its own recorded dispersion forward
// through [0, outEnd], the two washes mix as optical thickness through
// (outEnd, inStart), and the incoming scene consolidates by playing its own
// dispersion backward through [inStart, 1] -- so a transition "from A to B"
// read backward is the transition "from B to A" (at 1 - p when the bounds
// are symmetric, mapped through them otherwise: see `retarget`).
//
// This mirrors the engine's own `play(pair, fromFwd, p)` (engine/solver.ts),
// which computes the same three phases inline against its `playOut`/
// `playIn` preset fields; these functions exist so a caller can reason about
// -- and redirect -- a leg's phase without reaching into the engine, and so
// the three-phase logic itself is unit-tested directly.
import { DEFAULT_PRESET } from '../engine/preset.js';
import { smooth } from '../engine/math.js';

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

/**
 * Redirects an in-flight leg to `next` without a visual jump, in the same
 * terms the engine's `play()` draws it: the outgoing side shows its
 * recording at `p / outEnd`, the incoming side at `(1 - p) / (1 - inStart)`
 * (both clamped to [0, 1]), and the two are weighted by
 * `k = smooth(outEnd, inStart, p)`.
 *
 * - Outgoing phase (`k = 0`): only the source is on screen, so the source is
 *   kept and the destination swapped silently.
 * - Mix phase with `k < 0.5`: the source still dominates; it is kept and the
 *   destination swapped (the incoming half of the blend changes, which is the
 *   smaller part of the picture).
 * - Mix phase with `k >= 0.5`, or the incoming phase: the old destination
 *   dominates, so it becomes the source. Its recording is at the position
 *   `(1 - p) / (1 - inStart)` the incoming side was showing, which the
 *   outgoing side reaches at `outEnd * ((1 - p) / (1 - inStart))` (the full
 *   recording, `outEnd`, throughout the mix).
 *
 * Retargeting back to the leg's own source is the reverse transition: the
 * two ends swap and `p` is mapped so the same picture stays on screen (the
 * outgoing and incoming recordings trade places, and the blend weight
 * becomes `1 - k`).
 */
export function retarget<T>(from: T, to: T, p: number, next: T, bounds: PhaseBounds = DEFAULT_PHASE_BOUNDS): { from: T; to: T; p: number } {
  const { outEnd, inStart } = bounds;
  const phase = phaseOf(p, bounds);
  // How far the incoming side's recording has played back, 1 = fully dispersed.
  const incoming = Math.min(1, (1 - p) / (1 - inStart));
  if (next === from) {
    const reversed = phase === 'out' ? 1 - ((1 - inStart) * p) / outEnd : phase === 'mix' ? outEnd + inStart - p : outEnd * incoming;
    return { from: to, to: from, p: reversed };
  }
  if (phase === 'out' || (phase === 'mix' && smooth(outEnd, inStart, p) < 0.5)) return { from, to: next, p };
  return { from: to, to: next, p: outEnd * incoming };
}
