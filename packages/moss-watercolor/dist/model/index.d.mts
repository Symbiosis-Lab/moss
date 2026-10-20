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
/** What `playSides` draws at `p`: the outgoing side's dissolve `ux`, the incoming side's `uy`, and the mix `k` between them. */
declare function legAt(p: number, bounds?: PhaseBounds): {
  ux: number;
  uy: number;
  k: number;
};
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
declare function turnAt(p: number, bounds?: PhaseBounds): {
  from: 'x' | 'y' | 'mix';
  p: number;
};
//#endregion
export { DEFAULT_PHASE_BOUNDS, Phase, PhaseBounds, legAt, phaseOf, turnAt };