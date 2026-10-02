// Raking-light rendering of a sheet's surface, for inspecting it in the lab.
// Lambert shading normalised so a flat sheet shows the plain paper tint.
// Elevation 0 is not supported: the normalisation divides by its sine.
import type { PaperSheet } from './fields.js';

const TINT = [0xf4, 0xf1, 0xed] as const;

export function shadePaper(sheet: PaperSheet, light: { azimuth: number; elevation: number }, exaggeration = 1): Uint8ClampedArray {
  const { width: W, height: H, cell } = sheet, h = sheet.thickness, out = new Uint8ClampedArray(W * H * 4);
  const lx = Math.cos(light.elevation) * Math.cos(light.azimuth), ly = Math.cos(light.elevation) * Math.sin(light.azimuth), lz = Math.sin(light.elevation);
  const at = (x: number, y: number) => h[((y + H) % H) * W + ((x + W) % W)];
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
    const nx = (-(at(x + 1, y) - at(x - 1, y)) / (2 * cell)) * exaggeration, ny = (-(at(x, y + 1) - at(x, y - 1)) / (2 * cell)) * exaggeration;
    const len = Math.hypot(nx, ny, 1), shade = Math.max(0, (nx * lx + ny * ly + lz) / len) / lz;
    const o = (y * W + x) * 4;
    out[o] = TINT[0] * shade; out[o + 1] = TINT[1] * shade; out[o + 2] = TINT[2] * shade; out[o + 3] = 255;
  }
  return out;
}
