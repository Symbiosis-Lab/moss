import { describe, expect, it } from 'vitest';
import { crossCells, depositFibres, drapeProfile, type DepositParams, type Grid } from '../deposit.js';
import { COTTON, coarseness } from '../recipe.js';
import { seededRandom } from '../random.js';

const params = (o: Partial<DepositParams> = {}): DepositParams => ({ furnish: [COTTON], grammage: 0.05, flocculation: 1, flexibility: 2, machineBias: 0, drape: true, ...o });
const totalMass = (m: Float32Array, g: Grid) => m.reduce((s, v) => s + v, 0) * g.cell * g.cell;
const maxFibreMass = (len: number) => coarseness(COTTON) * len;

describe('drapeProfile', () => {
  it('bridges a gap when stiff and follows it when flexible', () => {
    const p = (drop: number) => { const f = Float64Array.from([5, 0, 0, 0, 5]); drapeProfile(f, 5, drop); return [...f]; };
    expect(p(0)).toEqual([5, 5, 5, 5, 5]);
    expect(p(1)).toEqual([5, 4, 3, 4, 5]);
    expect(p(10)).toEqual([5, 0, 0, 0, 5]);
  });
});

describe('crossCells', () => {
  it('gives each crossed cell its share of the segment', () => {
    const got: string[] = [];
    crossCells(0.5, 0.5, 2.5, 0.5, (x, y, share) => got.push(`${x},${y}:${share}`));
    expect(got).toEqual(['0,0:0.25', '1,0:0.5', '2,0:0.25']);
    let sum = 0, cells = 0;
    crossCells(3.7, 1.2, -2.1, 5.9, (_x, _y, share) => { sum += share; cells++; });
    expect(sum).toBeCloseTo(1, 12); expect(cells).toBe(11);
  });
});

