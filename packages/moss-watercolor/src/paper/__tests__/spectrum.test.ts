import { describe, expect, it } from 'vitest';
import { centroidWavelength, radialSpectrum } from '../spectrum.js';

describe('spectrum', () => {
  it('finds the wavelength of a pure sinusoid', () => {
    const W = 128, H = 64, f = new Float32Array(W * H);
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) f[y * W + x] = Math.sin((2 * Math.PI * x) / 16);
    expect(centroidWavelength(f, W, H, 1e-5) / 16e-5).toBeCloseTo(1, 2);
  });
  it('rejects sizes that are not powers of two', () => {
    expect(() => radialSpectrum(new Float32Array(12 * 8), 12, 8)).toThrow(RangeError);
  });
});
