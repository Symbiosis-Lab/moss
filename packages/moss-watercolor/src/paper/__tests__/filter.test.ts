import { describe, expect, it } from 'vitest';
import { boxBlurWrap, gaussianBlurWrap } from '../filter.js';

const sum = (a: Float32Array) => a.reduce((s, v) => s + v, 0);

describe('boxBlurWrap', () => {
  it('conserves the total and wraps across both edges', () => {
    const W = 16, H = 8, a = new Float32Array(W * H); a[0] = 1;
    const b = boxBlurWrap(a, W, H, 1);
    expect(sum(b)).toBeCloseTo(1, 6);
    expect(b[W - 1]).toBeCloseTo(1 / 9, 6);          // left neighbour across the x seam
    expect(b[(H - 1) * W]).toBeCloseTo(1 / 9, 6);    // neighbour across the y seam
  });
  it('clamps a radius wider than the tile instead of double counting', () => {
    const a = new Float32Array(4 * 4).fill(2);
    expect([...boxBlurWrap(a, 4, 4, 10)].every((v) => Math.abs(v - 2) < 1e-6)).toBe(true);
  });
});

describe('gaussianBlurWrap', () => {
  it('spreads a point to roughly the requested variance', () => {
    const W = 128, H = 1, a = new Float32Array(W); a[64] = 1;
    const b = gaussianBlurWrap(a, W, H, 6);
    let m = 0, v = 0; for (let x = 0; x < W; x++) m += x * b[x];
    for (let x = 0; x < W; x++) v += (x - m) * (x - m) * b[x];
    expect(Math.sqrt(v) / 6).toBeGreaterThan(0.85); expect(Math.sqrt(v) / 6).toBeLessThan(1.15);
  });
  it('passes a field through when sigma is below one cell instead of over-smoothing it', () => {
    const a = new Float32Array(16 * 16); a[17] = 1;
    expect(gaussianBlurWrap(a, 16, 16, 0.3)[17]).toBe(1);
  });
});
