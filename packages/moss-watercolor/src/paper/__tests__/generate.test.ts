import { describe, expect, it } from 'vitest';
import { bulkPorosity } from '../fields.js';
import { generatePaper, validateGrid } from '../generate.js';
import { PRESETS, type PresetName } from '../recipe.js';


describe('generatePaper (fine)', () => {
  it('produces finite fields for every preset', () => {
    for (const name of Object.keys(PRESETS) as PresetName[]) {
      const cell = name.startsWith('xuan') ? 5e-6 : 10e-6;
      const s = generatePaper(PRESETS[name], { width: 128, height: 128, cell }, 1);
      for (const f of [s.porosity, s.thickness, s.permeability.xx, s.permeability.yy, s.entryPressure, s.washburn]) expect([...f].every(Number.isFinite)).toBe(true);
    }
  });
  it('gets denser as fibres get more flexible', () => {
    const at = (flexibility: number) => bulkPorosity(generatePaper({ ...PRESETS['cotton-cold-press'], flexibility }, { width: 128, height: 128, cell: 10e-6 }, 3));
    expect(at(8)).toBeLessThan(at(0.5) - 0.05);
  });
  it('refuses a cell finer than the deposit lattice', () => {
    expect(() => generatePaper(PRESETS['cotton-cold-press'], { width: 64, height: 64, cell: 5e-6 }, 4)).toThrow(/finer than the deposit lattice/);
  });
  it('is byte-deterministic for a seed', () => {
    const g = { width: 64, height: 64, cell: 10e-6 };
    const a = generatePaper(PRESETS['cotton-rough'], g, 7), b = generatePaper(PRESETS['cotton-rough'], g, 7);
    expect(Buffer.from(a.washburn.buffer).equals(Buffer.from(b.washburn.buffer))).toBe(true);
  });
  it('keeps a near-empty sheet finite', () => {
    const s = generatePaper({ ...PRESETS['xuan-unsized'], grammage: 0.0005 }, { width: 64, height: 64, cell: 5e-6 }, 2);
    expect([...s.washburn, ...s.porosity, ...s.permeability.xx].every(Number.isFinite)).toBe(true);
  });
});

describe('validateGrid', () => {
  it('rejects grids it cannot tile, with a message saying why', () => {
    for (const g of [{ width: 7, height: 64, cell: 1e-5 }, { width: 64.5, height: 64, cell: 1e-5 }, { width: 64, height: 64, cell: 0 }, { width: 64, height: 64, cell: Number.NaN }])
      expect(() => validateGrid(g)).toThrow(RangeError);
  });
});
