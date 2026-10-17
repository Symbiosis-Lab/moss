import { i as clamp01, l as smooth, t as DEFAULT_PRESET } from "../preset-CbPMJNtm.mjs";

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
export { DEFAULT_PHASE_BOUNDS, legAt, phaseOf, turnAt };