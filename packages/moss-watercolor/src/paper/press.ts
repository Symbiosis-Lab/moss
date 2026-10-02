// Couching onto felt, then wet pressing. The felt is deposited with the same
// fibre code, low-passed at the scale the wet sheet can follow, and pressed
// into the sheet's top as a dent where the felt stands high. Pressing then
// compacts the sheet and, for hot-press, smooths its top surface.
import { depositFibres, type Grid } from './deposit.js';
import { gaussianBlurWrap } from './filter.js';
import type { FeltSpec, PressSpec } from './recipe.js';
import { seededRandom, type Rng } from './random.js';

const MIN_REMAINING = 0.2; // an imprint never cuts a cell below this share of its thickness

const REFERENCE_CELLS = 1024, REFERENCE_SEED = 0x9e3779b9;

/** Relative felt grammage, low-passed at the scale the wet sheet can follow. */
function feltRelief(felt: FeltSpec, grid: Grid, rng: Rng): Float32Array {
  const dep = depositFibres({ furnish: [felt.fibre], grammage: felt.grammage, flocculation: 1, flexibility: 2, machineBias: 0, drape: false }, grid, rng);
  const low = gaussianBlurWrap(dep.mass, grid.width, grid.height, felt.conformity / grid.cell);
  let m = 0; for (let i = 0; i < low.length; i++) m += low[i]; m /= low.length;
  for (let i = 0; i < low.length; i++) low[i] = m > 0 ? low[i] / m - 1 : 0;
  return low;
}

/**
 * The felt's relief, in units of its RMS at the conformity scale, so an
 * imprint depth is one physical dent whatever the cell size. Rescaling each
 * grid to unit RMS made a coarse felt 1.8 times deeper than a fine felt
 * averaged to the same cells, since averaging lowers the RMS and the
 * rescale put it back. The scale is measured once on a fine tile.
 */
export function feltSurface(felt: FeltSpec, grid: Grid, rng: Rng): Float32Array {
  const sd = referenceScale(felt), out = feltRelief(felt, grid, rng);
  for (let i = 0; i < out.length; i++) out[i] /= sd;
  return out;
}

const scales = new Map<string, number>();
function referenceScale(felt: FeltSpec): number {
  const key = JSON.stringify([felt.fibre, felt.grammage, felt.conformity]);
  let sd = scales.get(key);
  if (sd === undefined) {
    const cell = Math.min(felt.conformity, felt.fibre.width) / 2;
    const ref = feltRelief(felt, { width: REFERENCE_CELLS, height: REFERENCE_CELLS, cell }, seededRandom(REFERENCE_SEED));
    let v = 0; for (let i = 0; i < ref.length; i++) v += ref[i] * ref[i];
    sd = Math.sqrt(v / ref.length) || 1;
    if (scales.size > 64) scales.clear();
    scales.set(key, sd);
  }
  return sd;
}

export function pressSheet(initial: Float32Array, felt: Float32Array | null, imprintDepth: number, press: PressSpec, grid: Grid): Float32Array {
  const out = new Float32Array(initial.length);
  for (let i = 0; i < out.length; i++) {
    const dent = felt ? imprintDepth * felt[i] : 0;
    out[i] = Math.max(initial[i] * MIN_REMAINING, initial[i] - dent) * press.compaction;
  }
  if (press.polishStrength > 0 && press.polishLength > 0) {
    const smooth = gaussianBlurWrap(out, grid.width, grid.height, press.polishLength / grid.cell);
    for (let i = 0; i < out.length; i++) out[i] += press.polishStrength * (smooth[i] - out[i]);
  }
  return out;
}
