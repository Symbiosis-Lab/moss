import { describe, expect, it } from 'vitest';
import { ENGINE_TEXEL } from '../engine.js';
import { bulkPorosity } from '../fields.js';
import { FIT_GRID, bisect } from './fit.js';
import { generatePaper } from '../generate.js';
import { feltSurface } from '../press.js';
import { fitDensityClosure } from '../resample.js';
import { PRESETS, PRESET_TARGETS, type PaperRecipe } from '../recipe.js';
import { seededRandom } from '../random.js';
import { centroidWavelength } from '../spectrum.js';

const porosity = (r: PaperRecipe, xuan: boolean) => bulkPorosity(generatePaper(r, xuan ? FIT_GRID.xuan : FIT_GRID.cotton, 1));

describe.runIf(process.env.FIT_PAPER)('fit presets (prints values to paste into recipe.ts)', () => {
  it('fits flexibility, compaction, felt conformity and closures', () => {
    const out: Record<string, unknown> = {};
    const cp = PRESETS['cotton-cold-press'];
    const tf = bisect((f) => porosity({ ...cp, flexibility: f }, false), 0.05, 40, PRESET_TARGETS['cotton-cold-press'].porosity);
    out.cottonFlexibility = tf;
    for (const n of ['cotton-rough', 'cotton-hot-press'] as const)
      out[`${n}.compaction`] = bisect((c) => porosity({ ...PRESETS[n], flexibility: tf, press: { ...PRESETS[n].press, compaction: c } }, false), 0.3, 1, PRESET_TARGETS[n].porosity);
    const xu = PRESETS['xuan-unsized'];
    const tx = bisect((f) => porosity({ ...xu, flexibility: f }, true), 0.05, 40, PRESET_TARGETS['xuan-unsized'].porosity);
    out.xuanFlexibility = tx;
    out['xuan-sized.compaction'] = bisect((c) => porosity({ ...PRESETS['xuan-sized'], flexibility: tx, press: { ...PRESETS['xuan-sized'].press, compaction: c } }, true), 0.3, 1, PRESET_TARGETS['xuan-sized'].porosity);
    const g = { width: 512, height: 512, cell: 20e-6 };
    out.feltConformity = bisect((c) => centroidWavelength(feltSurface({ ...cp.felt!, conformity: c }, g, seededRandom(1)), 512, 512, g.cell), 0.02e-3, 2e-3, 1e-3);
    for (const n of Object.keys(PRESETS) as (keyof typeof PRESETS)[])
      out[`${n}.closure`] = fitDensityClosure({ ...PRESETS[n], flexibility: n.startsWith('xuan') ? tx : tf }, ENGINE_TEXEL, 1);
    console.log(JSON.stringify(out, null, 2));
  }, 600_000);
});

// A report for a person to read, not a check: the gaps it shows are recorded
// in the spec's risks.
describe.runIf(process.env.FIT_PAPER)('literature report', () => {
  it('prints the unfitted predictions next to their measured values', () => {
    const xu = generatePaper(PRESETS['xuan-unsized'], FIT_GRID.xuan, 1);
    const diam = Array.from(xu.capillaryRadius, (r) => 2 * r * 1e6), bins = new Map<number, number>();
    for (const d of diam) { const b = Math.round(d); bins.set(b, (bins.get(b) ?? 0) + 1); }
    const mode = [...bins.entries()].sort((a, b) => b[1] - a[1])[0][0];
    const cp = generatePaper(PRESETS['cotton-cold-press'], FIT_GRID.cotton, 1);
    const mean = (a: Float32Array) => a.reduce((s, v) => s + v, 0) / a.length;
    const rows = [
      { quantity: 'xuan-unsized pore diameter mode (µm)', model: mode, measured: 14.29, source: 'Shao et al. 2019' },
      { quantity: 'cotton-cold-press permeability (m²)', model: mean(cp.permeability.xx), measured: '1e-13–1e-11', source: 'derived estimate' },
      { quantity: 'cotton-cold-press Washburn (mm/√s)', model: mean(cp.washburn) * 1e3, measured: '1–10', source: 'generic paper strips' },
      { quantity: 'cotton-cold-press density (g/cm³)', model: (1 - bulkPorosity(cp)) * 1.5, measured: 0.52, source: 'mould-made cotton rag' },
    ];
    console.table(rows);
  }, 120_000);
});
