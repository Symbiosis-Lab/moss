/**
 * contents-ruler.ts — a contents ruler in the left margin of a long page.
 *
 * One short dash per section, pinned to the window and centred on its height.
 * Hovering (or focusing) the dashes opens their titles to the right; clicking
 * a dash jumps to that section. It is the wide-screen counterpart of the nav
 * island's sections panel and is owned by the island: `initNavIsland` mounts
 * it with the same headings, tears it down on the same morph, and tells it
 * which section is current. Nothing here scans the page for the current
 * section — there is exactly one scrollspy (`currentSection` in
 * nav-island.ts) and this module only listens to it.
 *
 * Where the geometry matters, it is measured, not breakpointed:
 *
 * - A label may reach no further than one gutter short of the text column, so
 *   its maximum width is `textLeft - GUTTER - labelLeft`, read from the layout.
 *   Titles longer than that are cut in the middle (see middle-truncate.ts).
 * - Under `MIN_LABEL` of room a label names nothing, so the ruler stays away
 *   and the island is all there is.
 * - A site can switch it off with CSS (`display: none`, `visibility: hidden`);
 *   whether it is actually rendered is read on every pass, so the island's
 *   sections button comes back however it was hidden.
 * - It also stays away when the dashes would not fit between the island's bar
 *   and the bottom of the window — the list must not scroll, because scrolling
 *   would clip the labels that hang out of it.
 *
 * Every dash row is the same height at rest and open. Re-spacing on hover was
 * tried and rejected: the row under the pointer moves, and the click lands on
 * a different section.
 *
 * Which of this needs an engine — hit-testing, label clearance, clipping — is
 * asserted in tests/render-gates/site/contents-ruler.spec.ts.
 */

import { canvasMeasure, truncateMiddle } from "./middle-truncate";
import { markRows, SECTION_SYNC_EVENT, type SectionSyncDetail } from "./section-event";

/** Clear space between the widest label and the text column — the same gap
 *  the text keeps from a sidenote. */
const GUTTER = 24;

/** Narrowest label worth showing: about seven CJK characters. Anything less
 *  is a few letters of a title, which names nothing. */
const MIN_LABEL = 96;

/** Slack under the measured label width. Canvas and DOM text widths agree to a
 *  fraction of a pixel; this makes a rounding difference an extra pixel of
 *  margin rather than a clipped letter. */
const MEASURE_SLACK = 2;

/** Fade in when the first section is this far up the window; fade out when the
 *  article's bottom is above the middle. Pinned to the window, the ruler only
 *  belongs on screen while the article's body is — not over a cover image above
 *  it or over the comments below it. */
const SHOW_BELOW = 0.85;
const HIDE_ABOVE = 0.5;

export interface RulerOptions {
  headings: HTMLElement[];
  /** Each heading's title, without its permalink anchor. */
  titles: string[];
  /** The nav's accessible name — the island's localized "sections" string. */
  label: string;
  /** The island: receives `data-ruler`, and emits `SECTION_SYNC_EVENT`. */
  island: HTMLElement;
  /** Space the island's bar occupies at the top of the window. */
  clearance: () => number;
}

