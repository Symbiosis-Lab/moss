/**
 * breadcrumb-fold.ts — ONE folding behaviour for both breadcrumb trails.
 *
 * The floating island and the masthead show the same trail and
 * used to shrink it two different ways: the island folded whole ancestors
 * into a `…` button, the masthead ellipsised every middle segment down to an
 * unreadable stub ("獎.. / 寫.. / 第.."). The fold is the better answer —
 * half an ancestor name tells the reader nothing — so the algorithm moved
 * here and both bars consume it. `nav-island.ts` and `masthead-fold.ts` own
 * their trails' wiring (reveal, panels' mutual exclusion, teardown); this
 * module owns the decisions they must not be allowed to make differently:
 * what folds, in what order, at what floor, and where a panel lands.
 *
 * The pure half is {@link foldPlan}, which takes measurements and returns a
 * decision. It is exported so it can be tested without a browser: the
 * *measuring* is the engine's job, the *decision* made from it is ours.
 * The geometry only an engine can answer — that a row never wraps and never
 * overflows — is asserted in tests/render-gates/site/nav-island.spec.ts.
 */

import { isVertical } from "../writing-mode";

/** Sub-pixel slack: fractional layout widths make an exact compare flicker. */
const SLACK = 1;

/**
 * How short the trail's last crumb may get before folding an ancestor is the
 * better trade — enough of it left to recognise the page.
 *
 * The number was measured on CJK, where 7em is seven characters. A Latin title
 * averages about half an em per glyph, so the same 7em is nearer fourteen
 * characters. Anyone retuning this should know which script it was read on.
 *
 * There are deliberately TWO numbers here, and they mean different things:
 *
 * - this one decides **when to fold**. Squeeze the last crumb to seven
 *   characters before dropping an ancestor; below that, drop the ancestor
 *   instead.
 * - the `min-width` CSS puts on that crumb (3em on the island's current
 *   crumb, 5em/3em on the masthead's parent segment) is the **hard clamp**.
 *   When every ancestor has already folded there is nothing left to drop, so
 *   the crumb takes whatever room is left, and the clamp is the point at
 *   which it stops shrinking and the row simply cannot get narrower.
 *
 * Collapsing them into one number breaks at one end or the other: 3em as the
 * fold threshold means a 420px screen shows four full ancestors and `末代…`,
 * and 7em as the clamp means a 330px screen overflows because the row has run
 * out of ways to shrink.
 */
export const FOLD_FLOOR_EM = 7;

/**
 * Decide which ancestors have to fold.
 *
 * `widths` is every crumb's natural width, in trail order. The first entry is
 * the site name and the last is the trail's final crumb (the current page on
 * the island, the parent page on the masthead — which skips the current
 * page); both always survive — they are the two ends of "where am I".
 * Everything between is foldable, and is sacrificed **left to right**:
 * `第一屆` locates a reader better than `評選` does, so the ancestor nearest
 * them is the last to go.
 *
 * Returns the indices to hide, ascending. Empty means the trail fits as it is.
 *
 * The last crumb is charged its FLOOR, not its natural width, when asking
 * whether a plan fits — because it is the one crumb allowed to ellipsise.
 * Charging it full width would make the trail buy room it
 * already has by dropping an ancestor, which is the more expensive of the two:
 * a truncated title still shows its first characters and the reader knows what
 * page they are on, whereas a dropped ancestor leaves the screen entirely.
 * Fold is the last resort, not the first.
 *
 * @param widths        natural width of each crumb, in order
 * @param separator     width of one `/` including its margins
 * @param more          width of the `…` button (charged only if anything folds)
 * @param available     the width the trail has to fit inside
 * @param currentFloor  narrowest the last crumb is still worth showing at —
 *                      see `FOLD_FLOOR_EM`, which is not the same number as
 *                      the CSS clamp that stops it shrinking
 */
export function foldPlan(
  widths: number[],
  separator: number,
  more: number,
  available: number,
  currentFloor = 0,
): number[] {
  const last = widths.length - 1;
  /** Narrowest the row can be with `hidden` folded and the last crumb squeezed. */
  const rowWidth = (hidden: Set<number>): number => {
    const kept = widths.filter((_, i) => !hidden.has(i));
    if (kept.length === 0) return 0;
    const charged = widths.map((w, i) => (i === last ? Math.min(w, currentFloor || w) : w));
    let total =
      charged.filter((_, i) => !hidden.has(i)).reduce((a, b) => a + b, 0) +
      separator * (kept.length - 1);
    // A fold adds the `…` and one more separator, so folding one narrow
    // ancestor can make the row WIDER. The loop below still terminates
    // correctly: it stops at the first plan that fits, and if none does it
    // returns the maximal fold, which is the best available answer.
    if (hidden.size > 0) total += more + separator;
    return total;
  };

  const hidden = new Set<number>();
  if (rowWidth(hidden) <= available + SLACK) return [];

  for (let i = 1; i < widths.length - 1; i++) {
    hidden.add(i);
    if (rowWidth(hidden) <= available + SLACK) break;
  }
  return [...hidden].sort((a, b) => a - b);
}

