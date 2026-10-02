//#region src/model/index.d.ts
interface PhaseBounds {
  /** The outgoing side plays through this fraction of p. */
  outEnd: number;
  /** The incoming side starts consolidating from this fraction of p. */
  inStart: number;
}
/** The engine's default `playOut`/`playIn`, read from its preset so the two cannot drift apart. */
declare const DEFAULT_PHASE_BOUNDS: PhaseBounds;
type Phase = 'out' | 'mix' | 'in';
/** Which of the three phases `p` falls in. */
declare function phaseOf(p: number, bounds?: PhaseBounds): Phase;
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
declare function retarget<T>(from: T, to: T, p: number, next: T, bounds?: PhaseBounds): {
  from: T;
  to: T;
  p: number;
};
//#endregion
export { DEFAULT_PHASE_BOUNDS, Phase, PhaseBounds, phaseOf, retarget };