/** Mount the ruler. Returns its disposer: removes the node and every listener. */
export function mountContentsRuler(opts: RulerOptions): () => void {
  const { headings, titles, island } = opts;
  const first = headings[0];
  const article = first.closest("article") ?? first.closest("main") ?? document.body;
  const undo: Array<() => void> = [];
  const on = (
    target: Window | Document | Element,
    type: string,
    handler: (event: Event) => void,
    options?: AddEventListenerOptions,
  ): void => {
    target.addEventListener(type, handler, options);
    undo.push(() => target.removeEventListener(type, handler, options));
  };

  const nav = document.createElement("nav");
  nav.className = "moss-contents-ruler";
  nav.setAttribute("aria-label", opts.label);
  nav.hidden = true;
  const list = document.createElement("ol");
  const links = headings.map((heading, i) => {
    const link = document.createElement("a");
    link.href = `#${heading.id}`;
    // The visible label may be cut short; the full title is the link's name.
    link.setAttribute("aria-label", titles[i]);
    link.title = titles[i];
    link.innerHTML =
      '<span class="moss-contents-ruler-dash"></span>' +
      '<span class="moss-contents-ruler-label" aria-hidden="true"></span>';
    const item = document.createElement("li");
    item.append(link);
    list.append(item);
    return link;
  });
  const labels = links.map(
    (link) => link.querySelector<HTMLElement>(".moss-contents-ruler-label")!,
  );
  nav.append(list);
  // Beside `main`, not inside it: `main` is a size container, which makes it
  // the containing block for fixed descendants and would pin the ruler to the
  // article instead of the window.
  const main = first.closest("main");
  if (main?.parentElement) main.after(nav);
  else document.body.append(nav);
  undo.push(() => nav.remove());

  let away = true;
  let disposed = false;

  /**
   * One frame's reading and writing, run when the island's scrollspy reports.
   *
   * The ruler counts as on screen when it is rendered (not `hidden`, and not
   * hidden by the site's own CSS) and not faded. The island hides its sections
   * button for exactly that case, so there is one contents menu at a time.
   * Reads come first, then the writes.
   */
  function fade(): void {
    if (disposed) return;
    const inBody = first.getBoundingClientRect().top < window.innerHeight * SHOW_BELOW;
    const notPast = article.getBoundingClientRect().bottom > window.innerHeight * HIDE_ABOVE;
    const rendered =
      !nav.hidden &&
      (nav.checkVisibility
        ? nav.checkVisibility({ checkVisibilityCSS: true })
        : getComputedStyle(nav).display !== "none");
    away = !(inBody && notPast) && !nav.matches(":focus-within");
    nav.toggleAttribute("data-away", away);
    island.dataset.ruler = rendered && !away ? "on" : "off";
  }

  function layout(): void {
    if (disposed) return;
    const vertical = document.body.dataset.typesetting === "vertical";
    // A hidden ruler has no boxes: show it, measure, then decide, all before
    // the next paint.
    nav.hidden = false;
    let room = 0;
    if (!vertical) {
      const textLeft = first.getBoundingClientRect().left;
      room = Math.floor(textLeft - GUTTER - labels[0].getBoundingClientRect().left);
    }
    const listHeight = list.getBoundingClientRect().height;
    const fits = (window.innerHeight - listHeight) / 2 >= opts.clearance();
    const eligible = !vertical && room >= MIN_LABEL && fits;
    nav.hidden = !eligible;
    if (eligible) {
      const measure = canvasMeasure(labels[0]);
      const padding = parseFloat(getComputedStyle(labels[0]).paddingInlineStart) || 0;
      // Writes only, no read-back: the canvas measure plus MEASURE_SLACK is
      // trusted, and the render gate asserts no label overflows in either engine.
      const fit = room - padding - MEASURE_SLACK;
      labels.forEach((label, i) => {
        label.style.maxWidth = `${room}px`;
        label.textContent = measure ? truncateMiddle(titles[i], fit, measure) : titles[i];
      });
    }
    fade();
  }

  // Re-measure whenever the text column may have moved. A ResizeObserver sees
  // size changes only; the reader's font-size control and the sidenote gutter
  // change the column by changing an attribute on the root, so those are
  // watched too. Coalesced to one pass per frame.
  let frame = 0;
  const relayout = (): void => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(layout);
  };
  undo.push(() => cancelAnimationFrame(frame));
  if (typeof ResizeObserver !== "undefined") {
    const ro = new ResizeObserver(relayout);
    ro.observe(article);
    ro.observe(first);
    undo.push(() => ro.disconnect());
  }
  if (typeof MutationObserver !== "undefined") {
    const mo = new MutationObserver(relayout);
    mo.observe(document.documentElement, { attributes: true });
    mo.observe(document.body, { attributes: true });
    undo.push(() => mo.disconnect());
  }
  on(window, "resize", relayout, { passive: true });
  document.fonts?.ready.then(relayout);

  // Keyboard focus keeps the ruler up even where it would have faded.
  on(nav, "focusin", fade);
  on(nav, "focusout", () => requestAnimationFrame(fade));

  // The one scrollspy lives in nav-island.ts; this is its output, once a frame.
  let marked = Number.NaN;
  on(island, SECTION_SYNC_EVENT, (event) => {
    fade();
    const { index } = (event as CustomEvent<SectionSyncDetail>).detail;
    if (index === marked) return;
    marked = index;
    markRows(links, index);
  });

  // Without hover (a tablet in landscape) the labels need a gesture of their
  // own: the first tap opens them, the next follows the link.
  on(nav, "click", (event) => {
    if (window.matchMedia?.("(hover: hover)").matches) return;
    if (!nav.hasAttribute("data-open")) {
      event.preventDefault();
      nav.setAttribute("data-open", "");
    }
  });
  on(document, "click", (event) => {
    if (!nav.contains(event.target as Node)) nav.removeAttribute("data-open");
  });

  layout();
  return () => {
    disposed = true;
    undo.forEach((f) => f());
    delete island.dataset.ruler;
  };
}
