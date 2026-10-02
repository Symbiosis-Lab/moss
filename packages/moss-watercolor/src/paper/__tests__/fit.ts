// How presets are fitted to their literature targets: bisection on one
// parameter with the others fixed, on a fixed seed and grid.
import type { Grid } from '../deposit.js';

export const FIT_GRID: Record<'cotton' | 'xuan', Grid> = {
  cotton: { width: 512, height: 512, cell: 10e-6 },
  xuan: { width: 1024, height: 1024, cell: 5e-6 },
};

export function bisect(f: (x: number) => number, lo: number, hi: number, target: number, iterations = 20): number {
  let flo = f(lo);
  const fhi = f(hi);
  if ((flo - target) * (fhi - target) > 0) throw new RangeError(`target ${target} is not between f(${lo}) = ${flo} and f(${hi}) = ${fhi}`);
  for (let i = 0; i < iterations; i++) {
    const mid = (lo + hi) / 2, fm = f(mid);
    if ((fm - target) * (flo - target) <= 0) hi = mid; else { lo = mid; flo = fm; }
  }
  return (lo + hi) / 2;
}
