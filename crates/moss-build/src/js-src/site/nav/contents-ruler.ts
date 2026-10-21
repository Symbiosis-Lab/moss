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
 * A page that asks for its contents open (`opts.pictures`, from
 * `contents: open`) keeps the same list and the same measuring, but shows the
 * titles at rest, at the page's body size, with a small picture of the
 * section's first image in place of the dash; a section with no image keeps
 * its dash.
 *
 * Every dash row is the same height at rest and open. Re-spacing on hover was
 * tried and rejected: the row under the pointer moves, and the click lands on
 * a different section.
 *
 * Which of this needs an engine — hit-testing, label clearance, clipping — is
 * asserted in tests/render-gates/site/contents-ruler.spec.ts.
 */

import { canvasMeasure, truncateMiddle, type Measure } from "./middle-truncate";
import { markRows, SECTION_SYNC_EVENT, type SectionSyncDetail } from "./section-event";
import { pictureImg } from "./section-pictures";

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

/** The open list's column is sized from its own widest title, but may take at
 *  most this share of the page's text measure (`--moss-content-width`, so a
 *  full-width page allows more than a narrow one); a longer title is cut. */
const MAX_COLUMN = 0.6;

/** Share of the text measure the text must keep after the shift. Together with
 *  `MAX_COLUMN` it means the list never takes more than the text leaves it;
 *  under this the list gives way and the island's panel is the contents. */