// The trail is a flex `row`, which always follows the container's own
// inline axis, so under vertical writing the row runs down the page and
// every "width" this file measures is actually a height. `isVertical`
// (shared with theme.ts's font-selector panel, which hits the same axis) is
// the one check that keeps that fact from being re-derived per measurement.

/** `el`'s border-box extent along its own inline axis. */
function inlineExtent(el: HTMLElement): number {
  return isVertical(el) ? el.offsetHeight : el.offsetWidth;
}

/** `label`'s scrollable extent along its own inline axis. */
function inlineScrollExtent(el: HTMLElement): number {
  return isVertical(el) ? el.scrollHeight : el.scrollWidth;
}

/** `el`'s visible (padding-box) extent along its own inline axis. */
function inlineClientExtent(el: HTMLElement): number {
  return isVertical(el) ? el.clientHeight : el.clientWidth;
}

/**
 * `el`'s margin along its own inline axis (both sides summed). Under
 * vertical writing the inline axis is physically top/bottom for both
 * `vertical-rl` and `vertical-lr` (only the BLOCK direction differs between
 * them), so the two physical margins to sum flip together with `isVertical`.
 */
function inlineMarginExtent(el: HTMLElement): number {
  const style = getComputedStyle(el);
  return isVertical(el)
    ? (parseFloat(style.marginTop) || 0) + (parseFloat(style.marginBottom) || 0)
    : (parseFloat(style.marginLeft) || 0) + (parseFloat(style.marginRight) || 0);
}

/** The room an element's own client box offers along its inline axis. */
export function inlineAvailableExtent(el: HTMLElement): number {
  return inlineClientExtent(el);
}

/**
 * A crumb's inline extent with nothing clipped.
 *
 * Ancestors never shrink under fold management, so their box extent IS their
 * natural extent. The trail's last crumb does shrink, and its
 * `.breadcrumb-label` clips with an ellipsis — so for that one the label's
 * scroll extent is the honest number. Taking the larger of the two is right
 * for both without branching on which crumb it is.
 */
export function naturalWidth(el: HTMLElement): number {
  const label = el.querySelector<HTMLElement>(".breadcrumb-label");
  return Math.max(inlineExtent(el), label ? inlineScrollExtent(label) : 0);
}

/**
 * The room an element actually takes from the row, margins included.
 *
 * `offsetWidth`/`offsetHeight` stop at the border, so a `/` measured that way
 * reads 4px when it occupies 11 to 14 — `.breadcrumb-separator` carries
 * `margin: 0 0.35em`, dropping to `0.25em` below 32rem, which at the island's
 * 14px is 9.8px or 7px of margin per separator. The fold plan charged none of
 * it, so it concluded a row fit and the row then overflowed by however many
 * separators were on it. Measure, do not restate: the margin changes with the
 * viewport.
 */
export function outerWidth(el: HTMLElement): number {
  return inlineExtent(el) + inlineMarginExtent(el);
}

/** The elements one folding trail is made of. */
export interface FoldParts {
  /** The trail container — where the sample separator is found. */
  trail: HTMLElement;
  /** Every crumb in trail order. First and last never fold. */
  crumbs: HTMLElement[];
  /** The `…` button, hidden until something folds. */
  moreBtn: HTMLButtonElement;
  /** The separator that follows the `…`, hidden with it. */
  moreSep: HTMLElement;
  /** The panel the folded levels are listed in. */
  menu: HTMLElement;
  /**
   * The width the row offers the trail. Read BEFORE any crumb is unhidden:
   * on the masthead, unhiding first changes what the flex line allocates to
   * `.nav-left` (a folded trail can pull `.nav-right` back onto row 1), so a
   * width read after unhiding answers a layout that no longer exists once
   * the fold is re-applied — which oscillates. The island's bar is fixed
   * width, so for it the order is merely harmless.
   */
  available: () => number;
}

/**
 * Measure a trail and fold it to fit — hide the planned ancestors, populate
 * the levels panel, and name the folded levels on the `…` button (both
 * `data-tooltip`, which site.css renders, and `aria-label`, so a screen
 * reader hears no less than a sighted reader sees).
 *
 * Returns the joined folded-level names, `""` when nothing folded — the
 * caller keeps it so the tooltip can be restored after its panel closes.
 */
