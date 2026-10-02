import { describe, expect, it } from 'vitest';
import { feltSurface, pressSheet } from '../press.js';
import { FELT_FIBRE, PRESETS, type FeltSpec } from '../recipe.js';
import { seededRandom } from '../random.js';
import { centroidWavelength } from '../spectrum.js';
import type { Grid } from '../deposit.js';

const grid: Grid = { width: 256, height: 256, cell: 20e-6 };
const felt = (conformity: number): FeltSpec => ({ fibre: FELT_FIBRE, grammage: 0.2, conformity, imprintDepth: 30e-6 });
const rms = (a: Float32Array) => { const m = a.reduce((s, v) => s + v, 0) / a.length; return Math.sqrt(a.reduce((s, v) => s + (v - m) ** 2, 0) / a.length); };
const mean = (a: Float32Array) => a.reduce((s, v) => s + v, 0) / a.length;

describe('feltSurface', () => {
  it('is zero-mean with unit RMS at the conformity scale', () => {
    const f = feltSurface(felt(0.15e-3), grid, seededRandom(1));
    expect(Math.abs(mean(f))).toBeLessThan(1e-4); expect(rms(f)).toBeGreaterThan(0.85); expect(rms(f)).toBeLessThan(1.15);
  });
  it('keeps its depth when generated coarse: a coarse felt matches a fine one averaged to its cells', () => {
    const spec = PRESETS['cotton-cold-press'].felt!, k = 16, M = 512 / k, avg = new Float32Array(M * M);
    const fine = feltSurface(spec, { width: 512, height: 512, cell: 20e-6 }, seededRandom(3));
    for (let y = 0; y < 512; y++) for (let x = 0; x < 512; x++) avg[Math.floor(y / k) * M + Math.floor(x / k)] += fine[y * 512 + x] / (k * k);
    const coarse = feltSurface(spec, { width: M, height: M, cell: 20e-6 * k }, seededRandom(4));
    expect(rms(coarse) / rms(avg)).toBeGreaterThan(0.8); expect(rms(coarse) / rms(avg)).toBeLessThan(1.25);
  });
  it('coarsens as the conformity length grows', () => {
    const wl = (c: number) => centroidWavelength(feltSurface(felt(c), grid, seededRandom(1)), 256, 256, grid.cell);
    expect(wl(0.4e-3)).toBeGreaterThan(wl(0.1e-3) * 1.3);
  });
});

describe('pressSheet', () => {
  const flat = new Float32Array(256 * 256).fill(5e-4);
  const f = feltSurface(felt(0.15e-3), grid, seededRandom(2));
  const finish = (name: 'cotton-rough' | 'cotton-cold-press' | 'cotton-hot-press') => pressSheet(flat, f, PRESETS[name].felt!.imprintDepth, PRESETS[name].press, grid);
  it('orders roughness rough > cold-press > hot-press', () => {
    const r = rms(finish('cotton-rough')), c = rms(finish('cotton-cold-press')), h = rms(finish('cotton-hot-press'));
    expect(r).toBeGreaterThan(c); expect(c).toBeGreaterThan(h);
  });
  it('polishing smooths the top surface at fixed imprint', () => {
    const at = (polishStrength: number) => rms(pressSheet(flat, f, 30e-6, { compaction: 1, polishLength: 1e-3, polishStrength }, grid));
    expect(at(0.9)).toBeLessThan(at(0) * 0.5);
  });
  it('scales mean thickness by compaction, imprint leaves the mean alone', () => {
    const out = pressSheet(flat, f, 30e-6, { compaction: 0.8, polishLength: 0, polishStrength: 0 }, grid);
    expect(mean(out) / (5e-4 * 0.8)).toBeCloseTo(1, 2);
  });
  it('is the identity with no felt and no press', () => {
    const out = pressSheet(flat, null, 0, { compaction: 1, polishLength: 0, polishStrength: 0 }, grid);
    expect(out[0]).toBeCloseTo(5e-4, 9);
  });
});
