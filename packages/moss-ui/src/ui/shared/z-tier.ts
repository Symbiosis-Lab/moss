/**
 * Read one of `tokens.css`'s `--z-*` stacking-scale tokens as an integer.
 *
 * Lazy and cached per token — not resolved at module init, because its
 * callers (tooltip.ts, the host's panel divider) are imported transitively
 * into node-environment unit tests that have no `document`/`getComputedStyle`
 * at all. `tokens.css` is the one place the scale is declared; this only
 * ever reads it, never restates a tier as a number of its own.
 */
const cache = new Map<string, number>();

export function zTier(customProperty: string, fallback: number): number {
  let n = cache.get(customProperty);
  if (n === undefined) {
    n = parseInt(getComputedStyle(document.documentElement).getPropertyValue(customProperty), 10);
    if (Number.isNaN(n)) n = fallback;
    cache.set(customProperty, n);
  }
  return n;
}
