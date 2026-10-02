//#region src/engine/math.ts
const clamp01 = (v) => Math.min(1, Math.max(0, v));
const smooth = (a, b, t) => {
	const x = clamp01((t - a) / (b - a));
	return x * x * (3 - 2 * x);
};

//#endregion
//#region src/engine/preset.ts
const DEFAULT_PRESET = {
	dt: 1 / 120,
	stepsPerFrame: 36,
	tSplash: .2,
	tDry: 1.2,
	tTake: 1.2,
	tCure: .9,
	tTotal: 2.1,
	recDuration: 1.7,
	recEvery: 8,
	recStir: (t) => 1.5 * smooth(.25, .9, t),
	recOpen: (t) => smooth(.45, 1.35, t),
	recRain: (t) => .012 * smooth(.45, .7, t) * (1 - smooth(1.4, 1.7, t)),
	recFlood: (t) => smooth(1, 1.3, t),
	recRinseFrom: .9,
	recRinse: .02,
	recLight: .28,
	playOut: .42,
	playIn: .58
};
/** Derived from `recDuration`/`dt`/`recEvery`: never store these independently, or a preset override can silently desync the recorded step count from the frames actually kept. */
function recordingSteps(preset) {
	return Math.round(preset.recDuration / preset.dt);
}
function recordingFrameCount(preset) {
	return Math.ceil(recordingSteps(preset) / preset.recEvery) + 1;
}

//#endregion
export { smooth as a, clamp01 as i, recordingFrameCount as n, recordingSteps as r, DEFAULT_PRESET as t };