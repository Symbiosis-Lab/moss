/**
 * True when `el`'s inline axis runs top-to-bottom instead of left-to-right —
 * i.e. `writing-mode: vertical-rl`/`vertical-lr`. Every script that measures
 * or positions along "the" axis text flows on (the font-selector panel in
 * theme.ts, the breadcrumb fold in nav/breadcrumb-fold.ts) needs this same
 * check; one owner here is what keeps it from being re-derived, and
 * potentially gotten wrong, at each site.
 */
export function isVertical(el: Element): boolean {
  return getComputedStyle(el).writingMode.startsWith("vertical");
}
