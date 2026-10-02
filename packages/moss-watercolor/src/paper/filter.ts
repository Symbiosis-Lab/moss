// Blurs on a periodic tile, so a blurred paper still tiles without a seam.
function boxPass(src: Float32Array, dst: Float32Array, W: number, H: number, r: number, alongX: boolean): void {
  const n = alongX ? W : H, lines = alongX ? H : W;
  const rr = Math.min(r, Math.floor((n - 1) / 2));
  const span = 2 * rr + 1;
  for (let l = 0; l < lines; l++) {
    const at = (i: number) => { const k = ((i % n) + n) % n; return alongX ? l * W + k : k * W + l; };
    let acc = 0;
    for (let i = -rr; i <= rr; i++) acc += src[at(i)];
    for (let i = 0; i < n; i++) {
      dst[at(i)] = acc / span;
      acc += src[at(i + rr + 1)] - src[at(i - rr)];
    }
  }
}

export function boxBlurWrap(src: Float32Array, width: number, height: number, radius: number): Float32Array {
  const r = Math.max(0, Math.round(radius));
  if (r === 0) return src.slice();
  const tmp = new Float32Array(src.length), out = new Float32Array(src.length);
  boxPass(src, tmp, width, height, r, true);
  boxPass(tmp, out, width, height, r, false);
  return out;
}

/** Three box passes approximate a Gaussian of standard deviation sigma cells (Wells 1986). */
export function gaussianBlurWrap(src: Float32Array, width: number, height: number, sigma: number): Float32Array {
  // Below about 0.7 cells the nearest three-box blur is no blur at all: a
  // length shorter than a cell is not resolved, so the field passes through.
  const r = sigma > 0 ? Math.round((Math.sqrt(4 * sigma * sigma + 1) - 1) / 2) : 0;
  if (r === 0) return src.slice();
  return boxBlurWrap(boxBlurWrap(boxBlurWrap(src, width, height, r), width, height, r), width, height, r);
}
