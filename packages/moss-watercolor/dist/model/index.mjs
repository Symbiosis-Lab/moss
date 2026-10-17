import { i as clamp01, l as smooth, t as DEFAULT_PRESET } from "../preset-RER7tjVe.mjs";

//#region src/model/index.ts
/** The engine's default `playOut`/`playIn`, read from its preset so the two cannot drift apart. */
const DEFAULT_PHASE_BOUNDS = {
	outEnd: DEFAULT_PRESET.playOut,
	inStart: DEFAULT_PRESET.playIn
};
/** Which of the three phases `p` falls in. */
function phaseOf(p, bounds = DEFAULT_PHASE_BOUNDS) {
	if (p <= bounds.outEnd) return "out";
	if (p < bounds.inStart) return "mix";
	return "in";
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
function retarget(from, to, p, next, bounds = DEFAULT_PHASE_BOUNDS) {
	const { outEnd, inStart } = bounds;
	const phase = phaseOf(p, bounds);
	const incoming = Math.min(1, (1 - p) / (1 - inStart));
	if (next === from) return {
		from: to,
		to: from,
		p: phase === "out" ? 1 - (1 - inStart) * p / outEnd : phase === "mix" ? outEnd + inStart - p : outEnd * incoming
	};
	if (phase === "out" || phase === "mix" && smooth(outEnd, inStart, p) < .5) return {
		from,
		to: next,
		p
	};
	return {
		from: to,
		to: next,
		p: outEnd * incoming
	};
}
/** What `playSides` draws at `p`: the outgoing side's dissolve `ux`, the incoming side's `uy`, and the mix `k` between them. */
function legAt(p, bounds = DEFAULT_PHASE_BOUNDS) {
	const { outEnd, inStart } = bounds;
	return {
		ux: clamp01(p / outEnd),
		uy: clamp01((1 - p) / (1 - inStart)),
		k: smooth(outEnd, inStart, p)
	};
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
function turnAt(p, bounds = DEFAULT_PHASE_BOUNDS) {
	const { outEnd, inStart } = bounds, phase = phaseOf(p, bounds);
	if (Math.abs(outEnd - (1 - inStart)) > 1e-9) throw new Error(`turnAt: bounds must be symmetric (outEnd ${outEnd}, inStart ${inStart})`);
	if (phase === "out") return {
		from: "x",
		p
	};
	if (phase === "in") return {
		from: "y",
		p: outEnd * (1 - p) / (1 - inStart)
	};
	return {
		from: "mix",
		p: outEnd
	};
}

//#endregion
export { DEFAULT_PHASE_BOUNDS, legAt, phaseOf, retarget, turnAt };