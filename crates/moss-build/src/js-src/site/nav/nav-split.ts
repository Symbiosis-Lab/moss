/**
 * nav-split.ts: when a breadcrumb trail runs out of room, the trail keeps the
 * toggles beside it on row 1 and the links take row 2 alone.
 *
 * The toggles (search, language, theme) are tools for the whole page, so they
 * keep the corner the one-row masthead gives them; only the navigation moves.
 * The markup puts them first, and focus follows the markup on every page:
 * skip link, toggles, name or trail, menu button, links. A plain site name
 * needs nothing from here above the hamburger range: site.css floats the
 * toggles and the name wraps beside them from the first paint.
 *
 * A trail is laid in a flex row (trail | links | toggles), and flex wrapping
 * cannot lift the toggles back up once the links wrap. So this module
 * measures. When the one-row layout no longer fits, it sets `data-nav-split`
 * on `.nav-content`, and site.css places the trail, toggles and links on a
 * grid. On a phone (site.css sets `--moss-nav-phone`), before splitting it
 * tries a tighter single row: `data-nav-compact` draws the gaps in, and the
 * masthead splits only if that still overflows. Wider, a near-fit splits at
 * once.
 *
 * In the hamburger range (≤20rem) the links are an overlay and the name stays
 * beside the menu button and toggles while it reasonably can. A plain name
 * wraps there (site.css); past two lines, `data-nav-stack` gives it its
 * one-line width back and the controls go below. A breadcrumb trail is split
 * beside them while its most-folded form fits, and otherwise keeps the
 * shipped wrap, with the controls below.
 *
 * Every measured input is the same in every mode, so a decision cannot
 * oscillate: the link and toggle widths are fixed boxes, the column gap is
 * re-read after each mode is applied, a breadcrumb trail is measured on a
 * copy (its live crumbs fold to fit whatever room the split leaves), and the
 * name's lines are counted with the stack cleared. Sizes are read on the
 * masthead's inline axis; on a vertical page its physical width is its block
 * size, which the split itself changes. A theme that keeps `.nav-right` a box
 * gets the plain wrap from the split's grid; one that flattens it (`display:
 * contents`) has laid a trail's masthead out itself and is left alone. A theme
 * that sets `flex-wrap: nowrap` on `.nav-content` gets `data-nav-scroll` where
 * the links would leave its one row, a plain name's float layout included: the
 * row stays, its links scrolling inline. With JS off nothing here runs: a
 * trail is alone on row 1, the links and toggles on row 2. On a phone site.css
 * holds a trail's two rows at the toggles' height, so the split this makes
 * after the first paint moves only the toggles.
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
  /** `.nav-content`'s column gap, before the links and again before the toggles. */
  gap: number;
}

/**
 * Would `name | links | toggles` fit on one row? Pure so the decision is
 * testable without layout. The widths are fractional, as flex lays them out,
 * and flex wraps at any overflow: a slack of a whole pixel accepted rows that
 * then wrapped. 0.01px absorbs only float error in the sum.
 */
export function oneRowFits(w: RowWidths): boolean {
  return w.name + w.links + w.icons + 2 * w.gap <= w.container + 0.01;
}

/** `el`'s border box along the masthead's inline axis, unrounded. */
function inlineSize(el: HTMLElement, vertical: boolean): number {
  const r = el.getBoundingClientRect();
  return vertical ? r.height : r.width;
}

/** `el`'s content box along the masthead's inline axis, unrounded. */
function contentSize(el: HTMLElement, vertical: boolean): number {
  const s = getComputedStyle(el);
  const edges = vertical
    ? [s.paddingTop, s.paddingBottom, s.borderTopWidth, s.borderBottomWidth]
    : [s.paddingLeft, s.paddingRight, s.borderLeftWidth, s.borderRightWidth];
  return edges.reduce((room, edge) => room - (parseFloat(edge) || 0), inlineSize(el, vertical));
}

/** Everything the previous `initNavSplit()` bound, undone on the next call. */
const stack = teardownStack();
const { on } = stack;

function setMode(el: HTMLElement, mode: "compact" | "split" | "stack" | "scroll" | null): void {
  el.removeAttribute("data-nav-split");
  el.removeAttribute("data-nav-compact");
  el.removeAttribute("data-nav-stack");
  el.removeAttribute("data-nav-scroll");
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

/** Whether the floated links start below the masthead's first line, which is where a float that fits sits. */
function linksDropped(navContent: HTMLElement, links: HTMLElement): boolean {
  const s = getComputedStyle(navContent);
  const top = navContent.getBoundingClientRect().top + (parseFloat(s.borderTopWidth) || 0) + (parseFloat(s.paddingTop) || 0);
  return links.getBoundingClientRect().top > top + 0.5;
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
  if (!navLeft || !siteName || !links || !icons || links.children.length === 0) {
    setMode(navContent, null);
    return;
  }
  const trail = navLeft.querySelector(".breadcrumb-segment") !== null;

  function layout(): void {
    const vertical = isVertical(navContent!);
    const size = (el: HTMLElement) => inlineSize(el, vertical);
    setMode(navContent!, null);
    // A theme that holds the masthead to one row (`flex-wrap: nowrap`, often
    // with a fixed height) has no room for a second: links that do not fit
    // scroll inside that row instead.
    const oneRow = getComputedStyle(navContent!).flexWrap === "nowrap";
    // ≤20rem the links are the hamburger's overlay panel, not a row: the
    // menu button stands in for them.
    if (hamburger && getComputedStyle(hamburger).display !== "none") {
      if (trail) {
        const beside = oneRowFits({
          container: contentSize(navContent!, vertical),
          name: trailSize(navLeft!, size, "folded"),
          links: size(hamburger),
          icons: size(icons!),
          gap: columnGap(navContent!, 0),
        });
        if (beside) setMode(navContent!, "split");
      } else if (nameLines(siteName!, vertical) > 2) {
        setMode(navContent!, "stack");
      }
      return;
    }
    // Above it a plain name floats beside the toggles in CSS alone (site.css),
    // and links that do not fit beside its last line drop below it.
    if (!trail) {
      if (oneRow && !vertical && linksDropped(navContent!, links!)) setMode(navContent!, "scroll");
      return;
    }
    // A theme that flattens `.nav-right` itself has placed the links and
    // toggles beside the trail; the split would only undo that. Read with no
    // mode set: the split flattens it too.
    if (navRight && getComputedStyle(navRight).display === "contents") return;
    const fitsOneRow = (): boolean => {
      const linkGap = columnGap(links!, 0);
      const linkWidths = [...links!.children].map((child) => size(child as HTMLElement));
      return oneRowFits({
        container: contentSize(navContent!, vertical),
        name: trailSize(navLeft!, size, "natural"),
        links: linkWidths.reduce((sum, w) => sum + w, 0) + linkGap * Math.max(0, linkWidths.length - 1),
        icons: size(icons!),
        gap: columnGap(navContent!, 0),
      });
    };
    // Full spacing, then (on a phone) tight gaps, then two rows.
    if (fitsOneRow()) return;
    if (getComputedStyle(navContent!).getPropertyValue("--moss-nav-phone").trim() === "1") {
      setMode(navContent!, "compact");
      if (fitsOneRow()) return;
    }
    setMode(navContent!, oneRow ? "scroll" : "split");
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
