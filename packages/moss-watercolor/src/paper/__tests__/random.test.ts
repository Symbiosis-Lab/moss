import { describe, expect, it } from 'vitest';
import { lognormal, seededRandom, vonMisesAxial } from '../random.js';

describe('seededRandom', () => {
  it('is deterministic and lies in [0, 1)', () => {
    const a = seededRandom(7), b = seededRandom(7);
    for (let i = 0; i < 1000; i++) { const x = a(); expect(x).toBe(b()); expect(x).toBeGreaterThanOrEqual(0); expect(x).toBeLessThan(1); }
  });
});

describe('lognormal', () => {
  it('has the requested arithmetic mean', () => {
    const rng = seededRandom(1); let sum = 0; const n = 40000;
    for (let i = 0; i < n; i++) sum += lognormal(rng, 1.5e-3, 0.4);
    expect(sum / n / 1.5e-3).toBeCloseTo(1, 1);
  });
});

describe('vonMisesAxial', () => {
  const meanCos2 = (kappa: number) => { const rng = seededRandom(3); let s = 0; const n = 40000; for (let i = 0; i < n; i++) s += Math.cos(2 * vonMisesAxial(rng, kappa)); return s / n; };
  it('is isotropic at kappa 0', () => { expect(Math.abs(meanCos2(0))).toBeLessThan(0.02); });
  it('concentrates around 0 as I1(k)/I0(k) predicts', () => { expect(meanCos2(4)).toBeCloseTo(0.8635, 1); });
  it('returns angles in [0, pi)', () => { const rng = seededRandom(5); for (let i = 0; i < 2000; i++) { const t = vonMisesAxial(rng, 2); expect(t).toBeGreaterThanOrEqual(0); expect(t).toBeLessThan(Math.PI); } });
});