describe('depositFibres', () => {
  const grid: Grid = { width: 128, height: 128, cell: 10e-6 };
  it('deposits the requested grammage, overshooting by at most one fibre', () => {
    const d = depositFibres(params(), grid, seededRandom(1));
    const target = 0.05 * 128 * 128 * 1e-10;
    const got = totalMass(d.mass, grid);
    expect(got).toBeGreaterThanOrEqual(target * (1 - 1e-6));
    expect(got - target).toBeLessThan(maxFibreMass(128 * 10e-6 / 2) + target * 1e-6);
  });
  it('is deterministic for a seed', () => {
    const a = depositFibres(params(), grid, seededRandom(9)), b = depositFibres(params(), grid, seededRandom(9));
    expect(Buffer.from(a.surface.buffer).equals(Buffer.from(b.surface.buffer))).toBe(true);
  });
  it('is isotropic without machine bias and recovers a set bias', () => {
    const big: Grid = { width: 256, height: 256, cell: 10e-6 };
    const aniso = (k: number) => { const d = depositFibres(params({ machineBias: k, grammage: 0.1 }), big, seededRandom(2)); let xx = 0, yy = 0, xy = 0, m = 0; for (let i = 0; i < d.mass.length; i++) { xx += d.oxx[i]; yy += d.oyy[i]; xy += d.oxy[i]; m += d.mass[i]; } return { diff: (xx - yy) / m, shear: xy / m }; };
    const iso = aniso(0); expect(Math.abs(iso.diff)).toBeLessThan(0.05); expect(Math.abs(iso.shear)).toBeLessThan(0.05);
    expect(aniso(4).diff).toBeGreaterThan(0.75);
  });
  it('makes a flexible sheet thinner than a stiff one of the same mass', () => {
    const mean = (f: number) => { const d = depositFibres(params({ flexibility: f, grammage: 0.1 }), grid, seededRandom(4)); return d.surface.reduce((s, v) => s + v, 0) / d.surface.length; };
    expect(mean(8)).toBeLessThan(mean(0.25) * 0.9);
  });
  it('clumps more as the acceptance probability falls', () => {
    // Formation: the spread of grammage over 160 µm blocks. One sheet is too
    // noisy to read a trend from (single-seed ratios range 0.95 to 1.27), so
    // this averages three.
    const g: Grid = { width: 256, height: 256, cell: 10e-6 };
    const cv = (p: number, seed: number) => {
      const d = depositFibres(params({ grammage: 0.05, flocculation: p }), g, seededRandom(seed));
      const B = 16, blocks: number[] = [];
      for (let by = 0; by < 256; by += B) for (let bx = 0; bx < 256; bx += B) {
        let s = 0; for (let y = by; y < by + B; y++) for (let x = bx; x < bx + B; x++) s += d.mass[y * 256 + x];
        blocks.push(s);
      }
      const m = blocks.reduce((a, b) => a + b, 0) / blocks.length;
      return Math.sqrt(blocks.reduce((a, b) => a + (b - m) ** 2, 0) / blocks.length) / m;
    };
    const seeds = [5, 1, 2];
    const mean = (p: number) => seeds.reduce((s, seed) => s + cv(p, seed), 0) / seeds.length;
    expect(mean(0.05)).toBeGreaterThan(mean(1) * 1.15);
  });
  it('never puts more fibre in a cell than fits under its surface', () => {
    // Each covered cell gains one share of mass and one thickness of height,
    // so solid volume never exceeds the column. Point samples floored onto
    // cells broke this on diagonal fibres, doubling a cell's mass.
    const d = depositFibres(params({ grammage: 0.1 }), grid, seededRandom(3));
    let worst = 0;
    for (let i = 0; i < d.mass.length; i++) if (d.surface[i] > 0) worst = Math.max(worst, d.mass[i] / 1500 / d.surface[i]);
    expect(worst).toBeLessThan(1.05);
  });
  it('deposits a coarse sheet with the grammage texture of a fine one averaged to its cells', () => {
    // Fibres three cells long, as cotton is at the engine's texel, where
    // covering only the cells near the line concentrates each fibre's mass.
    const k = 8, N = 512, M = N / k, short = { furnish: [{ ...COTTON, lengthMean: 0.25e-3, lengthLogSd: 0 }], grammage: 0.1 };
    const stat = (a: Float32Array, W: number) => {
      const m = a.reduce((s, v) => s + v, 0) / a.length; let v = 0, l = 0;
      for (let y = 0; y < W; y++) for (let x = 0; x < W; x++) { const d = a[y * W + x] - m; v += d * d; l += d * (a[y * W + (x + 1) % W] - m); }
      return { cv: Math.sqrt(v / a.length) / m, lag: l / v };
    };
    const fine = depositFibres(params(short), { width: N, height: N, cell: 10e-6 }, seededRandom(5)), avg = new Float32Array(M * M);
    for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) avg[Math.floor(y / k) * M + Math.floor(x / k)] += fine.mass[y * N + x] / (k * k);
    const f = stat(avg, M), c = stat(depositFibres(params({ ...short, drape: false }), { width: M, height: M, cell: k * 10e-6 }, seededRandom(15)).mass, M);
    expect(Math.abs(c.cv / f.cv - 1)).toBeLessThan(0.06);
    expect(Math.abs(c.lag - f.lag)).toBeLessThan(0.06);
  });
  it('wraps fibres longer than the tile and conserves mass on an odd non-square grid', () => {
    for (const g of [{ width: 37, height: 53, cell: 10e-6 }, { width: 8, height: 8, cell: 10e-6 }] as Grid[]) {
      const d = depositFibres(params({ grammage: 0.2 }), g, seededRandom(6));
      const target = 0.2 * g.width * g.height * g.cell * g.cell;
      expect(totalMass(d.mass, g)).toBeGreaterThanOrEqual(target * (1 - 1e-6));
      expect([...d.surface].every(Number.isFinite)).toBe(true);
    }
  });
  it('leaves no seam: the wrap step is unremarkable among interior steps', () => {
    // Interior column steps vary four-fold between neighbouring pairs, so one
    // arbitrary pair says nothing. The seam is scored against the spread of
    // every interior pair, averaged over three sheets.
    const W = 128;
    const z = (seed: number) => {
      const d = depositFibres(params({ grammage: 0.15 }), grid, seededRandom(seed));
      const step = (x: number) => {
        let s = 0;
        for (let y = 0; y < W; y++) s += Math.abs(d.surface[y * W + ((x + 1) % W)] - d.surface[y * W + x]);
        return s / W;
      };
      const interior = Array.from({ length: W - 1 }, (_, x) => step(x));
      const m = interior.reduce((a, b) => a + b, 0) / interior.length;
      const sd = Math.sqrt(interior.reduce((a, b) => a + (b - m) ** 2, 0) / interior.length);
      return (step(W - 1) - m) / sd;
    };
    const seeds = [8, 1, 2];
    expect(seeds.reduce((s, seed) => s + z(seed), 0) / seeds.length).toBeLessThan(2);
  });
});
