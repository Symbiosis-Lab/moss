// Moving a sheet between cell sizes. Resampling averages mass and thickness
// conservatively, recombines orientation and fibre radius by mass, and
// recomputes every derived field with the same relations. Coarse mode cannot
// drape fibres it cannot resolve, so its thickness comes from a density
// closure measured on fine, unpressed sheets of the same recipe at the same
// coarse cell size.
import { depositFibres } from './deposit.js';
import { deriveFields, type PaperSheet } from './fields.js';
import { normal, seededRandom, type Rng } from './random.js';
import { CELL_MATCH, isFine, latticeCell, type DensityClosure, type PaperRecipe } from './recipe.js';

export const MIN_DENSITY = 100; // kg/m³, a floor so an empty coarse cell never gets infinite thickness

function wholeFactor(from: number, to: number, w: number, h: number): number {
  const f = to / from, k = Math.round(f);
  if (k < 1 || Math.abs(f - k) > 1e-6 * f || w % k || h % k)
    throw new RangeError(`resampling ${from} m cells to ${to} m needs a whole factor dividing ${w}×${h}, got ${f}`);
  return k;
}

export function resamplePaper(sheet: PaperSheet, cell: number): PaperSheet {
  const k = wholeFactor(sheet.cell, cell, sheet.width, sheet.height);
  const W = sheet.width / k, H = sheet.height / k, n = W * H, inv = 1 / (k * k);
  const z = () => new Float32Array(n);
  const mass = z(), thickness = z(), oxx = z(), oxy = z(), oyy = z(), radiusMass = z();
  for (let y = 0; y < sheet.height; y++) for (let x = 0; x < sheet.width; x++) {
    const s = y * sheet.width + x, d = Math.floor(y / k) * W + Math.floor(x / k), m = sheet.grammage[s];
    mass[d] += m * inv; thickness[d] += sheet.thickness[s] * inv;
    oxx[d] += sheet.orientation.xx[s] * m * inv; oxy[d] += sheet.orientation.xy[s] * m * inv; oyy[d] += sheet.orientation.yy[s] * m * inv;
    radiusMass[d] += sheet.fibreRadius[s] * m * inv;
  }
  let fallback = 0; for (let i = 0; i < sheet.fibreRadius.length; i++) fallback += sheet.fibreRadius[i]; fallback /= sheet.fibreRadius.length;
  return deriveFields({ grid: { width: W, height: H, cell }, mass, thickness, oxx, oxy, oyy, radiusMass, contactAngle: sheet.contactAngle[0], orientationRadius: 0, fallbackRadius: fallback });
}

export function fitDensityClosure(recipe: PaperRecipe, coarseCell: number, seed = 1, coarseCells = 16): DensityClosure {
  const k = Math.max(1, Math.round(coarseCell / latticeCell(recipe))), fineCell = coarseCell / k, N = coarseCells * k;
  if (!isFine(recipe, fineCell))
    throw new RangeError(`a ${coarseCell} m cell must be within ${CELL_MATCH * 100}% of a whole multiple of the deposit lattice (${latticeCell(recipe)} m) to take a density closure`);
  const dep = depositFibres({ furnish: recipe.furnish, grammage: recipe.grammage, flocculation: recipe.flocculation, flexibility: recipe.flexibility, machineBias: recipe.machineBias, drape: true }, { width: N, height: N, cell: fineCell }, seededRandom(seed));
  const n = coarseCells * coarseCells, m = new Float64Array(n), t = new Float64Array(n);
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) { const d = Math.floor(y / k) * coarseCells + Math.floor(x / k); m[d] += dep.mass[y * N + x]; t[d] += dep.surface[y * N + x]; }
  let mBar = 0; for (let i = 0; i < n; i++) mBar += m[i]; mBar /= n;
  const rho = Array.from(m, (mi, i) => mi / Math.max(t[i], 1e-30)), g = Array.from(m, (mi) => mi / mBar - 1);
  let rBar = 0; for (const r of rho) rBar += r; rBar /= n;
  let sxy = 0, sxx = 0; for (let i = 0; i < n; i++) { sxy += g[i] * (rho[i] - rBar); sxx += g[i] * g[i]; }
  const slope = sxx > 0 ? sxy / sxx : 0;
  // What grammage leaves unexplained: how the top fibres happen to pile up.
  // In a cotton sheet it is over half the thickness variance at the engine's
  // texel, so dropping it flattens the relief by a third.
  const res = Array.from(m, (mi, i) => (mi > 0 ? (t[i] * Math.max(MIN_DENSITY, rBar + slope * g[i])) / mi - 1 : 0));
  let rm = 0; for (const r of res) rm += r; rm /= n;
  let v = 0, lag = 0;
  for (let y = 0; y < coarseCells; y++) for (let x = 0; x < coarseCells; x++) {
    const a = res[y * coarseCells + x] - rm;
    v += a * a;
    lag += a * (res[y * coarseCells + ((x + 1) % coarseCells)] - rm) + a * (res[((y + 1) % coarseCells) * coarseCells + x] - rm);
  }
  return { cell: coarseCell, mean: rBar, slope, residual: Math.sqrt(v / n), residualCorrelation: v > 0 ? lag / (2 * v) : 0 };
}

export function closureFor(recipe: PaperRecipe, cell: number): DensityClosure {
  const c = recipe.densityClosures.find((d) => Math.abs(d.cell - cell) <= CELL_MATCH * cell);
  if (!c) throw new RangeError(`no density closure for ${cell} m cells; fit one with fitDensityClosure(recipe, ${cell}) and add it to recipe.densityClosures`);
  return c;
}

/**
 * Zero-mean, unit-variance noise whose neighbouring cells correlate by `corr`:
 * white noise through a separable [b, 1, b] kernel, whose neighbour
 * correlation is 2b / (1 + 2b²). That reaches at most 1/√2, which covers
 * every residual measured, at sub-cell correlation lengths a Gaussian blur
 * cannot express.
 */
function correlatedNoise(width: number, height: number, corr: number, rng: Rng): Float32Array {
  const r = Math.min(Math.max(corr, 0), Math.SQRT1_2), b = r > 0 ? (1 - Math.sqrt(Math.max(0, 1 - 2 * r * r))) / (2 * r) : 0;
  const n = width * height, w = new Float32Array(n), h = new Float32Array(n), out = new Float32Array(n), norm = 1 + 2 * b * b;
  for (let i = 0; i < n; i++) w[i] = normal(rng);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++)
    h[y * width + x] = w[y * width + x] + b * (w[y * width + ((x + 1) % width)] + w[y * width + ((x + width - 1) % width)]);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++)
    out[y * width + x] = (h[y * width + x] + b * (h[((y + 1) % height) * width + x] + h[((y + height - 1) % height) * width + x])) / norm;
  return out;
}

export function closureThickness(mass: Float32Array, closure: DensityClosure, width: number, height: number, rng: Rng): Float32Array {
  let mBar = 0; for (let i = 0; i < mass.length; i++) mBar += mass[i]; mBar /= mass.length;
  const eta = correlatedNoise(width, height, closure.residualCorrelation, rng);
  const out = new Float32Array(mass.length);
  for (let i = 0; i < mass.length; i++)
    out[i] = (mass[i] / Math.max(MIN_DENSITY, closure.mean + closure.slope * (mBar > 0 ? mass[i] / mBar - 1 : 0))) * Math.max(0.2, 1 + closure.residual * eta[i]);
  return out;
}
