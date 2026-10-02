// Sequential fibre deposition on a periodic tile (Niskanen & Alava 1994,
// as in the KCL-PAKKA simulator). Each straight fibre settles as low as it
// can without passing through the sheet, bending by at most `flexibility`
// fibre thicknesses per fibre width of its length, then adds its thickness
// and mass. Flocculation follows Provatas et al., generalised so it keeps
// acting after the first layer: a fibre over ground thinner than
// BARE_FRACTION of the running mean is accepted only with probability
// `flocculation`.
import { coarseness, fibreThickness, type FibreType } from './recipe.js';
import { lognormal, vonMisesAxial, type Rng } from './random.js';

export interface Grid { width: number; height: number; cell: number }
export interface Deposit {
  grid: Grid;
  mass: Float32Array;
  surface: Float32Array;
  oxx: Float32Array;
  oxy: Float32Array;
  oyy: Float32Array;
  radiusMass: Float32Array;
}
export interface DepositParams {
  furnish: FibreType[];
  grammage: number;
  flocculation: number;
  flexibility: number;
  machineBias: number;
  /** Fine mode drapes fibres over the surface; coarse mode deposits mass and orientation only. */
  drape: boolean;
}

export const BARE_FRACTION = 0.5;

const wrap = (i: number, n: number) => ((i % n) + n) % n;
const grow = (a: Int32Array) => { const b = new Int32Array(a.length * 2); b.set(a); return b; };

/** Visits each cell a segment crosses with the share of its length inside that cell (Amanatides & Woo 1987). */
export function crossCells(x0: number, y0: number, x1: number, y1: number, visit: (x: number, y: number, share: number) => void): void {
  const dx = x1 - x0, dy = y1 - y0, sx = Math.sign(dx), sy = Math.sign(dy);
  let x = Math.floor(x0), y = Math.floor(y0), t = 0;
  let tx = dx > 0 ? (x + 1 - x0) / dx : dx < 0 ? (x - x0) / dx : Infinity;
  let ty = dy > 0 ? (y + 1 - y0) / dy : dy < 0 ? (y - y0) / dy : Infinity;
  const stepX = dx !== 0 ? Math.abs(1 / dx) : Infinity, stepY = dy !== 0 ? Math.abs(1 / dy) : Infinity;
  for (;;) {
    const next = Math.min(tx, ty, 1);
    if (next > t) visit(x, y, next - t);
    if (next >= 1) return;
    t = next;
    if (tx <= ty) { x += sx; tx += stepX; } else { y += sy; ty += stepY; }
  }
}

/** Lowest profile at or above `floor` whose neighbouring samples differ by at most `drop`; in place. */
export function drapeProfile(floor: Float64Array, n: number, drop: number): void {
  for (let i = 1; i < n; i++) floor[i] = Math.max(floor[i], floor[i - 1] - drop);
  for (let i = n - 2; i >= 0; i--) floor[i] = Math.max(floor[i], floor[i + 1] - drop);
}

