/**
 * breadcrumb-hint.ts — show a breadcrumb's full title only when it is cut off.
 *
 * A breadcrumb segment shrinks with the window: at some width its label gets
 * an ellipsis, and on a phone it can collapse to a bare "…". That is when a
 * hover hint earns its place. When the whole label is on screen, a hint just
 * repeats what the reader can already read, which is noise — reported on
 * https://example.mosspub.com/about/publications/ where every segment
 * showed one.
 *
 * Whether a label is cut is a question about rendered width, and CSS cannot
 * ask it: `::after` has no way to know its host overflowed. `scrollWidth >
 * clientWidth` on the label span is the question, and it needs a script. So
 * nav.rs emits the full title in `data-hint-label`, an attribute site.css
 * styles nothing from, and this module promotes it to `data-tooltip` (which
 * site.css does render) exactly while the label is actually truncated,
 * removing it again when the window widens.
 *
 * With JS off, no segment ever gets `data-tooltip` and no hint appears. That
 * is the right fallback: always-on was the bug, and the label the reader
 * cannot fully see is still reachable by clicking through to the section.
 */

export {};

/**
 * Any breadcrumb segment carrying a full title, link or not. The island's
 * current-page crumb is a `<span>` — it is the page you are already on, so it
 * is not a link — and it is also the ONE crumb the island allows to truncate
 * (ADR-049 §4), which makes it the crumb that most needs this.
 */
const SEGMENT = ".breadcrumb-segment[data-hint-label]";
const LABEL = ".breadcrumb-label";

/**
 * Sub-pixel slack. Fractional layout widths make `scrollWidth` (rounded up)
 * exceed `clientWidth` (rounded down) by up to a pixel on labels that are not
 * clipped at all, which would put the hint back on every segment.
 */
const SLACK = 1;

let observer: ResizeObserver | null = null;

/** Promote or drop one segment's hint based on its label's current width. */
function sync(segment: HTMLElement): void {
  const label = segment.querySelector<HTMLElement>(LABEL);
  if (!label) return;

  const truncated = label.scrollWidth > label.clientWidth + SLACK;
  if (truncated) {
    const full = segment.getAttribute("data-hint-label") ?? "";
    // Only write when it would change: a no-op attribute write still
    // invalidates style for the element.
    if (segment.getAttribute("data-tooltip") !== full) {
      segment.setAttribute("data-tooltip", full);
    }
  } else if (segment.hasAttribute("data-tooltip")) {
    segment.removeAttribute("data-tooltip");
  }
}

/** Re-measure every breadcrumb segment on the page. */
function syncAll(): void {
  document.querySelectorAll<HTMLElement>(SEGMENT).forEach(sync);
}

/**
 * Wire up truncation-aware breadcrumb hints.
 *
 * Idempotent: safe to call again after an in-place morph replaces the nav,
 * which is why it re-observes from scratch rather than tracking what it has
 * already seen — the old nodes are gone and their observations with them.
 */
export function initBreadcrumbHints(): void {
  // Drop the previous observations FIRST, and unconditionally. A morph to a
  // page with no breadcrumbs leaves the old labels detached, and an observer
  // still holding them keeps that dead subtree alive for as long as the tab
  // lives. Disconnecting after the early return below would skip exactly the
  // case that needs it.
  observer?.disconnect();
  observer = null;

  const labels = document.querySelectorAll<HTMLElement>(`${SEGMENT} ${LABEL}`);
  if (labels.length === 0) return;

  // A label is a flex item that shrinks as the window narrows, so observing
  // each label is a direct read of the thing the answer depends on — no
  // debounce needed, and it also catches width changes no `resize` event
  // reports (font loading, a sibling segment appearing after a morph).
  if (typeof ResizeObserver !== "undefined") {
    observer = new ResizeObserver(() => syncAll());
    labels.forEach((label) => observer!.observe(label));
  }

  syncAll();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initBreadcrumbHints);
} else {
  initBreadcrumbHints();
}
// The preview morphs the page in place rather than reloading it, so the nav
// can be replaced by brand-new nodes the observer above has never seen.
document.addEventListener("moss-morph-patched", initBreadcrumbHints);

// Belt-and-braces for engines without ResizeObserver (and for the case where
// the trail is re-laid-out without any single label changing size).
window.addEventListener("resize", syncAll, { passive: true });
