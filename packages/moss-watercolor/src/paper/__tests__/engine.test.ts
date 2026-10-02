import { describe, expect, it } from 'vitest';
import { ENGINE_SPANS, enginePaper, toEngineChannels } from '../engine.js';
import { generatePaper } from '../generate.js';
import { PRESETS } from '../recipe.js';

describe('toEngineChannels', () => {
  const sheet = generatePaper(PRESETS['cotton-cold-press'], { width: 64, height: 64, cell: 10e-6 }, 1), n = 64 * 64;
  const tex = toEngineChannels(sheet);
  it('packs RGBA bytes at the sheet size', () => {
    expect(tex.data).toBeInstanceOf(Uint8Array); expect(tex.data.length).toBe(n * 4);
  });
  it('puts each field in the channel its shader reads, and leaves pore reserved', () => {
    const ramp = (lo: number, hi: number, up: boolean) => Float32Array.from({ length: n }, (_, i) => lo + ((up ? i : n - 1 - i) / (n - 1)) * (hi - lo));
    const k = ramp(1e-11, 1e-10, false);
    const t = toEngineChannels({ ...sheet, thickness: ramp(400e-6, 500e-6, true), washburn: ramp(0.005, 0.02, false), permeability: { xx: k, xy: k, yy: k } }).data;
    const last = (n - 1) * 4;
    expect([t[0] < t[last], t[1] > t[last + 1], t[2] > t[last + 2]]).toEqual([true, true, true]);
    for (let i = 0; i < n; i++) expect(t[i * 4 + 3]).toBe(128);
  });
  it('decodes back to the physical field within a byte', () => {
    const { lo, hi } = ENGINE_SPANS.absorbency, step = (hi - lo) / 255;
    for (let i = 0; i < n; i += 97) expect(Math.abs(lo + (tex.data[i * 4 + 1] / 255) * (hi - lo) - sheet.washburn[i])).toBeLessThanOrEqual(step / 2 + 1e-12);
  });
});

describe('enginePaper', () => {
  it('builds a true-scale coarse texture from the preset closure, and refuses without one', () => {
    const recipe = PRESETS['xuan-unsized'];
    expect(() => enginePaper({ ...recipe, densityClosures: [] }, { texels: 32 })).toThrow(/fitDensityClosure/);
    expect([enginePaper(recipe, { texels: 32 }).width, enginePaper(recipe, { texels: 32 }).height]).toEqual([32, 32]);
  });
  it('keeps each paper physical: rough cotton carries several times the relief of hot-press', () => {
    const sd = (name: 'cotton-rough' | 'cotton-hot-press') => {
      const d = enginePaper(PRESETS[name], { texels: 64 }).data, r = Array.from({ length: 64 * 64 }, (_, i) => d[i * 4]);
      const m = r.reduce((a, b) => a + b, 0) / r.length; return Math.sqrt(r.reduce((a, b) => a + (b - m) ** 2, 0) / r.length);
    };
    expect(sd('cotton-rough')).toBeGreaterThan(3 * sd('cotton-hot-press'));
  });
  it('carries a felted sheet to engine scale, felt included', () => {
    const recipe = PRESETS['cotton-cold-press'];
    const felted = enginePaper(recipe, { texels: 32 }), bare = enginePaper({ ...recipe, felt: null }, { texels: 32 });
    expect(Buffer.from(felted.data.buffer).equals(Buffer.from(bare.data.buffer))).toBe(false);
  });
});
