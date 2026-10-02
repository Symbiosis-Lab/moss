import { describe, expect, it } from 'vitest';
import { bisect } from './fit.js';

describe('bisect', () => {
  it('solves increasing and decreasing monotone functions', () => {
    expect(bisect((x) => x * x, 0, 4, 2, 40)).toBeCloseTo(Math.SQRT2, 6);
    expect(bisect((x) => 1 / x, 0.1, 10, 0.5, 40)).toBeCloseTo(2, 6);
  });
  it('refuses a target it cannot bracket', () => {
    expect(() => bisect((x) => x, 0, 1, 5)).toThrow(RangeError);
  });
});