export function depositFibres(p: DepositParams, grid: Grid, rng: Rng): Deposit {
  const { width: W, height: H, cell: c } = grid;
  const n = W * H, cellArea = c * c, area = n * cellArea;
  const mass = new Float32Array(n), surface = new Float32Array(n);
  const oxx = new Float32Array(n), oxy = new Float32Array(n), oyy = new Float32Array(n), radiusMass = new Float32Array(n);
  const weights = p.furnish.map((f) => f.massFraction / (f.lengthMean * coarseness(f)));
  const total = weights.reduce((a, b) => a + b, 0);
  const cumulative: number[] = []; let acc = 0;
  for (const w of weights) { acc += w / total; cumulative.push(acc); }
  const target = p.grammage * area, maxLength = (Math.min(W, H) * c) / 2;
  let idx = new Int32Array(4096), bin = new Int32Array(4096), floor = new Float64Array(1024), deposited = 0;

  while (deposited < target) {
    const u = rng(); let k = 0;
    while (k < cumulative.length - 1 && u > cumulative[k]) k++;
    const f = p.furnish[k];
    const length = Math.max(c, Math.min(lognormal(rng, f.lengthMean, f.lengthLogSd), maxLength));
    const theta = vonMisesAxial(rng, p.machineBias);
    const cx = rng() * W, cy = rng() * H, dx = Math.cos(theta), dy = Math.sin(theta);
    const steps = Math.max(1, Math.ceil(length / c)), across = Math.max(1, Math.round(f.width / c));
    if (p.flocculation < 1) {
      // Does this fibre land where material already is? Asked of the whole
      // fibre the answer is always yes once the sheet is a few layers deep:
      // averaging the ground under a fibre hundreds of cells long returns the
      // sheet's own mean, so the test fired on 2% of fibres and left formation
      // flat. It is asked instead of a box one fibre wide at the fibre's
      // centre, the smallest scale at which "material is here" means anything.
      // On coarse cells the box cannot be smaller than one cell, so clumping is
      // weaker there (at the engine's texel, grammage spread about a tenth
      // below a fine sheet averaged to it). Measured on
      // a cotton sheet: at p = 0.05 it rejects 38% of fibres and raises the
      // grammage CV by a quarter.
      let local = 0;
      const back = Math.floor((across - 1) / 2), forward = across - 1 - back;
      for (let yy = -back; yy <= forward; yy++)
        for (let xx = -back; xx <= forward; xx++)
          local += mass[wrap(Math.floor(cy) + yy, H) * W + wrap(Math.floor(cx) + xx, W)];
      if (local / (across * across) < (BARE_FRACTION * deposited) / area && rng() >= p.flocculation) continue;
    }
    const fibreMass = coarseness(f) * length, r = f.width / 2;
    const add = (q: number, dm: number) => { mass[q] += dm; oxx[q] += dm * dx * dx; oxy[q] += dm * dx * dy; oyy[q] += dm * dy * dy; radiusMass[q] += dm * r; };
    if (p.drape) {
      // The fibre covers exactly the cells whose centres lie inside its
      // rectangle, so each covered cell gets one equal share of its mass and
      // one thickness of height. Point samples floored onto cells collide on
      // diagonal fibres, doubling a cell's mass while its height rises once,
      // which left one xuan cell in ten denser than solid cellulose.
      const hl = Math.max(length / c, 1) / 2, hw = Math.max(f.width / c, 1) / 2;
      const ey = Math.abs(dy) * hl + Math.abs(dx) * hw;
      let count = 0;
      for (let y = Math.ceil(cy - ey - 0.5); y <= Math.floor(cy + ey - 0.5); y++) {
        const py = y + 0.5 - cy;
        let lo = -Infinity, hi = Infinity;
        if (Math.abs(dx) > 1e-9) { const a = (-hl - py * dy) / dx, b = (hl - py * dy) / dx; lo = Math.max(lo, Math.min(a, b)); hi = Math.min(hi, Math.max(a, b)); }
        else if (Math.abs(py * dy) > hl) continue;
        if (Math.abs(dy) > 1e-9) { const a = (py * dx - hw) / dy, b = (py * dx + hw) / dy; lo = Math.max(lo, Math.min(a, b)); hi = Math.min(hi, Math.max(a, b)); }
        else if (Math.abs(py * dx) > hw) continue;
        for (let x = Math.ceil(cx + lo - 0.5); x <= Math.floor(cx + hi - 0.5); x++) {
          if (count === idx.length) { idx = grow(idx); bin = grow(bin); }
          const along = (x + 0.5 - cx) * dx + py * dy;
          idx[count] = wrap(y, H) * W + wrap(x, W);
          bin[count++] = Math.min(steps - 1, Math.max(0, Math.floor(((along + hl) / (2 * hl)) * steps)));
        }
      }
      if (count === 0) { idx[0] = wrap(Math.floor(cy), H) * W + wrap(Math.floor(cx), W); bin[0] = 0; count = 1; }
      const t = fibreThickness(f);
      if (floor.length < steps) floor = new Float64Array(steps * 2);
      floor.fill(0, 0, steps);
      for (let k = 0; k < count; k++) floor[bin[k]] = Math.max(floor[bin[k]], surface[idx[k]]);
      drapeProfile(floor, steps, (p.flexibility * t * (length / steps)) / f.width);
      for (let k = 0; k < count; k++) { const q = idx[k], top = floor[bin[k]] + t; if (top > surface[q]) surface[q] = top; }
      const dm = fibreMass / count / cellArea;
      for (let k = 0; k < count; k++) add(idx[k], dm);
    } else {
      // Coarse cells are wider than the fibre, so each cell the centreline
      // crosses gets the share of the fibre's length inside it: exactly what
      // averaging a fine sheet over the coarse cell gives. Covering only the
      // cells whose centres lie near the line put a cotton fibre in 2.8 cells
      // of the engine's texel instead of the 4.6 it crosses, which raised
      // grammage variance and halved its correlation between neighbours.
      const half = length / c / 2;
      crossCells(cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half, (x, y, share) => add(wrap(y, H) * W + wrap(x, W), (fibreMass * share) / cellArea));
    }
    deposited += fibreMass;
  }
  return { grid, mass, surface, oxx, oxy, oyy, radiusMass };
}