export function applyFold(parts: FoldParts): string {
  const { trail, crumbs, moreBtn, moreSep, menu } = parts;
  if (crumbs.length < 3) return "";

  // Read the row's offer first — see the doc on `FoldParts.available`.
  const available = parts.available();

  // Measure from the unfolded state: what the row WOULD need is the question,
  // and a row that is already folded cannot answer it.
  crumbs.forEach((crumb) => {
    crumb.hidden = false;
    const sep = crumb.nextElementSibling;
    if (sep instanceof HTMLElement && sep.classList.contains("breadcrumb-separator")) {
      sep.hidden = false;
    }
  });
  moreBtn.hidden = true;
  moreSep.hidden = true;

  const widths = crumbs.map(naturalWidth);
  const sampleSep = trail.querySelector<HTMLElement>(".breadcrumb-separator");
  const separator = sampleSep ? outerWidth(sampleSep) : 0;
  // The `…` is hidden, so it has no box to measure. Unhide it for one read.
  moreBtn.hidden = false;
  const moreWidth = outerWidth(moreBtn);
  moreBtn.hidden = true;

  // Never charge the last crumb less than CSS will actually let it shrink
  // to: assuming give that does not exist is how a row "fits" in the plan and
  // overflows on screen. So the clamp wins if it is ever raised above the
  // fold threshold, and the two numbers cannot be put out of order by hand.
  const current = crumbs[crumbs.length - 1];
  const style = getComputedStyle(current);
  const floor = Math.max(
    FOLD_FLOOR_EM * (parseFloat(style.fontSize) || 16),
    parseFloat(style.minWidth) || 0,
  );

  const folded = foldPlan(widths, separator, moreWidth, available, floor);

  folded.forEach((i) => {
    crumbs[i].hidden = true;
    const sep = crumbs[i].nextElementSibling;
    if (sep instanceof HTMLElement) sep.hidden = true;
  });

  moreBtn.hidden = folded.length === 0;
  moreSep.hidden = folded.length === 0;

  menu.textContent = "";
  folded.forEach((i) => {
    const crumb = crumbs[i];
    const row = document.createElement("a");
    row.href = crumb.getAttribute("href") ?? "#";
    row.textContent = crumb.textContent ?? "";
    menu.appendChild(row);
  });

  // Hover NAMES the folded levels; click OPENS them. Never hover-to-open: a
  // touch device has no hover, and narrow is exactly the width where folding
  // happens, so the affordance would not exist where it is needed. Via
  // `data-tooltip` — site.css renders that — and never the native `title`.
  //
  // The same names go into `aria-label`. They were only in the tooltip
  // before, which told a sighted reader which levels were folded and left a
  // screen-reader user with a generic "show hidden levels" — the accessible
  // name saying strictly less than the visual one, which is backwards.
  //
  // The emitter's generic label is stashed on the element the first time
  // through, so widening the window can put it back: the `…` must not keep
  // announcing levels that are back on screen.
  if (moreBtn.dataset.foldBaseLabel === undefined) {
    moreBtn.dataset.foldBaseLabel = moreBtn.getAttribute("aria-label") ?? "";
  }
  const base = moreBtn.dataset.foldBaseLabel;
  if (folded.length > 0) {
    const names = folded.map((i) => crumbs[i].textContent?.trim() ?? "").join(" / ");
    moreBtn.setAttribute("data-tooltip", names);
    moreBtn.setAttribute("aria-label", base ? `${base}: ${names}` : names);
    return names;
  }
  moreBtn.removeAttribute("data-tooltip");
  if (base) moreBtn.setAttribute("aria-label", base);
  return "";
}

/** Distance from the bar's bottom edge to the panel's top. Matches site.css. */
const PANEL_GAP = 6;

/**
 * Place an open panel: directly under the bar that owns it, left-aligned to
 * whatever opened it, then pulled back so it stays inside that bar's width.
 *
 * Both coordinates are resolved against the panel's CONTAINING BLOCK, which is
 * not the bar: panels are siblings of the trail (on the island because the bar
 * clips its overflow; on the masthead because `.nav-left` does), so their
 * containing block is the wrapper around both. That gap is why BOTH axes are
 * measured here rather than left to CSS —
 *
 * - horizontally, offsetting from the bar put the panel nowhere near the
 *   control that opened it on every viewport where the two boxes differ;
 * - vertically, site.css's `top: calc(100% + 6px)` is 100% of the CONTAINING
 *   BLOCK. On the island that is the bar, near enough. On the masthead it is
 *   `.nav-content` — the whole header, both rows — so on a phone the folded
 *   levels opened a row and a half below the `…` that opened them, floating
 *   over the article's cover image.
 *
 * The CSS `top` stays as the pre-placement resting value; from the first open
 * onward this is authoritative.
 *
 * @param container the panel's containing block (its positioned ancestor)
 * @param bar       the visible bar the panel hangs off and is clamped inside
 */
export function placePanel(
  menu: HTMLElement,
  anchor: HTMLElement,
  container: HTMLElement,
  bar: HTMLElement,
): void {
  const containerBox = container.getBoundingClientRect();
  const barBox = bar.getBoundingClientRect();
  const anchorBox = anchor.getBoundingClientRect();

  menu.style.removeProperty("right");
  menu.style.left = "0px";
  const width = menu.getBoundingClientRect().width;

  // Clamp into the bar. `Math.max(min, ...)` on the upper bound keeps a panel
  // wider than the bar pinned to its left edge rather than pushed off it.
  const min = barBox.left;
  const max = Math.max(min, barBox.right - width);
  const x = Math.min(Math.max(anchorBox.left, min), max);
  menu.style.left = `${x - containerBox.left}px`;
  menu.style.top = `${barBox.bottom - containerBox.top + PANEL_GAP}px`;
}
