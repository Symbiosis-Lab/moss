/**
 * nav-split.ts — when the masthead runs out of room, the site name (or the
 * breadcrumb trail) keeps the toggles beside it on row 1 and the links take
 * row 2 alone.
 *
 * The toggles (search, language, theme) are tools for the whole page, so they
 * keep the corner the one-row masthead gives them; only the navigation moves.
 * The shipped CSS cannot do that: it wraps `.nav-right` (links + toggles, one
 * unit) to row 2 when it can't fit beside `.nav-left`.
 *
 * The wanted two-row order — name + toggles / links — is a reordering of the
 * one-row sequence (name | links | toggles), and flex wrapping cannot reorder:
 * it places items in sequence and never floats a later sibling back up to an
 * earlier line. So this module measures. When the one-row layout no longer
 * fits, it sets `data-nav-split` on `.nav-content`, and site.css lifts the two
 * groups out of `.nav-right` (`display: contents`) and orders them. Focus
 * order stays the markup's (name, links, toggles), the same at every width.
 *
 * On a phone (up to 48rem), before splitting it tries a tighter single row:
 * `data-nav-compact` draws the outer and toggle gaps in, and the masthead
 * splits only if that still overflows. Wider, a near-fit splits at once.
 *
 * In the hamburger range (≤20rem) the links are an overlay and the name stays
 * beside the menu button and toggles while it reasonably can. A plain name
 * wraps there (site.css); past two lines, `data-nav-stack` gives it its
 * one-line width back and the controls go below. A breadcrumb trail is split
 * beside them while its most-folded form fits, and otherwise keeps the
 * shipped wrap, with the controls below.
 *
 * Every measured input is the same in every mode, so a decision cannot
 * oscillate: the link and toggle widths are fixed boxes, the column gaps are
 * re-read after each mode is applied, a breadcrumb trail is measured on a
 * copy (its live crumbs fold to fit whatever room the split leaves),
 * and the name's lines are counted with the stack cleared. Sizes are read on
 * the masthead's inline axis; on a vertical page its physical width is its
 * block size, which the split itself changes. A theme that keeps `.nav-right`
 * a box is left to the CSS. With JS off nothing here runs and the shipped
 * wrap is the fallback: the name alone on row 1, the links and toggles on
 * row 2.
 */

import { isVertical } from "../writing-mode";
import { teardownStack } from "./teardown";

/** Widths (px) of everything a one-row masthead has to seat. */
export interface RowWidths {
  /** The row on offer: `.nav-content`'s content box. */
  container: number;
  /** The site name, or the whole breadcrumb trail, at its natural width. */
  name: number;
  /** The link set laid on one line: link widths + link gaps. */
  links: number;
  /** The toggle cluster (its buttons are fixed boxes; no flex growth). */
  icons: number;
  /** Gap between `.nav-left` and the link set. */
  outerGap: number;
  /** Gap between the link set and the toggle cluster. */
  innerGap: number;
}

/**
 * Would `name | links | toggles` fit on one row? Pure so the decision is
 * testable without layout; +1px absorbs subpixel rounding in offsetWidth.
 */
export function oneRowFits(w: RowWidths): boolean {
  return w.name + w.outerGap + w.links + w.innerGap + w.icons <= w.container + 1;
}

const PHONE_QUERY = "(max-width: 48rem)";

/** Everything the previous `initNavSplit()` bound, undone on the next call. */
const stack = teardownStack();
const { on } = stack;

function setMode(el: HTMLElement, mode: "compact" | "split" | "stack" | null): void {
  el.removeAttribute("data-nav-split");
  el.removeAttribute("data-nav-compact");
  el.removeAttribute("data-nav-stack");
  if (mode) el.setAttribute(`data-nav-${mode}`, "");
}

/**
 * A breadcrumb trail's extent, read from a max-content copy beside it, out of
 * flow and invisible, and removed at once. `"natural"`: every crumb, nothing
 * folded or ellipsised. `"folded"`: the least the fold can leave, which is the
 * site name, the `…` when there are middles to fold, and the last crumb at its
 * CSS minimum.
 */
export function trailSize(
  navLeft: HTMLElement,
  size: (el: HTMLElement) => number,
  form: "natural" | "folded",
): number {
  const copy = navLeft.cloneNode(true) as HTMLElement;
  const crumbs = [...copy.querySelectorAll<HTMLElement>("[data-trail-crumb]")];
  const middles = form === "folded" ? crumbs.slice(1, -1) : [];
  if (middles.length === 0) {
    copy.querySelectorAll(".moss-breadcrumb-more, [data-trail-more-separator]").forEach((el) => el.remove());
  }
  middles.forEach((crumb) => {
    crumb.nextElementSibling?.remove();
    crumb.remove();
  });
  copy.querySelectorAll("[hidden]").forEach((el) => el.removeAttribute("hidden"));
  const liveCrumbs = navLeft.querySelectorAll<HTMLElement>("[data-trail-crumb]");
  const last = crumbs[crumbs.length - 1];
  if (form === "folded" && last && liveCrumbs.length > 0) {
    const floor = getComputedStyle(liveCrumbs[liveCrumbs.length - 1]).minInlineSize;
    last.style.cssText = `flex:none;overflow:hidden;inline-size:${parseFloat(floor) || 0}px`;
  }
  copy.setAttribute("aria-hidden", "true");
  copy.style.cssText = "position:absolute;visibility:hidden;inline-size:max-content;overflow:visible;flex:none";
  navLeft.after(copy);
  const extent = size(copy);
  copy.remove();
  return extent;
}

