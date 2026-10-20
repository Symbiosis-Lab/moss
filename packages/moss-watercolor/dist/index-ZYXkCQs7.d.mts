import { t as Paper } from "./default-Cs3xymcD.mjs";

//#region src/engine/math.d.ts

type Colour = string | readonly [number, number, number];
//#endregion
//#region src/engine/preset.d.ts
interface WatercolorPreset {
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
  /** The share of a leg, at each end, over which refraction, pigment and paper clearing fade in from the plain print. */
  endpointMargin: number;
  /** With a transparent ground, the share of its pigment a leg still shows at the middle of it, in [0, 1]: 1 never thins it, 0 clears it entirely. Ignored on a paper ground. */
  drainFloor: number;
}
declare const DEFAULT_PRESET: WatercolorPreset;
//#endregion
//#region src/engine/solver.d.ts
interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
type PrintSource = HTMLImageElement | HTMLCanvasElement | HTMLVideoElement | ImageBitmap;
interface CreateSimOptions {
  canvas: HTMLCanvasElement;
  /** Composition pixels before `divisor`. */
  texW: number;
  texH: number;
  /** The simulation grid is texW/divisor x texH/divisor. */
  divisor: number;
  /** Where the canvas lies in its parent, in CSS px; read fresh every draw/playSides call. */
  rect: () => Rect;
  /** Defaults to `createPaper()`, the fixed 256x256 generated sheet; the shaders tile at that period, so any other size is refused. */
  paper?: Paper;
  /** Overrides merged onto {@link DEFAULT_PRESET}. */
  preset?: Partial<WatercolorPreset>;
  /** Splices one of `DIAG_VIEWS` into the display shader in place of the normal composite; 0 (the default) leaves the normal composite. */
  diagMode?: number;
  /** What lies under the canvas. `'paper'` (the default) draws the wash over the page colour read from `--bg`, as a layer that is clear only where nothing is inked on a light page and opaque on a dark one. `'transparent'` draws only pigment over nothing: the canvas is the print, paper included, at both ends of a leg, and its pigment thins toward `drainFloor` through the middle of it (`drain` in `engine/math.ts`), so whatever the host has behind it shows through the film. Light pages darken what is behind them, dark pages lighten it, by plain source-over. */
  ground?: 'paper' | 'transparent';
  /** The page colour the wash is drawn against: `#rgb`, `#rrggbb`, an opaque computed `rgb()`, or `[r, g, b]` in sRGB 0 to 1 (`parseColour`). Defaults to the `--bg` custom property on the document root, read once here; anything unparseable is white. A host whose colours live elsewhere passes it, and changes it later with `setTint`. Its luma below 0.5 draws the wash the other way round. */
  tint?: Colour;
}
interface StepOptions {
  /** Scales the opening splash (0: the water arrives without one). */
  splash?: number;
  /** Scales the film's mixing toward its neighbourhood's and whole film's colour (0: only diffusion between neighbours). */
  mix?: number;
  /** The share of suspended pigment lost to the water's depth per step. */
  dilute?: number;
  /** Scales the small eddies the pigment rides. */
  curl?: number;
  /** How fast the picture-sized billows carry the pigment, in grid cells a step. */
  flow?: number;
  /** The billows' smallest wavelength, in grid cells. */
  flowSize?: number;
  /** How much faster dense colour runs apart than thin (0: as fast). */
  bloom?: number;
  /** A second ring of neighbours this many cells off, so the colour reaches further a step (1: none). */
  reach?: number;
  /** The least the pigment diffuses, as a share of the most (0: only as fast as the film moves). */
  diffuse?: number;
  standing?: boolean;
  open?: number;
  rain?: number;
  rinse?: number;
  light?: number;
  dose?: number;
  lift?: number;
}
/** How a scene side's ink lets go. `stir`, `open`, `rain` and `lift` scale the preset's recording schedule (1 leaves it); `liftRamp` is the share of the recording over which the letting go comes up from 15%; `flow` comes up over the first 30% of the recording, and `dilute` is reached by its end from `diluteFrom`; the rest are the `StepOptions` of the same name, applied to every recorded step. */
interface SideLook {
  stir?: number;
  open?: number;
  rain?: number;
  lift?: number;
  liftRamp?: number;
  splash?: number;
  mix?: number;
  curl?: number;
  dilute?: number;
  diluteFrom?: number;
  diffuse?: number;
  flow?: number;
  flowSize?: number;
  bloom?: number;
  reach?: number;
}
/** One scene's dispersion, recorded over `foot`'s footprint. Its recording state is the engine's own. */
interface SceneSide {
  readonly print: PrintSource;
  readonly foot: PrintSource;
  readonly look: SideLook;
}
/** `print` is the plain print the frame showed when it was baked, and `at` where its envelope stood, folded below the middle: a leg continuing from it holds that envelope until it catches up. */
interface BakedSide {
  baked: true;
  A: WebGLTexture | null;
  fb: WebGLFramebuffer;
  print: PrintSource;
  at: number;
}
interface WatercolorSim {
  dispose(): void;
  setPrints(src: PrintSource, tgt: PrintSource): void;
  reset(): void;
  step(fwd: boolean, t: number, cure: number, stir?: number, tilt?: number, relift?: number, options?: StepOptions): void;
  probe(x: number, y: number): number[][];
  /** `p`, if given, is the position through the leg (0 the outgoing print, 1 the incoming): at either end the canvas shows that print exactly, and the wash's own look fades in over `endpointMargin`. Left out, the full look is drawn. */
  draw(fwd: boolean, cure: number, clearance?: number, p?: number): void;
  /** Changes the tint the wash is drawn against (a theme toggle). A change of more than 1/255 in any channel makes every side recorded so far count as unrecorded: `sideRecorded()` is false, `playSides()` draws nothing from it and `recordSide()` starts it again, since its pigment was measured against the old colour. A smaller change is ignored. A leg being played cannot be finished across it; hand off to the host's own page first. */
  setTint(colour: Colour): void;
  /** Adds a `webglcontextlost` handler; returns an unsubscribe function. The factory listens on the canvas only once the first handler is added, and from then on cancels the event's default action (which is what lets the context be restored). A page may also attach its own listener to the canvas directly. */
  onContextLost(handler: (event: Event) => void): () => void;
  /** One scene's own dispersion, recorded over `foot`'s footprint: a print covering every scene it may mix with. Its options shape the recording: `dose` charges the print's colour, and `rinse` is the flush per step once the wash is mixed, while its mean pigment is above `light`. */
  sceneSide(print: PrintSource, foot: PrintSource, options?: {
    dose?: number;
    rinse?: number;
    light?: number;
    look?: SideLook;
  }): SceneSide;
  /** Records up to `budget` steps of a scene side. One side records at a time: the sides share the simulation's state. */
  recordSide(side: SceneSide, budget: number): void;
  sideRecorded(side: SceneSide): boolean;
  /** `x` at dissolve `ux` (0 its print, 1 its mixed wash), mixed by `k` into `y` at `uy`; `p` in [0, 1] places the leg's envelope, so at either end the canvas is that end's plain print. A `y` with nothing recorded yet adds nothing to the mix, but the leg still ends on its print. `toNothing`, on a transparent ground, drains the canvas clear from the middle of the leg on, so the host shows its own incoming page; ignored on a paper ground. */
  playSides(x: SceneSide | BakedSide, ux: number, y: SceneSide | null, uy: number, k: number, p: number, options?: {
    toNothing?: boolean;
  }): boolean;
  /** The mix `playSides(x, ux, y, uy, k, p)` draws, kept as optical thickness: a side at its mixed end, to turn a transition toward another scene from where it stands. Played as `x` from at most `at` (`turnAt` resumes there), the frame is unchanged. */
  bake(x: SceneSide | BakedSide, ux: number, y: SceneSide | null, uy: number, k: number, p: number): BakedSide;
  freeSide(side: SceneSide | BakedSide): void;
}
declare function createSim(opts: CreateSimOptions): WatercolorSim | null;
//#endregion
//#region src/engine/host.d.ts
interface ClockState {
  /** Sim-seconds already stepped toward the last goal. */
  t: number;
  /** The `t` last painted, or -1 if nothing has been painted yet. */
  drawn: number;
}
interface AdvanceResult {
  /** Whether `t` reached within one step of `goal` (the sim is as far along as this frame's goal asks). */
  caughtUp: boolean;
  /** Steps actually spent this call. */
  count: number;
}
declare function advance(sim: Pick<WatercolorSim, 'reset' | 'step'>, state: ClockState, goal: number, fwd: boolean, stir: number, tilt: number, relift: number, paint: (t: number) => void, preset?: WatercolorPreset): AdvanceResult;
//#endregion
//#region src/engine/shaders.d.ts
declare const DIAG_VIEWS: Record<number, string>;
//#endregion
export { BakedSide as a, Rect as c, StepOptions as d, WatercolorSim as f, WatercolorPreset as h, advance as i, SceneSide as l, DEFAULT_PRESET as m, AdvanceResult as n, CreateSimOptions as o, createSim as p, ClockState as r, PrintSource as s, DIAG_VIEWS as t, SideLook as u };