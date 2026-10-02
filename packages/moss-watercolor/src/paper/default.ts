// The paper's own generated texture: tileable value noise, four fbm octave
// sets packed one per channel (r relief, g absorbency, b fibre, a pore),
// each independently contrast-stretched to the full 0-255 range. The same
// seed always produces a bit-identical sheet, and `createPaper()` with no
// arguments is the default 256x256 sheet (pinned by a hash in the tests).
// The result carries its own width/height rather than a caller assuming 256,
// but `createSim` still accepts only 256x256: the shaders' paper period is a
// constant, not yet a uniform.
import { seededRandom } from './random.js';

export interface PaperOptions {
  width?: number;
  height?: number;
  seed?: number;
}

export interface Paper {
  width: number;
  height: number;
  /** RGBA, one byte per channel; declared as a union so a future
   *  higher-precision generator can return Float32Array without moving
   *  this contract again. */
  data: Uint8Array | Float32Array;
}

export function createPaper({ width = 256, height = 256, seed = 90210 }: PaperOptions = {}): Paper {
  const rnd = seededRandom(seed);
  const lattice = (n: number) => { const g = new Float32Array(n * n); for (let i = 0; i < g.length; i++) g[i] = rnd(); return g; };
  const sm = (t: number) => t * t * (3 - 2 * t);
  const value = (g: Float32Array, n: number, x: number, y: number) => { const gx = x * n, gy = y * n, x0 = Math.floor(gx) % n, y0 = Math.floor(gy) % n, x1 = (x0 + 1) % n, y1 = (y0 + 1) % n, fx = sm(gx - Math.floor(gx)), fy = sm(gy - Math.floor(gy));
    const a = g[y0 * n + x0], b = g[y0 * n + x1], c = g[y1 * n + x0], d = g[y1 * n + x1]; return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy; };
  const octs = [4, 8, 16, 32, 64, 128];
  const fbm = (lats: Float32Array[], x: number, y: number, o0: number) => { let s = 0, amp = 1, tot = 0; for (let o = o0; o < octs.length; o++) { s += amp * value(lats[o], octs[o], x, y); tot += amp; amp *= 0.55; } return s / tot; };
  const L = [0, 1, 2, 3].map(() => octs.map(lattice));
  const ch = [0, 1, 2, 3].map(() => new Float32Array(width * height));
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) { const i = y * width + x, u = x / width, v = y / height;
    ch[0][i] = fbm(L[0], u, v, 2) * 0.7 + 0.3 * rnd(); ch[1][i] = fbm(L[1], u, v, 1); ch[2][i] = fbm(L[2], u, v, 0); ch[3][i] = fbm(L[3], u, v, 3); }
  const px = new Uint8Array(width * height * 4);
  for (let k = 0; k < 4; k++) { let lo = 1, hi = 0; for (const t of ch[k]) { lo = Math.min(lo, t); hi = Math.max(hi, t); } for (let i = 0; i < width * height; i++) px[i * 4 + k] = ((ch[k][i] - lo) / (hi - lo)) * 255; }
  return { width, height, data: px };
}
