// Every paper-and-fluid timing constant the solver and its recording-and-
// playback methods read, in one place. `createSim()` reads
// `opts.preset ?? DEFAULT_PRESET` instead of each constant being its own
// hardcoded literal, so a caller tuning the physics overrides this object
// rather than hunting through the solver for the value to change. Field
// names are the lower-camel form of the all-caps constants the solver used
// to hold (`tSplash` was `T_SPLASH`, `recStir` was `REC_STIR`, ...), so a
// search for either spelling finds the other.
//
// `tCure` and `tTotal` are not read inside the solver itself (the caller
// computes the cure value `step()` takes); they are here because the host
// utility that paces a sim's clock toward a goal (`advance`, in
// `engine/host.ts`) needs the same numbers, and one preset is the point.
import { smooth } from './math.js';

export interface WatercolorPreset {
  /** One simulation step, in paper-time seconds. */
  dt: number;
  /** The most steps a single frame may spend recording a dispersion. */
  stepsPerFrame: number;
  /** When the first splash drops land. */
  tSplash: number;
  /** When the sheet starts drying (a hot-air blast: faster evaporation, rim current, settling). */
  tDry: number;
  /** When the sheet starts taking up pigment. */
  tTake: number;
  /** Host utility only: when the cure (the clock's own, not a recording's) begins. */
  tCure: number;
  /** Host utility only: the clock's full span. */
  tTotal: number;
  /** A recorded dispersion's length, in paper-time seconds. */
  recDuration: number;
  /** Keep a frame every this many recorded steps. */
  recEvery: number;
  /** Stirring applied while recording, as a function of the recording's own progress (0 at rest, 1 at `recDuration`). */
  recStir: (t: number) => number;
  /** How far the sheet opens toward the whole paper while recording. */
  recOpen: (t: number) => number;
  /** Plain water added evenly while recording. */
  recRain: (t: number) => number;
  /** A flooded recording's (the title's) water-over-everything curve. */
  recFlood: (t: number) => number;
  /** From this fraction of the recording, a wash lighter than `recLight` is left alone; a wash still darker is flushed. */
  recRinseFrom: number;
  /** The flush rate once `recRinseFrom` is reached. */
  recRinse: number;
  /** The suspended-pigment-per-wet-cell darkness a recorded wash is flushed down to. */
  recLight: number;
  /** A transition's outgoing side plays through this fraction of the position. */
  playOut: number;
  /** A transition's incoming side gathers from this fraction of the position. */
  playIn: number;
}

export const DEFAULT_PRESET: WatercolorPreset = {
  dt: 1 / 120,
  stepsPerFrame: 36,
  tSplash: 0.2,
  tDry: 1.2,
  tTake: 1.2,
  tCure: 0.9,
  tTotal: 2.1,
  recDuration: 1.7,
  recEvery: 8,
  recStir: (t) => 1.5 * smooth(.25, .9, t),
  recOpen: (t) => smooth(.45, 1.35, t),
  recRain: (t) => .012 * smooth(.45, .7, t) * (1 - smooth(1.4, 1.7, t)),
  recFlood: (t) => smooth(1, 1.3, t),
  recRinseFrom: 0.9,
  recRinse: 0.02,
  recLight: 0.28,
  playOut: 0.42,
  playIn: 0.58,
};

/** Derived from `recDuration`/`dt`/`recEvery`: never store these independently, or a preset override can silently desync the recorded step count from the frames actually kept. */
export function recordingSteps(preset: WatercolorPreset): number {
  return Math.round(preset.recDuration / preset.dt);
}
export function recordingFrameCount(preset: WatercolorPreset): number {
  return Math.ceil(recordingSteps(preset) / preset.recEvery) + 1;
}
