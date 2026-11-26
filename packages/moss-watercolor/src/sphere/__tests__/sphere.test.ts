import { describe, expect, it, vi } from 'vitest';
import { createSphere } from '../index.js';
import { registerSim } from '../../internal/context.js';
import type { PigmentSample, WatercolorSim } from '../../engine/solver.js';
import { mockGL } from './gl.js';

function setup() {
  const { gl, snapshot } = mockGL(), sim = {} as WatercolorSim;
  const page = { tint: [1, 1, 1] as [number, number, number], dark: false };
  registerSim(sim, { gl, width: 205, height: 195, page, ground: 'transparent' });
  const sample: PigmentSample = { k0: { s: {}, d: {} }, k1: { s: {}, d: {} }, f: .25 };
  return { gl, snapshot, sim, page, sample };
}
const hold = { k: .5, lift: 1, grain: 1, t: 3 };

describe('sphere draw', () => {
  it('restores every changed state and draws one point per cell', () => {
    const { gl, snapshot, sim, sample } = setup(), before = snapshot();
    const sphere = createSphere(sim);
    expect(snapshot()).toEqual(before);
    expect(sphere.draw(sample, sample, hold)).toBe(true);
    expect(snapshot()).toEqual(before);
    expect(gl.drawArrays).toHaveBeenLastCalledWith(gl.POINTS, 0, 205 * 195);
    expect(gl.uniform1f).toHaveBeenCalledWith('uFx', .25);
    expect(gl.uniform1f).toHaveBeenCalledWith('uFy', .25);
  });
  it('restores state even when a draw throws', () => {
    const { gl, snapshot, sim, sample } = setup(), sphere = createSphere(sim), before = snapshot();
    vi.mocked(gl.drawArrays).mockImplementationOnce(() => { throw new Error('draw interrupted'); });
    expect(() => sphere.draw(sample, sample, hold)).toThrow('draw interrupted');
    expect(snapshot()).toEqual(before);
  });
  it('does nothing at zero grain or with missing pigment', () => {
    const { gl, snapshot, sim, sample } = setup(), sphere = createSphere(sim), before = snapshot();
    expect(sphere.draw(sample, sample, { ...hold, grain: 0, lift: 0 })).toBe(true);
    expect(sphere.draw(null, null, hold)).toBe(false);
    expect(gl.drawArrays).not.toHaveBeenCalled();
    expect(snapshot()).toEqual(before);
  });
  it('uses updated tint, honours stride and frees its resources only once', () => {
    const { gl, sim, sample, page } = setup(), sphere = createSphere(sim, { stride: 3 });
    page.tint = [.1, .2, .3]; page.dark = true;
    sphere.draw(sample, sample, hold);
    expect(gl.uniform3fv).toHaveBeenCalledWith('uTint', page.tint);
    expect(gl.uniform1f).toHaveBeenCalledWith('uDark', 1);
    expect(gl.drawArrays).toHaveBeenLastCalledWith(gl.POINTS, 0, Math.ceil(205 * 195 / 3));
    sphere.dispose(); sphere.dispose();
    expect(gl.deleteProgram).toHaveBeenCalledTimes(1);
    expect(gl.deleteVertexArray).toHaveBeenCalledTimes(1);
    expect(sphere.draw(sample, sample, hold)).toBe(false);
  });
  it('rejects invalid options and releases resources on compile failure', () => {
    const { gl, sim } = setup();
    expect(() => createSphere(sim, { stride: .5 })).toThrow(/stride/);
    expect(() => createSphere(sim, { pointSize: NaN })).toThrow(/pointSize/);
    gl.getShaderParameter = () => false;
    gl.getShaderInfoLog = () => 'bad shader';
    expect(() => createSphere(sim)).toThrow('bad shader');
    expect(gl.deleteProgram).toHaveBeenCalledTimes(1);
    expect(gl.deleteShader).toHaveBeenCalledTimes(1);
  });
});
