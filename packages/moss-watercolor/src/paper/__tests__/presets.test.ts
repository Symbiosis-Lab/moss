import { describe, expect, it } from 'vitest';
import { ENGINE_TEXEL } from '../engine.js';
import { bulkPorosity } from '../fields.js';
import { FIT_GRID } from './fit.js';
import { generatePaper } from '../generate.js';
import { feltSurface } from '../press.js';
import { fitDensityClosure } from '../resample.js';
import { PRESETS, PRESET_TARGETS, type PresetName } from '../recipe.js';
import { seededRandom } from '../random.js';
import { centroidWavelength } from '../spectrum.js';

const names = Object.keys(PRESETS) as PresetName[];
const gridFor = (n: PresetName) => (n.startsWith('xuan') ? FIT_GRID.xuan : FIT_GRID.cotton);

describe('fitted presets', () => {
  for (const name of names) {
    it(`${name} reaches its porosity target`, () => {
      expect(Math.abs(bulkPorosity(generatePaper(PRESETS[name], gridFor(name), 1)) - PRESET_TARGETS[name].porosity)).toBeLessThan(0.02);
    });
    it(`${name} carries a density closure at the engine's texel size`, () => {
      expect(PRESETS[name].densityClosures.some((c) => Math.abs(c.cell - ENGINE_TEXEL) <= 0.05 * ENGINE_TEXEL)).toBe(true);
    });
  }
  // The closures are pasted from a fit run; this catches them going stale
  // when anything upstream of them changes.
  for (const name of ['cotton-cold-press', 'xuan-unsized', 'xuan-sized'] as const) {
    it(`${name}'s pasted closure is what the fitter gives today`, () => {
      const pasted = PRESETS[name].densityClosures[0], fresh = fitDensityClosure(PRESETS[name], ENGINE_TEXEL, 1);
      expect(fresh.mean).toBeCloseTo(pasted.mean, 1); expect(fresh.slope).toBeCloseTo(pasted.slope, 1);
      expect(fresh.residual).toBeCloseTo(pasted.residual, 3); expect(fresh.residualCorrelation).toBeCloseTo(pasted.residualCorrelation, 2);
    }, 30_000);
  }
  it('gives cotton felt an imprint near the estimated 1 mm wavelength', () => {
    const felt = PRESETS['cotton-cold-press'].felt!, g = { width: 512, height: 512, cell: 20e-6 };
    const wl = centroidWavelength(feltSurface(felt, g, seededRandom(1)), 512, 512, g.cell);
    expect(wl).toBeGreaterThan(0.75e-3); expect(wl).toBeLessThan(1.33e-3);
  });
});
