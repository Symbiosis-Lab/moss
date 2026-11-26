// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { createSim } from '../solver.js';

it('pigmentAt shares playback lookup, clamping, partial recording and tint invalidation', () => {
  let id = 0;
  const values: Record<string, unknown> = { LINK_STATUS: 1, ACTIVE_UNIFORMS: 2 };
  const gl = new Proxy(values, { get(target, key: string) {
    if (key in target) return target[key];
    if (key === 'getProgramParameter') return (_: unknown, which: number) => which === 1;
    if (key === 'getExtension') return () => ({ loseContext() {} });
    if (key === 'getShaderParameter') return () => true;
    if (key.startsWith('create')) return () => ({ id: ++id });
    if (/^[A-Z_0-9]+$/.test(key)) return target[key] = ++id + 100;
    return () => ({});
  } });
  const canvas = document.createElement('canvas');
  vi.spyOn(canvas, 'getContext').mockReturnValue(gl as unknown as WebGL2RenderingContext);
  const context2d = { drawImage() {}, imageSmoothingQuality: 'high' };
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(context2d as unknown as CanvasRenderingContext2D);
  const sim = createSim({ canvas, texW: 8, texH: 8, divisor: 1, tint: '#fff',
    rect: () => ({ x: 0, y: 0, w: 8, h: 8 }), preset: { recDuration: 4 / 120, recEvery: 2 } })!;
  const print = document.createElement('canvas'), side = sim.sceneSide(print, print);
  expect(sim.pigmentAt(side, .5)).toBeNull();
  sim.recordSide(side, 0);
  const initial = sim.pigmentAt(side, 0)!;
  expect(initial.k0).toBe(initial.k1);
  sim.recordSide(side, 2);
  const half = sim.pigmentAt(side, .25)!;
  expect(half.k0).toBe(initial.k0); expect(half.k1).not.toBe(initial.k0); expect(half.f).toBe(.5);
  expect(sim.pigmentAt(side, 1)).toEqual({ k0: half.k1, k1: half.k1, f: 0 });
  expect(sim.pigmentAt(side, -1)).toEqual({ k0: half.k0, k1: half.k1, f: 0 });
  sim.recordSide(side, 2);
  expect(sim.sideRecorded(side)).toBe(true);
  expect(sim.pigmentAt(side, 2)).toEqual(sim.pigmentAt(side, 1));
  sim.setTint('#111');
  expect(sim.pigmentAt(side, .5)).toBeNull();
  sim.recordSide(side, 4);
  expect(sim.pigmentAt(side, .5)).not.toBeNull();
  sim.freeSide(side);
  expect(sim.pigmentAt(side, .5)).toBeNull();
  sim.dispose();
  vi.restoreAllMocks();
});
