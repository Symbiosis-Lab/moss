import { o as smooth, t as clamp01 } from "./math-CZ65GQ6j.mjs";
import { t as DEFAULT_PRESET } from "./preset-JJvOfqsz.mjs";

//#region src/model/sphere.ts
const DEFAULT_SPHERE_BOUNDS = {
	outEnd: .18,
	holdStart: .34,
	holdEnd: .66,
	inStart: .82
};
/** Five phases; the host owns time through Out/Gather/Release/In and scroll through Hold.
* Sample both sides at the returned dissolves so the wash and grains share a frame.
* Grain reaches 1 before lift leaves 0, and lift returns to 0 before grain fades.
*/
function sphereLegAt(p, bounds = DEFAULT_SPHERE_BOUNDS) {
	const { outEnd, holdStart, holdEnd, inStart } = bounds;
	if (!Number.isFinite(p)) throw new RangeError("sphereLegAt: p must be finite");
	if (!(0 < outEnd && outEnd < holdStart && holdStart < holdEnd && holdEnd < inStart && inStart < 1)) throw new RangeError("sphereLegAt: bounds must be ordered strictly inside (0, 1)");
	p = clamp01(p);
	const gather = (outEnd + holdStart) / 2, release = (holdEnd + inStart) / 2;
	const up = smooth(gather, holdStart, p), down = smooth(holdEnd, release, p);
	const grain = smooth(outEnd, gather, p) * (1 - smooth(release, inStart, p));
	return {
		ux: p <= outEnd ? p / outEnd : 1 - .45 * smooth(outEnd, gather, p) + .45 * smooth(holdEnd, inStart, p),
		uy: p >= inStart ? (1 - p) / (1 - inStart) : 1 - .45 * smooth(outEnd, holdStart, p) + .45 * smooth(release, inStart, p),
		k: smooth(holdStart, holdEnd, p),
		lift: up * (1 - down),
		grain
	};
}

//#endregion
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
export { DEFAULT_SPHERE_BOUNDS as a, turnAt as i, legAt as n, sphereLegAt as o, phaseOf as r, DEFAULT_PHASE_BOUNDS as t };