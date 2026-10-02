import { describe, expect, it } from 'vitest';
import { bulkPorosity, deriveFields, gebart, type FieldInputs } from '../fields.js';

const uniform = (o: Partial<FieldInputs> & { m?: number; T?: number; a?: number; axx?: number } = {}): FieldInputs => {
  const W = 4, H = 4, n = W * H, m = o.m ?? 0.3, T = o.T ?? 5e-4, a = o.a ?? 10e-6, axx = o.axx ?? 0.5;
  const fill = (v: number) => new Float32Array(n).fill(v);
  return { grid: { width: W, height: H, cell: 1e-4 }, mass: fill(m), thickness: fill(T), oxx: fill(m * axx), oxy: fill(0), oyy: fill(m * (1 - axx)), radiusMass: fill(m * a), contactAngle: 0, orientationRadius: 0, fallbackRadius: 7e-6, ...o };
};

describe('gebart', () => {
  it('flows more easily along fibres than across, and less as the sheet packs', () => {
    const g = gebart(10e-6, 0.35); expect(g.along).toBeGreaterThan(g.across);
    expect(gebart(10e-6, 0.5).along).toBeLessThan(g.along); expect(gebart(10e-6, 0.5).across).toBeLessThan(g.across);
    expect(gebart(10e-6, 0.95).across).toBe(0);
  });
});

describe('deriveFields', () => {
  it('matches the literature forms on a hand-computed cell', () => {
    // a = 10 µm, porosity 0.6, isotropic, fully wetting. Gebart (1992),
    // hexagonal: K∥ = 8a²(1−V)³/(57V²), K⊥ = 16/(9π√6)·a²(√(Vmax/V) − 1)^2.5.
    const s = deriveFields(uniform());
    const rel = (got: number, want: number) => expect(got / want).toBeCloseTo(1, 4);
    rel(s.permeability.xx[0], (1.8947368e-11 + 4.2021115e-12) / 2);
    rel(s.capillaryRadius[0], 15e-6);
    rel(s.entryPressure[0], 9680);
    rel(s.washburn[0], 0.023311224);
  });
  it('computes porosity from mass and thickness, per cell and for the sheet', () => {
    const s = deriveFields(uniform());
    expect(s.porosity[0]).toBeCloseTo(1 - 0.3 / (1500 * 5e-4), 5); // 0.6
    expect(bulkPorosity(s)).toBeCloseTo(0.6, 5);
  });
  it('makes aligned fibres anisotropic and isotropic ones not', () => {
    const al = deriveFields(uniform({ axx: 1 })), iso = deriveFields(uniform({ axx: 0.5 }));
    expect(al.permeability.xx[0]).toBeGreaterThan(al.permeability.yy[0] * 1.2);
    expect(iso.permeability.xx[0]).toBeCloseTo(iso.permeability.yy[0], 20);
  });
  it('stops wicking at 90 degrees and above without NaN', () => {
    for (const ang of [Math.PI / 2, (100 * Math.PI) / 180]) {
      const s = deriveFields(uniform({ contactAngle: ang }));
      expect(s.washburn[0]).toBe(0); expect(s.entryPressure[0]).toBeLessThanOrEqual(1e-9);
      expect(Number.isFinite(s.entryPressure[0])).toBe(true);
    }
  });
  it('keeps every field finite over bare cells', () => {
    const s = deriveFields(uniform({ m: 0, T: 0 }));
    for (const f of [s.porosity, s.fibreRadius, s.permeability.xx, s.permeability.yy, s.capillaryRadius, s.entryPressure, s.washburn, s.orientation.xx])
      expect([...f].every(Number.isFinite)).toBe(true);
    expect(s.fibreRadius[0]).toBeCloseTo(7e-6, 12); expect(s.orientation.xx[0]).toBeCloseTo(0.5, 6);
  });
});
