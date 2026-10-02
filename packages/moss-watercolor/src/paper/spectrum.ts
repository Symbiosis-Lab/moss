// Radially averaged power spectra of periodic fields: how a paper's texture
// is compared with a measured sheet and how its felt scale is checked.
const isPow2 = (n: number) => n > 0 && (n & (n - 1)) === 0;

function fft(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len, wr = Math.cos(ang), wi = Math.sin(ang), half = len >> 1;
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let k = 0; k < half; k++) {
        const a = i + k, b = a + half;
        const xr = re[b] * cr - im[b] * ci, xi = re[b] * ci + im[b] * cr;
        re[b] = re[a] - xr; im[b] = im[a] - xi; re[a] += xr; im[a] += xi;
        const ncr = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = ncr;
      }
    }
  }
}

export function radialSpectrum(field: Float32Array, width: number, height: number): { frequency: Float64Array; power: Float64Array } {
  if (!isPow2(width) || !isPow2(height)) throw new RangeError(`spectra need power-of-two sizes, got ${width}×${height}`);
  let mean = 0; for (let i = 0; i < field.length; i++) mean += field[i]; mean /= field.length;
  const re = new Float64Array(width * height), im = new Float64Array(width * height);
  for (let i = 0; i < field.length; i++) re[i] = field[i] - mean;
  const rr = new Float64Array(width), ri = new Float64Array(width);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) { rr[x] = re[y * width + x]; ri[x] = im[y * width + x]; }
    fft(rr, ri);
    for (let x = 0; x < width; x++) { re[y * width + x] = rr[x]; im[y * width + x] = ri[x]; }
  }
  const cr = new Float64Array(height), ci = new Float64Array(height);
  for (let x = 0; x < width; x++) {
    for (let y = 0; y < height; y++) { cr[y] = re[y * width + x]; ci[y] = im[y * width + x]; }
    fft(cr, ci);
    for (let y = 0; y < height; y++) { re[y * width + x] = cr[y]; im[y * width + x] = ci[y]; }
  }
  const bins = Math.max(width, height) / 2, binWidth = 1 / Math.max(width, height);
  const power = new Float64Array(bins), counts = new Float64Array(bins), frequency = new Float64Array(bins);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const fx = (x <= width / 2 ? x : x - width) / width, fy = (y <= height / 2 ? y : y - height) / height;
    const f = Math.hypot(fx, fy), b = Math.round(f / binWidth);
    if (b < 1 || b > bins) continue;
    const k = y * width + x;
    power[b - 1] += re[k] * re[k] + im[k] * im[k]; counts[b - 1]++;
  }
  for (let b = 0; b < bins; b++) { frequency[b] = (b + 1) * binWidth; if (counts[b]) power[b] /= counts[b]; }
  return { frequency, power };
}

/** 1 / (power-weighted mean frequency), in metres. */
export function centroidWavelength(field: Float32Array, width: number, height: number, cell: number): number {
  const { frequency, power } = radialSpectrum(field, width, height);
  let fp = 0, p = 0;
  for (let i = 0; i < power.length; i++) { fp += frequency[i] * power[i]; p += power[i]; }
  return p > 0 ? cell / (fp / p) : Infinity;
}
