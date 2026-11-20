import { clamp01, smooth } from '../engine/math.js';

export interface SphereBounds {
  outEnd: number;
  holdStart: number;
  holdEnd: number;
  inStart: number;
}
export const DEFAULT_SPHERE_BOUNDS: Readonly<SphereBounds> = {
  outEnd: .18, holdStart: .34, holdEnd: .66, inStart: .82,
};

/** Five phases; the host owns time through Out/Gather/Release/In and scroll through Hold.
 * Sample both sides at the returned dissolves so the wash and grains share a frame.
 * Grain reaches 1 before lift leaves 0, and lift returns to 0 before grain fades.
 */
export function sphereLegAt(p: number, bounds: SphereBounds = DEFAULT_SPHERE_BOUNDS) {
  const { outEnd, holdStart, holdEnd, inStart } = bounds;
  if (!Number.isFinite(p)) throw new RangeError('sphereLegAt: p must be finite');
  if (!(0 < outEnd && outEnd < holdStart && holdStart < holdEnd && holdEnd < inStart && inStart < 1)) {
    throw new RangeError('sphereLegAt: bounds must be ordered strictly inside (0, 1)');
  }
  p = clamp01(p);
  const gather = (outEnd + holdStart) / 2, release = (holdEnd + inStart) / 2;
  const up = smooth(gather, holdStart, p), down = smooth(holdEnd, release, p);
  const grain = smooth(outEnd, gather, p) * (1 - smooth(release, inStart, p));
  // The middle samples retain separated hues; the full Out and In still play.
  const ux = p <= outEnd ? p / outEnd : 1 - .45 * smooth(outEnd, gather, p) + .45 * smooth(holdEnd, inStart, p);
  const uy = p >= inStart ? (1 - p) / (1 - inStart) : 1 - .45 * smooth(outEnd, holdStart, p) + .45 * smooth(release, inStart, p);
  return { ux, uy, k: smooth(holdStart, holdEnd, p), lift: up * (1 - down), grain };
}
