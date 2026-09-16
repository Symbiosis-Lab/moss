// Pure geometry for editor→preview line-based scroll sync.
//
// Extracted so the real bridge code path is unit-tested directly (the bridge
// itself is an injected IIFE that can't be imported in jsdom). See
// frontend/app/preview/__tests__/iframe-bridge-scroll-sync.test.ts.

/**
 * Absolute document scrollTop to place a source line that falls BETWEEN two
 * annotated elements — `best` (the nearest annotated line ≤ target) and `next`
 * (the nearest annotated line > target).
 *
 * The span MUST be the visual distance between the two anchors
 * (`nextTop - bestTop`), NOT `best`'s own height. An UNannotated tall block —
 * e.g. an `![[folder-to-site.html]]` HTML embed rendered as a full-width 16:9
 * iframe with no `data-source-line` — can sit in the gap between the two
 * anchors. Using `best.offsetHeight` (a ~40px paragraph) as the span then
 * grossly under-measures the real ~450px distance, so the target lands just
 * below `best`, never crossing the iframe. That undershoot parks the preview
 * near the top of the region; combined with the preview→editor echo it drives a
 * top↔bottom scroll oscillation. Spanning `nextTop - bestTop` walks across the
 * iframe correctly.
 *
 * All inputs are in the same coordinate space: `scrollY` = current
 * `window.scrollY`; `bestTop`/`nextTop` = `getBoundingClientRect().top`
 * (viewport-relative) of the two anchors.
 *
 * @param scrollY  current window.scrollY
 * @param bestTop  viewport-relative top of the `best` anchor
 * @param nextTop  viewport-relative top of the `next` anchor
 * @param fraction (targetLine - bestLine) / (nextLine - bestLine), in [0, 1]
 */
export function interpolatedScrollTop(
  scrollY: number,
  bestTop: number,
  nextTop: number,
  fraction: number,
): number {
  // Guard a degenerate/inverted anchor pair (next visually at or above best):
  // fall back to landing at `best` rather than scrolling upward past it.
  const span = Math.max(0, nextTop - bestTop);
  return scrollY + bestTop + span * fraction;
}

/**
 * Is a caret-driven sync target already on screen, close enough that moving the
 * page would be a twitch rather than a service?
 *
 * The caret half of editor→preview sync fires on every keystroke. Centring the
 * target each time would make the preview jitter continuously while the author
 * typed — worse than a preview that never followed at all. So the editor asks
 * with `onlyIfOffscreen`, and this decides: the editor cannot, because it has
 * no idea what the preview is showing.
 *
 * The band is deliberately generous. Anything in the middle 70% counts as
 * already showing; only a target at the very edge, or off screen, is worth a
 * scroll. A narrow band would re-centre constantly as the caret drifted down a
 * paragraph — the same twitch, arriving more slowly.
 *
 * Scroll-driven syncs never consult this. There, continuous tracking IS the
 * feature.
 *
 * @param top             viewport-relative `getBoundingClientRect().top` of the target
 * @param viewportHeight  `window.innerHeight`
 */
export function isAlreadyShowing(top: number, viewportHeight: number): boolean {
  const margin = viewportHeight * QUIET_BAND;
  return top >= margin && top <= viewportHeight - margin;
}

/** Fraction of the viewport at each edge that does NOT count as "showing". */
export const QUIET_BAND = 0.15;
