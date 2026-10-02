// A sheet from a recipe: deposit, felt, press, derive. Fibres drape on one
// lattice, half the narrowest fibre's width. Coarser cells cannot drape them,
// so their thickness comes from a density closure fitted from lattice runs
// (see resample.ts); finer cells are refused.
import { depositFibres, type Grid } from './deposit.js';
import { deriveFields, type PaperSheet } from './fields.js';
import { feltSurface, pressSheet } from './press.js';
import { seededRandom } from './random.js';
import { closureFor, closureThickness } from './resample.js';
import { isFine, latticeCell, meanFibreLength, meanFibreRadius, type PaperRecipe } from './recipe.js';

export const FELT_STREAM = 0x5bd1e995;
export const RESIDUAL_STREAM = 0x27d4eb2f;

export function validateGrid(grid: Grid): void {
  const { width, height, cell } = grid;
  if (!Number.isInteger(width) || !Number.isInteger(height) || width < 8 || height < 8)
    throw new RangeError(`paper grids need integer sizes of at least 8 cells, got ${width}×${height}`);
  if (!(cell > 0) || !Number.isFinite(cell)) throw new RangeError(`paper cell size must be a positive number of metres, got ${cell}`);
}

export function generatePaper(recipe: PaperRecipe, grid: Grid, seed: number): PaperSheet {
  validateGrid(grid);
  const lattice = latticeCell(recipe), fine = isFine(recipe, grid.cell);
  if (grid.cell < lattice && !fine)
    throw new RangeError(`cells of ${grid.cell} m are finer than the deposit lattice (${lattice} m, half the narrowest fibre); flexibility is calibrated on the lattice, so generate at ${lattice} m`);
  const closure = fine ? null : closureFor(recipe, grid.cell);
  const dep = depositFibres({ furnish: recipe.furnish, grammage: recipe.grammage, flocculation: recipe.flocculation, flexibility: recipe.flexibility, machineBias: recipe.machineBias, drape: fine }, grid, seededRandom(seed));
  const initial = closure ? closureThickness(dep.mass, closure, grid.width, grid.height, seededRandom(seed ^ RESIDUAL_STREAM)) : dep.surface;
  const felt = recipe.felt ? feltSurface(recipe.felt, grid, seededRandom(seed ^ FELT_STREAM)) : null;
  const thickness = pressSheet(initial, felt, recipe.felt?.imprintDepth ?? 0, recipe.press, grid);
  return deriveFields({
    grid, mass: dep.mass, thickness, oxx: dep.oxx, oxy: dep.oxy, oyy: dep.oyy, radiusMass: dep.radiusMass,
    contactAngle: recipe.contactAngle, orientationRadius: meanFibreLength(recipe) / (2 * grid.cell), fallbackRadius: meanFibreRadius(recipe),
  });
}
