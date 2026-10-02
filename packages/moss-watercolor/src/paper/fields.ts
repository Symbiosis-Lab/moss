// Every field the solver can read, derived per cell in SI units from the
// deposited mass, the pressed thickness and the fibre orientation.
// Permeability uses Gebart (1992) for hexagonally packed fibres, mixed by the
// orientation tensor; capillary radius is twice the hydraulic radius of a
// bed of cylinders; entry pressure is Young–Laplace; wicking is Washburn.
import { CELLULOSE_DENSITY } from './recipe.js';
import { gaussianBlurWrap } from './filter.js';
import type { Grid } from './deposit.js';

export const WATER_SURFACE_TENSION = 0.0726; // N/m at 20 °C
export const WATER_VISCOSITY = 1.002e-3;     // Pa·s at 20 °C

export type Field = Float32Array;
export interface Tensor { xx: Field; xy: Field; yy: Field }
export interface PaperSheet {
  width: number; height: number; cell: number;
  grammage: Field; thickness: Field; porosity: Field; fibreRadius: Field;
  orientation: Tensor; permeability: Tensor;
  capillaryRadius: Field; entryPressure: Field; washburn: Field; contactAngle: Field;
}
export interface FieldInputs {
  grid: Grid;
  mass: Field; thickness: Field; oxx: Field; oxy: Field; oyy: Field; radiusMass: Field;
  contactAngle: number;
  /** Radius, in cells, of a box of equal variance over which orientation is averaged (half a mean fibre length); smoothing is Gaussian. */
  orientationRadius: number;
  /** Fibre radius used where a cell holds no fibre, m. */
  fallbackRadius: number;
}

const HEX = { c: 57, C: 16 / (9 * Math.PI * Math.sqrt(6)), vMax: Math.PI / (2 * Math.sqrt(3)) };
// A cell covered by one collapsed fibre lying on the base is solid
// cellulose, and a bare cell is a hole; the clamp keeps permeability and
// entry pressure finite for both. In thin xuan these are a few percent of
// cells, which is what the sheet is.
const POROSITY_RANGE = [0.02, 0.98] as const;

/** Sheet porosity: 1 − total mass / (cellulose density · total thickness). */
export function bulkPorosity(s: PaperSheet): number {
  let m = 0, t = 0;
  for (let i = 0; i < s.grammage.length; i++) { m += s.grammage[i]; t += s.thickness[i]; }
  return 1 - m / (CELLULOSE_DENSITY * t);
}

/** Gebart (1992), hexagonal packing; `solid` is the solid fraction, in (0, 1). */
export function gebart(radius: number, solid: number): { along: number; across: number } {
  const v = solid, a2 = radius * radius;
  const along = ((8 * a2) / HEX.c) * (1 - v) ** 3 / (v * v);
  const across = v < HEX.vMax ? HEX.C * a2 * (Math.sqrt(HEX.vMax / v) - 1) ** 2.5 : 0;
  return { along, across };
}

export function deriveFields(inp: FieldInputs): PaperSheet {
  const { width: W, height: H, cell } = inp.grid, n = W * H;
  const r = Math.max(0, Math.round(inp.orientationRadius));
  // A Gaussian of the box's variance (r²/3): a separable box leaves square blocks in the tensor.
  const smooth = (f: Field) => (r > 0 ? gaussianBlurWrap(f, W, H, r / Math.sqrt(3)) : f);
  const sm = smooth(inp.mass), sxx = smooth(inp.oxx), sxy = smooth(inp.oxy), syy = smooth(inp.oyy);
  const make = () => new Float32Array(n);
  const porosity = make(), fibreRadius = make(), axx = make(), axy = make(), ayy = make();
  const kxx = make(), kxy = make(), kyy = make(), capillaryRadius = make(), entryPressure = make(), washburn = make();
  // cos(π/2) is 6e-17, not 0: treat a wetting cosine that small as exactly
  // neutral, so a 90° sheet neither wicks nor pulls water in.
  const raw = Math.cos(inp.contactAngle), cos = raw > 1e-12 ? raw : Math.min(0, raw), gamma = WATER_SURFACE_TENSION;
  for (let i = 0; i < n; i++) {
    const m = inp.mass[i], T = Math.max(inp.thickness[i], 1e-12);
    const eps = Math.min(POROSITY_RANGE[1], Math.max(POROSITY_RANGE[0], 1 - m / (CELLULOSE_DENSITY * T)));
    const a = m > 0 ? inp.radiusMass[i] / m : inp.fallbackRadius;
    const ox = sm[i] > 0 ? sxx[i] / sm[i] : 0.5, oxy = sm[i] > 0 ? sxy[i] / sm[i] : 0, oy = sm[i] > 0 ? syy[i] / sm[i] : 0.5;
    const { along, across } = gebart(a, 1 - eps);
    const rc = (eps * a) / (1 - eps);
    porosity[i] = eps; fibreRadius[i] = a; axx[i] = ox; axy[i] = oxy; ayy[i] = oy;
    kxx[i] = along * ox + across * (1 - ox); kxy[i] = (along - across) * oxy; kyy[i] = along * oy + across * (1 - oy);
    capillaryRadius[i] = rc;
    entryPressure[i] = (2 * gamma * cos) / rc;
    washburn[i] = Math.sqrt(Math.max(0, (rc * gamma * cos) / (2 * WATER_VISCOSITY)));
  }
  return {
    width: W, height: H, cell,
    grammage: inp.mass.slice(), thickness: inp.thickness.slice(),
    porosity, fibreRadius, orientation: { xx: axx, xy: axy, yy: ayy }, permeability: { xx: kxx, xy: kxy, yy: kyy },
    capillaryRadius, entryPressure, washburn, contactAngle: new Float32Array(n).fill(inp.contactAngle),
  };
}