const MIN_TEXT = 0.4;

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
  /** Set when the page asked for its contents to stay open (`contents: open`):
   *  the titles show at rest, and each entry is a picture URL, or null for a
   *  section with no image, which keeps its dash. */
  pictures?: Array<string | null>;
  /** The element the author marked as the end of the main sections
   *  (`contents=end`): the ruler steps aside once its top passes the middle of
   *  the window, and returns when the reader scrolls back above it. */
  end?: Element | null;
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
  if (opts.pictures) nav.dataset.contents = "open";
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
    const picture = opts.pictures?.[i];
    if (picture) {
      const frame = document.createElement("span");
      frame.className = "moss-contents-ruler-pic";
      frame.setAttribute("aria-hidden", "true");
      frame.append(pictureImg(picture, () => link.removeAttribute("data-pic")));
      link.prepend(frame);
      link.setAttribute("data-pic", "");
    }
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

  /**
   * The left edge the list must clear. The first heading's by default; for an
   * open list the leftmost painted edge of the content: a page whose sections
   * sit in boxes or panels starts further left than its text, and the labels
   * would touch the box. Full-bleed bands (`data-width`) are meant to run past
   * the column and are not counted.
   */
  function contentLeft(): number {
    const text = first.getBoundingClientRect().left;
    if (!opts.pictures) return text;
    const boxes = [...article.children].filter((el) => !el.hasAttribute("data-width"));
    for (let el = first.parentElement; el && el !== article; el = el.parentElement) boxes.push(el);
    return Math.min(text, ...boxes.map((el) => el.getBoundingClientRect().left));
  }

  // Blocks wider than the text column (`data-width`) can sit anywhere in
  // `main`, and script can add them after load: the list is read on layout and
  // whenever the scope's children change, and tested each frame.
  const scope = first.closest("main") ?? article;
  let bands: Element[] = [];
  const collectBands = (): void => {
    bands = [...scope.querySelectorAll("[data-width]:not([data-width=body])")];
  };
  collectBands();
  let away = true;
  let disposed = false;
  let mo: MutationObserver | undefined;

  /**
   * Make room in the left margin for an open list: pad the body (as the
   * sidenote gutter does on the right) so the text starts `column` in. The
   * padding is derived from measured geometry — the text column's natural
   * left edge, with the shift removed — and is taken back when the shifted
   * column would be narrower than `MIN_TEXT`. A centred column that already
   * has the margin is not moved at all.
   */
  function reserveColumn(vertical: boolean, measure: Measure | null): void {
    if (!opts.pictures) return;
    const style = document.body.style;
    const set = (px: number): void => style.setProperty("--moss-contents-inset", `${px}px`);
    set(0);
    if (vertical) return;
    const box = article.getBoundingClientRect();
    const offset = contentLeft() - box.left;
    // The column is the list's own width: where its labels start, the widest
    // title at the open size, and a gutter before the text.
    const padding = parseFloat(getComputedStyle(labels[0]).paddingInlineStart) || 0;
    const widest = measure ? Math.max(...titles.map(measure)) : 0;
    const textMeasure = parseFloat(getComputedStyle(article).maxWidth) || box.width;
    const column = Math.min(
      MAX_COLUMN * textMeasure,
      // Never less than a label that names something (`MIN_LABEL`).
      Math.ceil(labels[0].getBoundingClientRect().left + Math.max(MIN_LABEL, padding + widest + MEASURE_SLACK) + GUTTER),
    );
    const need = column - offset;
    // A centred block moves half of what the body pads; one that fills the
    // window shrinks instead, so the pad is capped at the shift it needs.
    const pad = Math.min(need, Math.max(0, 2 * (need - box.left)));
    if (pad <= 0) return;
    set(pad);
    if (article.getBoundingClientRect().width - 2 * offset < MIN_TEXT * textMeasure) set(0);
  }

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
    const scopeEnd = (opts.end ?? article).getBoundingClientRect();
    const notPast = (opts.end ? scopeEnd.top : scopeEnd.bottom) > window.innerHeight * HIDE_ABOVE;
    // A wide block that reaches the ruler's column and shares its height would
    // be painted over: the ruler steps aside until the block has passed.
    // Only worth measuring while the ruler would otherwise be up.
    const zone = inBody && notPast && !nav.hidden ? list.getBoundingClientRect() : null;
    const covered =
      zone !== null &&
      bands.some((band) => {
        const box = band.getBoundingClientRect();
        return box.left < zone.right && box.top < zone.bottom && box.bottom > zone.top;
      });
    const rendered =
      !nav.hidden &&
      (nav.checkVisibility
        ? nav.checkVisibility({ checkVisibilityCSS: true })
        : getComputedStyle(nav).display !== "none");
    away = !(inBody && notPast && !covered) && !nav.matches(":focus-within");
    nav.toggleAttribute("data-away", away);
    island.dataset.ruler = rendered && !away ? "on" : "off";
  }

  function layout(): void {
    if (disposed) return;
    const vertical = document.body.dataset.typesetting === "vertical";
    // A hidden ruler has no boxes: show it, measure, then decide, all before
    // the next paint.
    nav.hidden = false;
    collectBands();
    const measure = canvasMeasure(labels[0]);
    reserveColumn(vertical, measure);
    let room = 0;
    if (!vertical) {
      const textLeft = contentLeft();
      room = Math.floor(textLeft - GUTTER - labels[0].getBoundingClientRect().left);
    }
    const listHeight = list.getBoundingClientRect().height;
    const fits = (window.innerHeight - listHeight) / 2 >= opts.clearance();
    const eligible = !vertical && room >= MIN_LABEL && fits;
    nav.hidden = !eligible;
    if (!eligible && opts.pictures) document.body.style.removeProperty("--moss-contents-inset");
    // The shift is ours, not a change of the page's: do not re-measure for it.
    if (opts.pictures) mo?.takeRecords();
    if (eligible) {
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
    const bandsMo = new MutationObserver(collectBands);
    bandsMo.observe(scope, { childList: true, subtree: true });
    undo.push(() => bandsMo.disconnect());
    mo = new MutationObserver(relayout);
    mo.observe(document.documentElement, { attributes: true });
    mo.observe(document.body, { attributes: true });
    undo.push(() => mo?.disconnect());
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
    if (opts.pictures) return;
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
    if (opts.pictures) document.body.style.removeProperty("--moss-contents-inset");
  };
}
