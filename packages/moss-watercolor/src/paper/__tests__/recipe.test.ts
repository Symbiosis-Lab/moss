import { describe, expect, it } from 'vitest';
import { COTTON, PRESETS, PRESET_TARGETS, coarseness, fibreThickness, meanFibreLength, minFibreWidth, type PresetName } from '../recipe.js';

describe('fibre geometry', () => {
  it('derives coarseness from two collapsed walls', () => {
    expect(coarseness(COTTON)).toBeCloseTo(2 * 20e-6 * 4e-6 * 1500, 12); // 0.24 mg/m
    expect(fibreThickness(COTTON)).toBeCloseTo(8e-6, 12);
  });
});

describe('presets', () => {
  const names = Object.keys(PRESETS) as PresetName[];
  it('are the five sheets the spec names', () => {
    expect(names.sort()).toEqual(['cotton-cold-press', 'cotton-hot-press', 'cotton-rough', 'xuan-sized', 'xuan-unsized']);
  });
  it('have furnish fractions summing to one and a porosity target each', () => {
    for (const n of names) {
      expect(PRESETS[n].furnish.reduce((s, f) => s + f.massFraction, 0)).toBeCloseTo(1, 9);
      expect(PRESET_TARGETS[n].porosity).toBeGreaterThan(0);
    }
  });
  it('mix xuan from Pteroceltis and rice straw', () => {
    expect(minFibreWidth(PRESETS['xuan-unsized'])).toBeCloseTo(10e-6, 12);
    expect(meanFibreLength(PRESETS['xuan-unsized'])).toBeCloseTo(0.7 * 2.45e-3 + 0.3 * 1.2e-3, 9);
  });
});
