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
 * - A plain ruler's label may reach no further than one gutter short of the
 *   text column, so its maximum width is `textLeft - GUTTER - labelLeft`, read
 *   from the layout. Titles longer than that, on either ruler, are one line
 *   ended with an ellipsis by CSS.
 * - Under `MIN_LABEL` of room a label names nothing, so the ruler stays away
 *   and the island is all there is.
 * - A `contents: pictures` ruler is up wherever the page reserves its lane:
 *   site.css narrows the column there and sets `--moss-contents-lane` on
 *   `body`, and this module reads only whether that is set, so the media
 *   condition has one owner and nothing here measures the margin. Where the
 *   lane is reserved the island's `data-ruler` is `lane` while the list is
 *   away, and its bar never shows.
 * - A site can switch it off with CSS (`display: none`, `visibility: hidden`);
 *   whether it is actually rendered is read on every pass, so the island
 *   comes back however it was hidden.
 * - It stays away on a right-to-left page, whose margin is on the other side,
 *   until the ruler mirrors; the island is the contents list there.
 * - It also stays away when the dashes would not fit between the island's bar
 *   and the bottom of the window — the list must not scroll, because scrolling
 *   would clip the labels that hang out of it.
 *
 * A page with `contents: pictures` (`opts.pictures`) is the same ruler at rest:
 * the same dashes, the same place, nothing reserved. Hovering or focusing it
 * magnifies the rows in place around the pointer, the focused one showing its
 * section's picture after its name (contents-lens.ts).
 *
 * The ruler belongs on screen from the first section to the end of the main
 * sections: the author's `contents=end` marker passing the middle of the
 * window, else the page's closing picture coming into view, else the
 * article's bottom passing the middle.
 *
 * Which of this needs an engine — hit-testing, label clearance, clipping — is
 * asserted in tests/render-gates/site/contents-ruler.spec.ts.
 */

import { markRows, SECTION_SYNC_EVENT, type SectionSyncDetail } from "./section-event";
import { mountContentsLens } from "./contents-lens";
import { pictureImg, type SectionPicture } from "./section-pictures";

/** Clear space between the widest label and the text column — the same gap
 *  the text keeps from a sidenote. */
const GUTTER = 24;

/** Narrowest label worth showing: about seven CJK characters. Anything less
 *  is a few letters of a title, which names nothing. */
const MIN_LABEL = 96;

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
  /** The island: receives `data-ruler` (`on` while the list is on screen,
   *  `lane` while it is away from a reserved lane, `off` otherwise), and
   *  emits `SECTION_SYNC_EVENT`. */
  island: HTMLElement;
  /** Space the island's bar occupies at the top of the window. */
  clearance: () => number;
  /** Set on a page with `contents: pictures`: each entry is a section's
   *  picture, or null for a section with no image. */
  pictures?: Array<SectionPicture | null>;
  /** The element the author marked as the end of the main sections
   *  (`contents=end`): the ruler steps aside once its top passes the middle of
   *  the window, and returns when the reader scrolls back above it. */
  end?: Element | null;
  /** The page's closing picture: with no marker, the ruler steps aside as
   *  soon as it comes into view. */
  closing?: Element | null;
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
  if (opts.pictures) nav.dataset.contents = "pictures";
  const list = document.createElement("ol");
  const links = headings.map((heading, i) => {
    const link = document.createElement("a");
    link.href = `#${heading.id}`;
    // The visible label may be cut short by CSS; the full title is the link's name.
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
      link.append(frame);
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
  labels.forEach((label, i) => (label.textContent = titles[i]));
  nav.append(list);
  // Beside `main`, not inside it: `main` is a size container, which makes it
  // the containing block for fixed descendants and would pin the ruler to the
  // article instead of the window.
  const main = first.closest("main");
  if (main?.parentElement) main.after(nav);
  else document.body.append(nav);
  undo.push(() => nav.remove());
  if (opts.pictures) undo.push(mountContentsLens({ nav, links }));

  // Blocks wider than the text column (`data-width`) can sit anywhere in
  // `main`, and script can add them after load: the list is read on layout and
  // whenever the scope's children change, and tested each frame. What is
  // tested is each block's children, not the block: a band whose content
  // keeps the text column covers nothing, while a wide picture or grid covers
  // the margin through its own child.
  const scope = first.closest("main") ?? article;
  let bands: Element[] = [];
  const collectBands = (): void => {
    bands = [...scope.querySelectorAll("[data-width]:not([data-width=body])")].flatMap((band) => [...band.children]);
  };
  collectBands();
  let away = true;
  /** The page reserves the list's lane and the list fits: set on layout. */
  let lane = false;
  let disposed = false;

  /**
   * One frame's reading and writing, run when the island's scrollspy reports.
   *
   * The ruler counts as on screen when it is rendered (not `hidden`, and not
   * hidden by the site's own CSS) and not faded. The island stays put away for
   * exactly that case, so there is one contents list at a time.
   * Reads come first, then the writes.
   */
  function fade(): void {
    if (disposed) return;
    const height = window.innerHeight;
    const inBody = first.getBoundingClientRect().top < height * SHOW_BELOW;
    const notPast = opts.end
      ? opts.end.getBoundingClientRect().top > height * HIDE_ABOVE
      : opts.closing
        ? opts.closing.getBoundingClientRect().top > height
        : article.getBoundingClientRect().bottom > height * HIDE_ABOVE;
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
    island.dataset.ruler = rendered && !away ? "on" : lane ? "lane" : "off";
  }

  function layout(): void {
    if (disposed) return;
    const vertical = document.body.dataset.typesetting === "vertical";
    const rtl = getComputedStyle(first).direction === "rtl";
    // A hidden ruler has no boxes: show it, measure, then decide, all before
    // the next paint.
    nav.hidden = false;
    collectBands();
    const textLeft = vertical ? 0 : first.getBoundingClientRect().left;
    const listBox = list.getBoundingClientRect();
    // Up to the gutter before the text is the margin the plain ruler's titles
    // may use.
    const room = Math.floor(textLeft - GUTTER - labels[0].getBoundingClientRect().left);
    const fits = (window.innerHeight - listBox.height) / 2 >= opts.clearance();
    const eligible = opts.pictures
      ? getComputedStyle(document.body).getPropertyValue("--moss-contents-lane").trim() !== "" && fits
      : !vertical && !rtl && room >= MIN_LABEL && fits;
    nav.hidden = !eligible;
    lane = Boolean(opts.pictures) && eligible;
    if (eligible && !opts.pictures) labels.forEach((label) => (label.style.maxWidth = `${room}px`));
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
