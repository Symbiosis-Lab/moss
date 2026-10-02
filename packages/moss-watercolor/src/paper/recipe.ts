// What a sheet is made from and how it is finished, in SI units (metres,
// kilograms, radians). Values and their sources are in the paper-generator
// design spec; "est." values have no citable source yet and are meant to be
// replaced by measurements of real sheets.
export const CELLULOSE_DENSITY = 1500; // kg/m³, the fibre wall

export interface FibreType {
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
export interface FeltSpec {
  fibre: FibreType;
  /** Felt batt mass deposited to form the felt surface, kg/m². */
  grammage: number;
  /** The wet sheet follows the felt only at scales above this, m. */
  conformity: number;
  /** RMS depth of the imprint, m. */
  imprintDepth: number;
}
export interface PressSpec {
  /** Thickness multiplier from wet pressing, 0–1. */
  compaction: number;
  /** Surface smoothed below this length, m (0 = no polishing). */
  polishLength: number;
  /** How far toward the smoothed surface, 0–1. */
  polishStrength: number;
}
/** Coarse-mode density, kg/m³: mean + slope · (local grammage / mean grammage − 1), fitted at `cell` metres. */
export interface DensityClosure {
  cell: number;
  /** Mean density, kg/m³, and its change per unit relative grammage. */
  mean: number; slope: number;
  /** Spread of the thickness grammage does not explain (relative), and its correlation between neighbouring cells. */
  residual: number; residualCorrelation: number;
}
export interface PaperRecipe {
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

export const coarseness = (f: FibreType): number => 2 * f.width * f.wall * CELLULOSE_DENSITY;
export const fibreThickness = (f: FibreType): number => 2 * f.wall;
export const minFibreWidth = (r: PaperRecipe): number => Math.min(...r.furnish.map((f) => f.width));
// Draping is a lattice model, and its thickness depends on the cell at first
// order: any grid places a fibre's edges only to within a cell, and a 300 g/m²
// cotton sheet comes out 500, 620, 670 and 700 µm thick at a half, quarter,
// eighth and sixteenth of the fibre width. Niskanen–Alava and KCL-PAKKA fix
// the cell relative to the fibre (one to three cells across) and calibrate
// flexibility there; this does the same, so a finer cell is refused rather
// than quietly building a thicker sheet.
export const latticeCell = (recipe: PaperRecipe): number => minFibreWidth(recipe) / 2;
/** Two cell sizes within this share of each other are the same cell. */
export const CELL_MATCH = 0.05;

export const isFine = (recipe: PaperRecipe, cell: number): boolean => Math.abs(cell - latticeCell(recipe)) <= CELL_MATCH * latticeCell(recipe);
/** A furnish property averaged by mass fraction. */
export const furnishMean = (r: PaperRecipe, of: (f: FibreType) => number): number => r.furnish.reduce((s, f) => s + f.massFraction * of(f), 0);
export const meanFibreLength = (r: PaperRecipe): number => furnishMean(r, (f) => f.lengthMean);
export const meanFibreRadius = (r: PaperRecipe): number => furnishMean(r, (f) => f.width / 2);

const deg = (d: number) => (d * Math.PI) / 180;

export const COTTON: FibreType = { name: 'cotton', lengthMean: 1.5e-3, lengthLogSd: 0.4, width: 20e-6, wall: 4e-6, massFraction: 1 };
export const PTEROCELTIS: FibreType = { name: 'pteroceltis', lengthMean: 2.45e-3, lengthLogSd: 0.25, width: 11e-6, wall: 2.5e-6, massFraction: 0.7 };
export const RICE_STRAW: FibreType = { name: 'rice-straw', lengthMean: 1.2e-3, lengthLogSd: 0.5, width: 10e-6, wall: 2e-6, massFraction: 0.3 };
/** Felt batt fibre, solid round: wall = width / 2 makes thickness equal width. */
export const FELT_FIBRE: FibreType = { name: 'felt', lengthMean: 5e-3, lengthLogSd: 0.3, width: 30e-6, wall: 15e-6, massFraction: 1 };

const COTTON_CLOSURE: DensityClosure = { cell: 0.0005291666666666666, mean: 421.29, slope: 170.82, residual: 0.0287, residualCorrelation: 0.406 };
const XUAN_CLOSURES: Record<'unsized' | 'sized', DensityClosure> = {
  unsized: { cell: 0.0005291666666666666, mean: 365.81, slope: -99.31, residual: 0.0446, residualCorrelation: 0.183 },
  sized: { cell: 0.0005291666666666666, mean: 359.16, slope: -85.39, residual: 0.0441, residualCorrelation: 0.183 },
};
// Fitted on a seed and grid fixed by fit.ts (FIT_PAPER=1 prints these). The
// closures are for the engine's texel, two CSS pixels (ENGINE_TEXEL).
// fitted: conformity so the felt imprint centroid wavelength is near 1 mm (est.);
// the centroid steps from 0.84 to 1.25 mm across 0.0387 mm, and this is the nearer side
const cottonFelt = (imprintDepth: number): FeltSpec => ({ fibre: FELT_FIBRE, grammage: 0.2, conformity: 0.0387e-3, imprintDepth });
const cotton = (felt: FeltSpec, press: PressSpec): PaperRecipe => ({
  // fitted: flexibility so cold-press porosity is 0.65 (mould-made rag 0.52 g/cm³)
  furnish: [COTTON], grammage: 0.3, flocculation: 0.6, flexibility: 0.8235, machineBias: 0,
  felt, press, contactAngle: deg(85), densityClosures: [COTTON_CLOSURE],
});
const xuan = (grammage: number, compaction: number, contactAngle: number, closure: DensityClosure): PaperRecipe => ({
  // fitted: flexibility so unsized porosity is 0.755 (Shao et al. 2019)
  furnish: [PTEROCELTIS, RICE_STRAW], grammage, flocculation: 0.6, flexibility: 0.4197, machineBias: 0,
  felt: null, press: { compaction, polishLength: 0, polishStrength: 0 }, contactAngle, densityClosures: [closure],
});

export const PRESETS = {
  // fitted: compaction so porosity is 0.68 (est.)
  'cotton-rough': cotton(cottonFelt(60e-6), { compaction: 0.875, polishLength: 0, polishStrength: 0 }),
  'cotton-cold-press': cotton(cottonFelt(30e-6), { compaction: 0.8, polishLength: 0.2e-3, polishStrength: 0.3 }),
  // fitted: compaction so porosity is 0.6 (est.)
  'cotton-hot-press': cotton(cottonFelt(8e-6), { compaction: 0.7, polishLength: 1e-3, polishStrength: 0.9 }),
  'xuan-unsized': xuan(0.0323, 1, 0, XUAN_CLOSURES.unsized),
  // fitted: compaction so porosity is 0.664 (Shao et al. 2019)
  'xuan-sized': xuan(0.0343, 0.7163, deg(85.2), XUAN_CLOSURES.sized),
} satisfies Record<string, PaperRecipe>;
export type PresetName = keyof typeof PRESETS;

export const PRESET_TARGETS: Record<PresetName, { porosity: number; source: string }> = {
  'cotton-rough': { porosity: 0.68, source: 'est.: looser than cold-press, which is unpressed felt-dried' },
  'cotton-cold-press': { porosity: 0.65, source: 'mould-made cotton rag at 0.52 g/cm³: 1 − 0.52/1.5' },
  'cotton-hot-press': { porosity: 0.6, source: 'est.: denser than cold-press' },
  'xuan-unsized': { porosity: 0.755, source: 'Shao et al. 2019, RSC Advances 9(69)' },
  'xuan-sized': { porosity: 0.664, source: 'Shao et al. 2019, RSC Advances 9(69)' },
};
