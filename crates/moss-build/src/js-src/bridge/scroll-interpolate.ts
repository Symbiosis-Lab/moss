// Pure geometry for editor→preview line-based scroll sync.
//
// Extracted so the real bridge code path is unit-tested directly (the bridge
// itself is an injected IIFE that can't be imported in jsdom). See
// __tests__/iframe-bridge-scroll-sync.test.ts.

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

/**
 * Viewport fraction of the sync "focus line" (0 = top, 0.5 = centre). The
 * editor anchors the same source line at the same fraction of its own
 * viewport, so the two values must stay equal.
 */
export const FOCUS_FRACTION = 0.5;

/** Sub-pixel scroll positions and elastic overscroll mean scrollY may not be
 *  exactly 0 at the top; 4px is below any visible chrome and above rounding
 *  error in WKWebView. The editor uses the same threshold for its own top. */
const TOP_THRESHOLD = 4;
/** Symmetric slack for the bottom edge. */
const BOTTOM_THRESHOLD = 4;

/** The source line an annotated element was rendered from (a range reports its
 *  first line), or 0 for an element that carries no annotation. */
export function getSourceLine(el: Element): number {
  const lineAttr = el.getAttribute('data-source-line');
  if (lineAttr) return parseInt(lineAttr, 10);
  const rangeAttr = el.getAttribute('data-source-range');
  if (rangeAttr) return parseInt(rangeAttr.split('-')[0], 10);
  return 0;
}

/** Where the preview is: the source line at its focus line, plus the two edge
 *  signals the editor honours before the line. */
export interface ScrollPosition {
  line: number;
  atTop: boolean;
  atBottom: boolean;
}

/**
 * Read where the preview is, the same way whether the user just scrolled it or
 * the editor is asking because it opened.
 *
 * `line` is the annotated element ON the focus line — the last one whose top is
 * at or above it — so the editor aligns that line at its own focus line. Above
 * every annotation (near the page top) the topmost visible one stands in; with
 * none at all it is 0, or 1 at the very top, where `atTop` is what the editor
 * actually acts on.
 *
 * `atTop` and `atBottom` are authoritative and independent of annotations.
 * Page chrome (a site header, a hero) and the last screenful (a tall footer, an
 * unannotated embed) carry no `data-source-line`, so a line alone cannot say
 * "the page is at its top" or "at its end"; without the flags the editor would
 * chase a line near the edge instead. A page too short to scroll sets both, and
 * the editor resolves `atTop` first.
 */
export function readScrollPosition(win: Window): ScrollPosition {
  const doc = win.document;
  const focusY = win.innerHeight * FOCUS_FRACTION;
  let focusEl: Element | null = null;
  let focusElTop = -Infinity;
  let topEl: Element | null = null;
  let topElTop = Infinity;
  for (const el of doc.querySelectorAll('[data-source-line], [data-source-range]')) {
    const top = el.getBoundingClientRect().top;
    if (top <= focusY && top > focusElTop) {
      focusElTop = top;
      focusEl = el;
    }
    if (top >= -10 && top < topElTop) {
      topElTop = top;
      topEl = el;
    }
  }
  const chosen = focusEl ?? topEl;
  const atTop = win.scrollY < TOP_THRESHOLD;
  const maxScroll = doc.documentElement.scrollHeight - win.innerHeight;
  const atBottom = win.scrollY >= maxScroll - BOTTOM_THRESHOLD;
  let line = chosen ? getSourceLine(chosen) : 0;
  if (atTop && line === 0) line = 1;
  return { line, atTop, atBottom };
}
