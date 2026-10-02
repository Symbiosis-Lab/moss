import { describe, expect, it } from 'vitest';
import { generatePaper } from '../generate.js';
import { PRESETS } from '../recipe.js';
import { shadePaper } from '../shade.js';

describe('shadePaper', () => {
  it('renders flat paper as the plain paper tint', () => {
    const s = generatePaper(PRESETS['cotton-hot-press'], { width: 16, height: 16, cell: 10e-6 }, 1);
    const flat = { ...s, thickness: new Float32Array(256).fill(1e-4) };
    const px = shadePaper(flat, { azimuth: 0, elevation: Math.PI / 6 });
    expect([px[0], px[1], px[2], px[3]]).toEqual([0xf4, 0xf1, 0xed, 255]);
  });
  it('lights slopes facing the light more than slopes facing away', () => {
    const s = generatePaper(PRESETS['cotton-rough'], { width: 16, height: 16, cell: 10e-6 }, 1);
    const ramp = { ...s, thickness: Float32Array.from({ length: 256 }, (_, i) => (i % 16) * 1e-6) };
    const lit = shadePaper(ramp, { azimuth: Math.PI, elevation: Math.PI / 6 })[5 * 4], away = shadePaper(ramp, { azimuth: 0, elevation: Math.PI / 6 })[5 * 4];
    expect(lit).toBeGreaterThan(away);
  });
  const base = generatePaper(PRESETS['cotton-rough'], { width: 16, height: 16, cell: 10e-6 }, 1);
  const ramp = (f: (x: number, y: number) => number) => ({ ...base, thickness: Float32Array.from({ length: 256 }, (_, i) => f(i % 16, Math.floor(i / 16)) * 1e-6) });
  const light = { azimuth: Math.PI, elevation: Math.PI / 6 };
  it('wraps the thickness gradient across the x=0 edge', () => {
    // At x=0 the neighbours are x=15 and x=1, a steep slope the other way; a clamped or unwrapped read would look like the interior.
    const px = shadePaper(ramp((x) => x), light, 1);
    expect(px[0]).toBeLessThan(px[5 * 4]);
  });
  it('lights a ramp along y by the y component of the light', () => {
    const t = ramp((_, y) => y);
    const facing = shadePaper(t, { azimuth: -Math.PI / 2, elevation: Math.PI / 6 }, 1)[(5 * 16 + 5) * 4], away = shadePaper(t, { azimuth: Math.PI / 2, elevation: Math.PI / 6 }, 1)[(5 * 16 + 5) * 4];
    const sideways = shadePaper(t, { azimuth: 0, elevation: Math.PI / 6 }, 1)[(5 * 16 + 5) * 4];
    expect(facing).toBeGreaterThan(sideways);
    expect(sideways).toBeGreaterThan(away);
  });
  it('scales the shading with exaggeration', () => {
    const t = ramp((x) => x), away = { azimuth: 0, elevation: Math.PI / 6 }, at = (e: number) => 0xf4 - shadePaper(t, away, e)[5 * 4];
    expect(at(4)).toBeGreaterThan(at(1));
    expect(at(0)).toBe(0);
  });
});
