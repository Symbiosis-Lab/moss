// The engine's paper texture from a physical sheet. Each channel carries the
// field whose role the shaders already give it: r relief = sheet thickness,
// g absorbency = Washburn coefficient, b fibre = permeability. Channel a,
// pore, is 128 everywhere: entry pressure comes from the same capillary
// radius as wicking, so within a sheet it was the absorbency channel
// inverted. It returns when pore throats are modelled apart from the
// hydraulic radius. A CSS pixel is 1/96 inch, so by default the paper is
// shown at its true size.
import type { Paper } from './default.js';
import type { PaperSheet } from './fields.js';
import { generatePaper } from './generate.js';
import type { PaperRecipe } from './recipe.js';

export const CSS_PX = 0.0254 / 96;
export const ENGINE_TEXEL = 2 * CSS_PX;
/**
 * Each channel's physical span: byte 0 is `lo` and byte 255 is `hi`, so
 * whatever reads the texture can decode it, and two papers keep their
 * physical difference. Stretching each sheet to its own percentiles gave
 * rough and hot-press cotton the same relief.
 */
export const ENGINE_SPANS = {
  /** Thickness about the sheet's mean, m. */
  relief: { lo: -100e-6, hi: 100e-6 },
  /** Washburn coefficient, m/√s. */
  absorbency: { lo: 0, hi: 0.032 },
  /** log10 of the mean in-plane permeability in m². */
  fibre: { lo: -12, hi: -9 },
} as const;

const encode = (v: number, { lo, hi }: { lo: number; hi: number }) => Math.round(255 * Math.min(1, Math.max(0, (v - lo) / (hi - lo))));

export function toEngineChannels(sheet: PaperSheet): Paper {
  const n = sheet.width * sheet.height, data = new Uint8Array(n * 4);
  let mean = 0; for (let i = 0; i < n; i++) mean += sheet.thickness[i]; mean /= n;
  for (let i = 0; i < n; i++) {
    data[i * 4] = encode(sheet.thickness[i] - mean, ENGINE_SPANS.relief);
    data[i * 4 + 1] = encode(sheet.washburn[i], ENGINE_SPANS.absorbency);
    data[i * 4 + 2] = encode(Math.log10((sheet.permeability.xx[i] + sheet.permeability.yy[i]) / 2), ENGINE_SPANS.fibre);
    data[i * 4 + 3] = 128;
  }
  return { width: sheet.width, height: sheet.height, data };
}

export function enginePaper(recipe: PaperRecipe, { texels = 256, cssPxPerTexel = 2, metresPerCssPx = CSS_PX, seed = 1 } = {}): Paper {
  return toEngineChannels(generatePaper(recipe, { width: texels, height: texels, cell: cssPxPerTexel * metresPerCssPx }, seed));
}
