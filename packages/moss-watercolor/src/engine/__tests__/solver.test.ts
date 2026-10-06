// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { createPaper } from '../../paper/default.js';
import { createSim } from '../solver.js';

const rect = () => ({ x: 0, y: 0, w: 1, h: 1 });

describe('createSim', () => {
  it('refuses a paper that is not 256x256, since the shaders tile at that period', () => {
    const canvas = { getContext: vi.fn() } as unknown as HTMLCanvasElement;
    expect(() => createSim({ canvas, texW: 8, texH: 8, divisor: 1, rect, paper: createPaper({ width: 128, height: 128 }) })).toThrow(/256x256.*shader constant/);
    expect(canvas.getContext).not.toHaveBeenCalled();
  });

  it('adds no webglcontextlost listener until a handler is registered', () => {
    // A context whose shader compile fails: createSim gets as far as setting
    // the canvas up, then returns null. Nothing may have been attached to the
    // canvas by then, so a canvas that never asks for handlers behaves as it
    // did before the sim touched it.
    const gl = new Proxy({}, { get: (_t, name) => name === 'createShader' ? () => { throw new Error('compile unavailable'); } : () => ({}) });
    const canvas = document.createElement('canvas');
    vi.spyOn(canvas, 'getContext').mockReturnValue(gl as unknown as WebGL2RenderingContext);
    const add = vi.spyOn(canvas, 'addEventListener');
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(createSim({ canvas, texW: 8, texH: 8, divisor: 1, rect, paper: createPaper() })).toBeNull();
    expect(add).not.toHaveBeenCalled();
  });
});