/** How many lines the name's text runs to: rects whose block extents overlap share a line. */
export function lineCount(extents: Array<[number, number]>): number {
  let lines = 0;
  let end = -Infinity;
  for (const [start, stop] of [...extents].sort((a, b) => a[0] - b[0])) {
    if (start >= end - 1) {
      lines += 1;
      end = stop;
    } else {
      end = Math.max(end, stop);
    }
  }
  return lines;
}

function nameLines(name: HTMLElement, vertical: boolean): number {
  const range = document.createRange();
  range.selectNodeContents(name);
  const extents = [...range.getClientRects()]
    .filter((r) => (vertical ? r.width : r.height) > 0)
    .map((r): [number, number] => (vertical ? [r.left, r.right] : [r.top, r.bottom]));
  return lineCount(extents);
}

function columnGap(el: Element, fallback: number): number {
  const parsed = parseFloat(getComputedStyle(el).columnGap);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export function initNavSplit(): void {
  stack.release();

  const navContent = document.querySelector<HTMLElement>(".nav-content");
  if (!navContent) return;

  const navLeft = navContent.querySelector<HTMLElement>(".nav-left");
  const siteName = navLeft?.querySelector<HTMLElement>(".site-name");
  const navRight = navContent.querySelector<HTMLElement>(".nav-right");
  const links = navContent.querySelector<HTMLElement>(".nav-links");
  const icons = navContent.querySelector<HTMLElement>(".nav-icons");
  const hamburger = navContent.querySelector<HTMLElement>(".mobile-menu-button");

  // A masthead with no link set has nothing to move.
  if (!navLeft || !siteName || !navRight || !links || !icons || links.children.length === 0) {
    setMode(navContent, null);
    return;
  }
  const trail = navLeft.querySelector(".breadcrumb-segment") !== null;

  function layout(): void {
    const vertical = isVertical(navContent!);
    const size = (el: HTMLElement) => (vertical ? el.offsetHeight : el.offsetWidth);
    // A theme that keeps `.nav-right` a box has laid out its own wrap, and
    // the split's ordering would put the toggles ahead of the links inside it.
    const split = (): void => {
      setMode(navContent!, "split");
      if (getComputedStyle(navRight!).display !== "contents") setMode(navContent!, null);
    };
    // ≤20rem the links are the hamburger's overlay panel, not a row: the
    // menu button stands in for them.
    if (hamburger && getComputedStyle(hamburger).display !== "none") {
      setMode(navContent!, null);
      if (trail) {
        const beside = oneRowFits({
          container: vertical ? navContent!.clientHeight : navContent!.clientWidth,
          name: trailSize(navLeft!, size, "folded"),
          links: size(hamburger),
          icons: size(icons!),
          outerGap: columnGap(navContent!, 0),
          innerGap: columnGap(navRight!, 0),
        });
        if (beside) split();
      } else if (nameLines(siteName!, vertical) > 2) {
        setMode(navContent!, "stack");
      }
      return;
    }
    const fitsOneRow = (): boolean => {
      const outerGap = columnGap(navContent!, 0);
      const linkGap = columnGap(links!, 0);
      // `.nav-right` has no box in split mode; fall back if its gap won't resolve.
      const innerGap = columnGap(navRight!, outerGap);
      const linkWidths = [...links!.children].map((child) => size(child as HTMLElement));
      return oneRowFits({
        container: vertical ? navContent!.clientHeight : navContent!.clientWidth,
        name: trail ? trailSize(navLeft!, size, "natural") : size(siteName!),
        links: linkWidths.reduce((sum, w) => sum + w, 0) + linkGap * Math.max(0, linkWidths.length - 1),
        icons: size(icons!),
        outerGap,
        innerGap,
      });
    };
    // Full spacing, then (on a phone) tight gaps, then two rows.
    setMode(navContent!, null);
    if (fitsOneRow()) return;
    if (window.matchMedia?.(PHONE_QUERY).matches) {
      setMode(navContent!, "compact");
      if (fitsOneRow()) return;
    }
    split();
  }

  // The masthead resizes with the window and with the font arriving; the
  // observer catches container changes, `fonts.ready` catches the swap from
  // fallback metrics that the container never feels.
  if (typeof ResizeObserver !== "undefined") {
    const ro = new ResizeObserver(() => layout());
    ro.observe(navContent);
    stack.add(() => ro.disconnect());
  }
  on(window, "resize", () => layout(), { passive: true });
  document.fonts?.ready.then(() => layout()).catch(() => {});

  layout();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initNavSplit);
} else {
  initNavSplit();
}

// A preview rebuild morphs the page in place rather than reloading it, so the
// nav can be replaced by nodes none of the listeners above have seen.
document.addEventListener("moss-morph-patched", initNavSplit);
