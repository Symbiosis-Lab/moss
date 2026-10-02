import { t as Paper } from "./default-C3ovSwFn.mjs";

//#region src/paper/recipe.d.ts
interface FibreType {
  name: string;
  /** Arithmetic mean of a lognormal length distribution, m. */
  lengthMean: number;
  lengthLogSd: number;
  /** Collapsed ribbon width, m. */
  width: number;
  /** Wall thickness, m. */
  wall: number;
  massFraction: number;
}
interface FeltSpec {
  fibre: FibreType;
  /** Felt batt mass deposited to form the felt surface, kg/m². */
  grammage: number;
  /** The wet sheet follows the felt only at scales above this, m. */
  conformity: number;
  /** RMS depth of the imprint, m. */
  imprintDepth: number;
}
interface PressSpec {
  /** Thickness multiplier from wet pressing, 0–1. */
  compaction: number;
  /** Surface smoothed below this length, m (0 = no polishing). */
  polishLength: number;
  /** How far toward the smoothed surface, 0–1. */
  polishStrength: number;
}
/** Coarse-mode density, kg/m³: mean + slope · (local grammage / mean grammage − 1), fitted at `cell` metres. */
interface DensityClosure {
  cell: number;
  /** Mean density, kg/m³, and its change per unit relative grammage. */
  mean: number;
  slope: number;
  /** Spread of the thickness grammage does not explain (relative), and its correlation between neighbouring cells. */
  residual: number;
  residualCorrelation: number;
}
interface PaperRecipe {
  furnish: FibreType[];
  /** kg/m² */
  grammage: number;
  /** Probability of accepting a fibre that lands on locally thin ground, (0, 1]. */
  flocculation: number;
  /** Niskanen–Alava T_f: height change allowed per fibre width of length, in fibre thicknesses. */
  flexibility: number;
  /** von Mises concentration of the doubled fibre angle around the machine direction (x); 0 = isotropic. */
  machineBias: number;
  felt: FeltSpec | null;
  press: PressSpec;
  /** Water contact angle, radians. */
  contactAngle: number;
  densityClosures: DensityClosure[];
}
declare const latticeCell: (recipe: PaperRecipe) => number;
declare const isFine: (recipe: PaperRecipe, cell: number) => boolean;
declare const PRESETS: {
  'cotton-rough': PaperRecipe;
  'cotton-cold-press': PaperRecipe;
  'cotton-hot-press': PaperRecipe;
  'xuan-unsized': PaperRecipe;
  'xuan-sized': PaperRecipe;
};
type PresetName = keyof typeof PRESETS;
declare const PRESET_TARGETS: Record<PresetName, {
  porosity: number;
  source: string;
}>;
//#endregion
//#region src/paper/deposit.d.ts
interface Grid {
  width: number;
  height: number;
  cell: number;
}
//#endregion
//#region src/paper/fields.d.ts
type Field = Float32Array;
interface Tensor {
  xx: Field;
  xy: Field;
  yy: Field;
}
interface PaperSheet {
  width: number;
  height: number;
  cell: number;
  grammage: Field;
  thickness: Field;
  porosity: Field;
  fibreRadius: Field;
  orientation: Tensor;
  permeability: Tensor;
  capillaryRadius: Field;
  entryPressure: Field;
  washburn: Field;
  contactAngle: Field;
}
/** Sheet porosity: 1 − total mass / (cellulose density · total thickness). */
declare function bulkPorosity(s: PaperSheet): number;
//#endregion
//#region src/paper/generate.d.ts
declare function validateGrid(grid: Grid): void;
declare function generatePaper(recipe: PaperRecipe, grid: Grid, seed: number): PaperSheet;
//#endregion
//#region src/paper/resample.d.ts
declare function resamplePaper(sheet: PaperSheet, cell: number): PaperSheet;
declare function fitDensityClosure(recipe: PaperRecipe, coarseCell: number, seed?: number, coarseCells?: number): DensityClosure;
declare function closureFor(recipe: PaperRecipe, cell: number): DensityClosure;
//#endregion
//#region src/paper/engine.d.ts
declare const CSS_PX: number;
declare const ENGINE_TEXEL: number;
/**
 * Each channel's physical span: byte 0 is `lo` and byte 255 is `hi`, so
 * whatever reads the texture can decode it, and two papers keep their
 * physical difference. Stretching each sheet to its own percentiles gave
 * rough and hot-press cotton the same relief.
 */
declare const ENGINE_SPANS: {
  /** Thickness about the sheet's mean, m. */
  readonly relief: {
    readonly lo: -0.0001;
    readonly hi: 0.0001;
  };
  /** Washburn coefficient, m/√s. */
  readonly absorbency: {
    readonly lo: 0;
    readonly hi: 0.032;
  };
  /** log10 of the mean in-plane permeability in m². */
  readonly fibre: {
    readonly lo: -12;
    readonly hi: -9;
  };
};
declare function toEngineChannels(sheet: PaperSheet): Paper;
declare function enginePaper(recipe: PaperRecipe, {
  texels,
  cssPxPerTexel,
  metresPerCssPx,
  seed
}?: {
  texels?: number | undefined;
  cssPxPerTexel?: number | undefined;
  metresPerCssPx?: number | undefined;
  seed?: number | undefined;
}): Paper;
//#endregion
export { PressSpec as C, PresetName as S, latticeCell as T, FeltSpec as _, toEngineChannels as a, PRESET_TARGETS as b, resamplePaper as c, Field as d, PaperSheet as f, DensityClosure as g, Grid as h, enginePaper as i, generatePaper as l, bulkPorosity as m, ENGINE_SPANS as n, closureFor as o, Tensor as p, ENGINE_TEXEL as r, fitDensityClosure as s, CSS_PX as t, validateGrid as u, FibreType as v, isFine as w, PaperRecipe as x, PRESETS as y };