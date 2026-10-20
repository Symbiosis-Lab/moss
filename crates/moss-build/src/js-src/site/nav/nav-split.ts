/**
 * nav-split.ts — on a plain-site-name masthead, the nav wraps to a clean two
 * rows: the site name alone on row 1, the links and toggles on row 2.
 *
 * The shipped CSS wraps `.nav-right` (links + toggles, one unit) to row 2 when
 * it can't fit beside `.nav-left`. On breadcrumb pages that is right: the
 * trail fills row 1, and row 2 anchors links at the start edge and toggles at
 * the end edge. But when `.nav-left` is just a short site name, that same
 * wrap leaves row 1 mostly empty and drops the toggle cluster to wherever the
 * last link row ends — chrome appended to a link list it has nothing to do
 * with.
 *
 * The wanted two-row order — site name / links + toggles — is a reordering of
 * the one-row sequence (name | links | toggles), and flex wrapping cannot
 * reorder: it places items in sequence and never floats a later sibling back
 * up to an earlier line. So this module measures. When the one-row layout no
 * longer fits, it sets `data-nav-split` on `.nav-content`, and site.css lifts
 * the two groups out of `.nav-right` (`display: contents`) and orders them:
 * the name takes row 1 alone, then the links and the toggles at its end edge
 * share row 2.
 *
 * Before splitting it tries a tighter single row: `data-nav-compact` draws the
 * outer and toggle gaps in, and the masthead splits only if that still overflows.
 *
 * Every measured input — site-name width, per-link widths, toggle-cluster
 * width — is the same in every mode, and the column gaps are re-read after each
 * mode is applied, so the decision cannot oscillate. Sizes are read on the
 * masthead's inline axis; on a vertical page its physical width is its block
 * size, which the split itself changes. Breadcrumb pages and the hamburger range
 * (≤20rem, where the links are an overlay) are left to the shipped CSS
 * untouched. With JS off nothing here runs and the shipped wrap is the
 * fallback, which also leaves the name alone on row 1; a link set that wraps
 * to several lines there holds the toggles at its bottom edge, which is what
 * the split avoids.
 */

import { isVertical } from "../writing-mode";

/** Widths (px) of everything a one-row masthead has to seat. */
export interface RowWidths {
  /** The row on offer: `.nav-content`'s content box. */
  container: number;
  /** The site name at its natural width. */
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

/** Everything the previous `initNavSplit()` bound, undone on the next call. */
let teardown: Array<() => void> = [];

function on(
  target: Window | Document | Element,
  type: string,
  handler: (event: Event) => void,
  options?: AddEventListenerOptions,
): void {
  target.addEventListener(type, handler, options);
  teardown.push(() => target.removeEventListener(type, handler, options));
}

function setMode(el: HTMLElement, mode: "compact" | "split" | null): void {
  el.removeAttribute("data-nav-split");
  el.removeAttribute("data-nav-compact");
  if (mode) el.setAttribute(`data-nav-${mode}`, "");
}

function columnGap(el: Element, fallback: number): number {
  const parsed = parseFloat(getComputedStyle(el).columnGap);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export function initNavSplit(): void {
  teardown.forEach((undo) => undo());
  teardown = [];

  const navContent = document.querySelector<HTMLElement>(".nav-content");
  if (!navContent) return;

  const navLeft = navContent.querySelector<HTMLElement>(".nav-left");
  const siteName = navLeft?.querySelector<HTMLElement>(".site-name");
  const navRight = navContent.querySelector<HTMLElement>(".nav-right");
  const links = navContent.querySelector<HTMLElement>(".nav-links");
  const icons = navContent.querySelector<HTMLElement>(".nav-icons");
  const hamburger = navContent.querySelector<HTMLElement>(".mobile-menu-button");

  // Breadcrumb pages keep the shipped layout (the trail owns row 1 — see the
  // module comment), and a masthead with no link set has nothing to move.
  if (
    !navLeft ||
    !siteName ||
    !navRight ||
    !links ||
    !icons ||
    links.children.length === 0 ||
    navLeft.querySelector(".breadcrumb-segment")
  ) {
    setMode(navContent, null);
    return;
  }

  function layout(): void {
    // ≤20rem the links are the hamburger's overlay panel, not a row.
    if (hamburger && getComputedStyle(hamburger).display !== "none") {
      setMode(navContent!, null);
      return;
    }
    const vertical = isVertical(navContent!);
    const size = (el: HTMLElement) => (vertical ? el.offsetHeight : el.offsetWidth);
    const fitsOneRow = (): boolean => {
      const outerGap = columnGap(navContent!, 0);
      const linkGap = columnGap(links!, 0);
      // `.nav-right` has no box in split mode; fall back if its gap won't resolve.
      const innerGap = columnGap(navRight!, outerGap);
      const linkWidths = [...links!.children].map((child) => size(child as HTMLElement));
      return oneRowFits({
        container: vertical ? navContent!.clientHeight : navContent!.clientWidth,
        name: size(siteName!),
        links: linkWidths.reduce((sum, w) => sum + w, 0) + linkGap * Math.max(0, linkWidths.length - 1),
        icons: size(icons!),
        outerGap,
        innerGap,
      });
    };
    // Full spacing, then tight gaps, then two rows.
    setMode(navContent!, null);
    if (fitsOneRow()) return;
    setMode(navContent!, "compact");
    if (fitsOneRow()) return;
    setMode(navContent!, "split");
  }

  // The masthead resizes with the window and with the font arriving; the
  // observer catches container changes, `fonts.ready` catches the swap from
  // fallback metrics that the container never feels.
  if (typeof ResizeObserver !== "undefined") {
    const ro = new ResizeObserver(() => layout());
    ro.observe(navContent);
    teardown.push(() => ro.disconnect());
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
