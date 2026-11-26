import { expect, it } from 'vitest';
import { registerSim, contextFor, forgetSim } from '../../internal/context.js';
import type { WatercolorSim } from '../../engine/index.js';

it('isolates contexts by sim identity without attaching a public handle', () => {
  const a = {} as WatercolorSim, b = {} as WatercolorSim;
  const first = { gl: {} as WebGL2RenderingContext, width: 205, height: 195,
    ground: 'transparent' as const, page: { tint: [1, 1, 1] as [number, number, number], dark: false } };
  const second = { ...first, gl: {} as WebGL2RenderingContext };
  registerSim(a, first); registerSim(b, second);
  expect(contextFor(a)).toBe(first);
  expect(contextFor(b)).toBe(second);
  expect(Object.keys(a)).toEqual([]);
  expect(() => contextFor({} as WatercolorSim)).toThrow(/unavailable/);
  forgetSim(a);
  expect(() => contextFor(a)).toThrow(/unavailable/);
  expect(contextFor(b)).toBe(second);
});
