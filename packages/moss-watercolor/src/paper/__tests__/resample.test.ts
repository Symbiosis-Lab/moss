import { describe, expect, it } from 'vitest';
import { bulkPorosity } from '../fields.js';
import { generatePaper } from '../generate.js';
import { closureFor, closureThickness, fitDensityClosure, resamplePaper } from '../resample.js';
import { seededRandom } from '../random.js';
import { PRESETS } from '../recipe.js';

const stats = (a: Float32Array) => { const m = a.reduce((s, v) => s + v, 0) / a.length; return { mean: m, sd: Math.sqrt(a.reduce((s, v) => s + (v - m) ** 2, 0) / a.length) }; };

describe('resamplePaper', () => {
  const fine = generatePaper(PRESETS['cotton-cold-press'], { width: 256, height: 256, cell: 10e-6 }, 1);
  it('conserves mass and thickness on average', () => {
    const c = resamplePaper(fine, 80e-6);
    expect([c.width, c.height]).toEqual([32, 32]);
    expect(stats(c.grammage).mean / stats(fine.grammage).mean).toBeCloseTo(1, 5);
    expect(stats(c.thickness).mean / stats(fine.thickness).mean).toBeCloseTo(1, 5);
  });
  it('rejects factors that are not whole or do not divide the tile', () => {
    expect(() => resamplePaper(fine, 25e-6)).toThrow(RangeError);
    expect(() => resamplePaper(fine, 30e-6)).toThrow(RangeError);
  });
});

describe('coarse mode', () => {
  const recipe = PRESETS['cotton-cold-press'];
  const closure = fitDensityClosure(recipe, 80e-6, 11, 32);
  const withClosure = { ...recipe, densityClosures: [closure] };
  it('needs a closure fitted at its cell size, and says how to get one', () => {
    expect(() => closureFor(recipe, 80e-6)).toThrow(/fitDensityClosure/);
    expect(closureFor(withClosure, 81e-6).cell).toBe(80e-6);
  });
  it('gives the unexplained roughness its fitted spread and neighbour correlation', () => {
    const W = 128, mass = new Float32Array(W * W).fill(0.3);
    const t = closureThickness(mass, { cell: 80e-6, mean: 400, slope: 0, residual: 0.05, residualCorrelation: 0.4 }, W, W, seededRandom(1));
    const m = t.reduce((s, v) => s + v, 0) / t.length; let v = 0, l = 0;
    for (let y = 0; y < W; y++) for (let x = 0; x < W; x++) { const d = t[y * W + x] - m; v += d * d; l += d * (t[y * W + (x + 1) % W] - m) + d * (t[((y + 1) % W) * W + x] - m); }
    expect(Math.abs(Math.sqrt(v / t.length) / m / 0.05 - 1)).toBeLessThan(0.04);
    expect(l / (2 * v)).toBeCloseTo(0.4, 1);
  });
  it('refuses a coarse cell that is not a whole number of lattice cells', () => {
    expect(() => fitDensityClosure(recipe, 15e-6, 1)).toThrow(/lattice/);
  });
  it('agrees with a fine run averaged to the same cells', () => {
    const fine = resamplePaper(generatePaper(recipe, { width: 512, height: 512, cell: 10e-6 }, 2), 80e-6);
    const coarse = generatePaper(withClosure, { width: 64, height: 64, cell: 80e-6 }, 3);
    expect(Math.abs(bulkPorosity(coarse) - bulkPorosity(fine))).toBeLessThan(0.03);
    expect(stats(coarse.thickness).mean / stats(fine.thickness).mean).toBeCloseTo(1, 1);
    const cvF = stats(fine.grammage).sd / stats(fine.grammage).mean, cvC = stats(coarse.grammage).sd / stats(coarse.grammage).mean;
    expect(cvC / cvF).toBeGreaterThan(0.7); expect(cvC / cvF).toBeLessThan(1.43);
    // Thickness is what the engine reads as relief, and only partly follows grammage.
    const tF = stats(fine.thickness).sd / stats(fine.thickness).mean, tC = stats(coarse.thickness).sd / stats(coarse.thickness).mean;
    expect(tC / tF).toBeGreaterThan(0.7); expect(tC / tF).toBeLessThan(1.43);
  });
});
