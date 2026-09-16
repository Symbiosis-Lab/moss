/**
 * nav-island.ts — the floating nav island's behaviour (ADR-049).
 *
 * The markup ships from `build/components/nav.rs` and is complete without this
 * file: every link in the island is one the masthead also carries, and the
 * island is `display: none` until this module writes `data-shown` on it. So
 * with JavaScript off, a moss page behaves exactly as it did before ADR-049.
 * What this module adds is the four things CSS cannot do:
 *
 * 1. **Reveal.** Show the bar when the reader scrolls back UP past the
 *    masthead. Scrolling up is already the gesture that means "I want to go
 *    back", so the nav arriving then is not an interruption.
 * 2. **Folding.** Keep the trail to ONE line at any width by dropping
 *    ancestors from the middle outward into a `…` button. This is measured,
 *    never breakpointed: a media query encodes a guess about how long a site's
 *    titles are, and that guess is wrong for every site but the one it was
 *    tuned on.
 * 3. **The two panels** — the folded levels, and this page's sections.
 * 4. **Reading progress**, along the island's own bottom edge.
 *
 * The folding itself — the algorithm, the measurement rules, the panel
 * placement — lives in `breadcrumb-fold.ts`, because the masthead folds the
 * same way (one behaviour, two consumers). What stays here is the island's
 * own wiring: reveal, the two panels' mutual exclusion, scrollspy, teardown.
 * The geometry that only an engine can answer — that the row never wraps and
 * never overflows, that the panel's labels all start at the same x — is
 * asserted in tests/render-gates/site/nav-island.spec.ts.
 */

import { applyFold, inlineAvailableExtent, placePanel } from "./breadcrumb-fold";

/**
 * Fallback clearance below the island's own bottom edge before a heading
 * counts as "the section you are in", used only when the root carries no
 * `scroll-padding-top` (see `scrollspyLine`). The LINE itself is measured
 * from the bar — hard-coding the whole thing was a guess at the island's
 * height, and the same species of guess ADR-049 §5 rejects for folding:
 * change the bar's padding and the scrollspy quietly stops agreeing with
 * the screen.
 */
const SCROLLSPY_CLEARANCE = 28;

/** Ignore scroll jitter below this many pixels when deciding direction. */
const JITTER = 2;

/**
 * How far below the scrollspy line a heading may sit and still count as having
 * reached it. Sub-pixel: see `markCurrentSection`.
 */
const LANDING_TOLERANCE = 1;

/**
 * How many sections make a contents table — and so, since ADR-049 §10 as
 * amended 2026-08-30, whether the island appears at all.
 *
 * Two. A list of one names the page's only section, which the title directly
 * above it already said, so the bar would be summoned to repeat what the
 * reader just read. Wikis set this higher (MediaWiki auto-shows a TOC at four)
 * because their contents block is in the flow and pushes the article down;
 * this one lives behind a button in a bar you have to scroll up to ask for, so
 * an extra row costs nothing and only the list of one is genuinely empty.
 */
const MIN_SECTIONS = 2;

// ---------------------------------------------------------------------------
// DOM wiring
// ---------------------------------------------------------------------------

/** Everything the previous `initNavIsland()` bound, undone on the next call. */
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

/**
 * This page's sections: every heading at the shallowest level the body uses.
 *
 * `h1` is excluded because it is the page title, not a section of it. From
 * `h2` down, the first level that matches anything wins — so a body written
 * with `##` answers `h2` (the overwhelmingly common case, unchanged), while a
 * body whose author started at `###` or `####` answers with those instead of
 * reporting no sections at all.
 *
 * Exported for the tests: which headings become the panel is a decision worth
 * stating as data, the same way `foldPlan` is.
 */
export function sectionHeadings(root: ParentNode = document): HTMLElement[] {
  for (const level of ["h2", "h3", "h4", "h5", "h6"]) {
    const found = [...root.querySelectorAll<HTMLElement>(`main ${level}[id]`)];
    if (found.length > 0) return found;
  }
  return [];
}

/** A heading's title, without the permalink anchor moss appends to it. */
function headingText(heading: HTMLElement): string {
  const copy = heading.cloneNode(true) as HTMLElement;
  copy.querySelector(".moss-heading-anchor")?.remove();
  return (copy.textContent ?? "").trim();
}

export function initNavIsland(): void {
  // Undo the previous wiring FIRST and unconditionally. A morph to a page with
  // no island leaves the old nodes detached, and listeners still holding them
  // keep that dead subtree alive for as long as the tab does. Doing this after
  // the early return below would skip exactly the case that needs it.
  teardown.forEach((undo) => undo());
  teardown = [];

  const island = document.querySelector<HTMLElement>(".moss-nav-island");
  if (!island) return;

  const bar = island.querySelector<HTMLElement>(".moss-nav-island-bar");
  const trail = island.querySelector<HTMLElement>(".moss-nav-island-trail");
  if (!bar || !trail) return;

  // Writing `data-shown` is what makes the island renderable at all. Do it
  // before the display probe below, or the probe reads the JS-off state and
  // always says "off". `inert` starts on to match: hidden means out of the tab
  // order and out of the accessibility tree, not merely transparent.
  island.dataset.shown = "false";
  island.inert = true;

  const crumbs = [...trail.querySelectorAll<HTMLElement>("[data-island-crumb]")];

  // Which headings become the sections panel — and, since ADR-049 §10 as
  // amended 2026-08-30, whether there is an island at all.
  //
  // One level only. A flat list stays scannable at a glance, and every heading
  // moss emits already carries an `id`, so each row is a real link that works
  // with the panel closed. Which level that is comes from the page rather than
  // a constant: `h2` is the norm and is still what almost every page answers
  // with — but an author who started their sections at `###`, or at `####`,
  // has sections all the same, and hard-coding `h2` told them they had none.
  const headings = sectionHeadings();

  const moreBtn = trail.querySelector<HTMLButtonElement>(".moss-nav-island-more");
  const moreSep = trail.querySelector<HTMLElement>("[data-trail-more-separator]");
  const sectionsBtn = island.querySelector<HTMLButtonElement>(".moss-nav-island-sections");
  const levelsMenu = island.querySelector<HTMLElement>('[data-island-menu="levels"]');
  const sectionsMenu = island.querySelector<HTMLElement>('[data-island-menu="sections"]');
  const progress = island.querySelector<HTMLElement>(".moss-nav-island-progress-fill");
  const header = document.querySelector<HTMLElement>("header");

  // Empty the sections panel FIRST, and unconditionally. A morph swaps the
  // document under an island that survives, so rows left from the previous
  // page are worse than useless: they name sections the reader cannot see and
  // link to ids no longer in the document. This has to happen on the way to a
  // dormant island too, which is why it sits above the gate rather than beside
  // the rebuild below.
  if (sectionsMenu && sectionsBtn) {
    sectionsMenu.replaceChildren();
    sectionsMenu.hidden = true;
    sectionsBtn.setAttribute("aria-expanded", "false");
  }

  // Two ways for a page to have no island, and one exit for both: remove
  // `data-shown` — without it site.css leaves the island `display: none` — and
  // bind nothing, so the markup is left exactly as emitted.
  //
  //  1. No contents table. The island appears only where the page has two or
  //     more sections (ADR-049 §10 as amended 2026-08-30, design note
  //     docs/archive/2026-08-30-nav-island-heading-gate-design.md). Everything
  //     else it carries — the trail, the progress rule — is a second copy of
  //     something the masthead holds one scroll-to-top away; the sections
  //     panel is the only thing it alone offers, so it is the only thing that
  //     earns a bar. Trail depth used to rescue a heading-less page and no
  //     longer does: that spared the rare deep essay without sections and made
  //     every short page on every site pay for it.
  //  2. The theme's off-switch, `--moss-nav-island-display: none`. Read as a
  //     computed value rather than as a custom property so a site can disable
  //     the island from inside a media query. It is checked second because it
  //     forces a style read and the heading count does not.
  if (
    headings.length < MIN_SECTIONS ||
    getComputedStyle(island).display === "none"
  ) {
    island.removeAttribute("data-shown");
    island.inert = false;
    return;
  }

  if (sectionsMenu) {
    headings.forEach((heading) => {
      const row = document.createElement("a");
      row.href = `#${heading.id}`;
      row.textContent = headingText(heading);
      row.addEventListener("click", () => closeMenus());
      sectionsMenu.appendChild(row);
    });
  }

  // -------------------------------------------------------------------------
  // Reveal
  // -------------------------------------------------------------------------

  let lastY = window.scrollY;
  let shown = false;

  /** The masthead's own bottom edge — above it the island would only repeat
   *  what is already on screen, which is also why it never fights a hero. */
  const mastheadBottom = (): number =>
    header ? header.offsetTop + header.offsetHeight : 0;

  /**
   * Show or hide the island — and take it out of the page entirely while it is
   * hidden.
   *
   * `inert` is what makes "hidden" mean hidden. Without it the bar is merely
   * transparent: its links stay in the tab order and in the accessibility
   * tree, so every breadcrumbed page announced its breadcrumb twice, and one
   * Tab from the top of the document landed inside an invisible bar sitting on
   * top of the masthead — the trap this was supposed to prevent, arrived at
   * from the other direction.
   *
   * It also settles what "reveal on focus" was for. The island holds no
   * destination the masthead does not also hold (ADR-049 §1), so there is
   * nothing a keyboard reader loses by it being unreachable while invisible —
   * and scrolling up, which is how they reveal it, is a key press away.
   */
  function setShown(next: boolean): void {
    if (next === shown) return;
    shown = next;
    island.dataset.shown = String(next);
    island.inert = !next;
    if (!next) closeMenus();
  }

  function onScroll(): void {
    const y = window.scrollY;
    const past = y > mastheadBottom();

    if (!past) {
      setShown(false);
    } else if (y < lastY - JITTER) {
      setShown(true);
    } else if (y > lastY + JITTER) {
      setShown(false);
    }
    lastY = y;

    if (progress) {
      const doc = document.documentElement;
      const scrollable = doc.scrollHeight - doc.clientHeight;
      const pct = scrollable > 0 ? (y / scrollable) * 100 : 0;
      progress.style.width = `${Math.max(0, Math.min(100, pct))}%`;
    }

    if (sectionsMenu && !sectionsMenu.hidden) markCurrentSection();
  }

  on(window, "scroll", onScroll, { passive: true });

  // Tabbing past the last row of an open panel used to leave it open behind the
  // page, its button still saying `aria-expanded="true"`, with focus landed on
  // whatever sat underneath. A panel closes when focus leaves the island for
  // the same reason it closes on an outside click: the reader has moved on.
  on(island, "focusout", (event) => {
    const next = (event as FocusEvent).relatedTarget as Node | null;
    if (next && island.contains(next)) return;
    closeMenus();
  });

  // -------------------------------------------------------------------------
  // Folding
  // -------------------------------------------------------------------------

  /** The folded level names, kept so the tooltip can be restored after a close. */
  let levelNames = "";

  function layoutTrail(): void {
    if (!moreBtn || !moreSep || !levelsMenu) return;
    // The algorithm and its measurement rules live in breadcrumb-fold.ts —
    // shared with the masthead, which folds the same way.
    levelNames = applyFold({
      trail,
      crumbs,
      moreBtn,
      moreSep,
      menu: levelsMenu,
      available: () => inlineAvailableExtent(trail),
    });
  }

  // The trail is a flex item that resizes with the window, with the font, and
  // with a sibling appearing after a morph. Observing the bar catches all three
  // — `resize` alone catches only the first.
  if (typeof ResizeObserver !== "undefined") {
    const ro = new ResizeObserver(() => layoutTrail());
    ro.observe(bar);
    teardown.push(() => ro.disconnect());
  }
  on(window, "resize", () => layoutTrail(), { passive: true });

  // -------------------------------------------------------------------------
  // Sections
  // -------------------------------------------------------------------------

  /**
   * The line a heading must cross to count as the current section.
   *
   * This is the root's `scroll-padding-top` when there is one, because that
   * IS where a fragment jump parks its target — and a section-panel row is an
   * ordinary fragment link whose click leaves the panel open, so the two have
   * to agree. Measuring the bar and adding a constant instead put the line
   * above the landing position: clicking "Section 3" jumped to Section 3 and
   * then highlighted Section 2.
   */
  function scrollspyLine(): number {
    const reserved = parseFloat(
      getComputedStyle(document.documentElement).scrollPaddingTop,
    );
    if (Number.isFinite(reserved) && reserved > 0) return reserved;
    return bar.getBoundingClientRect().bottom + SCROLLSPY_CLEARANCE;
  }

  /**
   * Mark the row for the last heading whose top has passed the island.
   *
   * The comparison carries a one-pixel tolerance because "has passed" is a
   * knife edge: a fragment jump parks the heading AT the line, and a page
   * whose layout is fractional parks it a fraction below — measured 110.125
   * against a line of 110. Nothing about that is the reader's scroll position,
   * so an exact `<=` marked the previous section for the whole class of pages
   * with a non-integer type scale, which is every Chinese page (the CJK size
   * multiplier) and every page at a reader font-scale step other than the
   * default. It only became visible when the Latin default joined them.
   */
  function markCurrentSection(): void {
    if (!sectionsMenu) return;
    const line = scrollspyLine() + LANDING_TOLERANCE;
    let current = -1;
    headings.forEach((heading, i) => {
      if (heading.getBoundingClientRect().top <= line) current = i;
    });
    [...sectionsMenu.children].forEach((row, i) => {
      if (i === current) row.setAttribute("aria-current", "true");
      else row.removeAttribute("aria-current");
    });
  }

  // -------------------------------------------------------------------------
  // The two panels
  // -------------------------------------------------------------------------

  const panels: Array<[HTMLElement | null, HTMLButtonElement | null]> = [
    [levelsMenu, moreBtn],
    [sectionsMenu, sectionsBtn],
  ];

  function closeMenus(except?: HTMLElement): void {
    panels.forEach(([menu, button]) => {
      if (!menu || !button || menu === except) return;
      menu.hidden = true;
      button.setAttribute("aria-expanded", "false");
    });
    // The `…` names its levels on hover again, now that they are not on screen.
    if (moreBtn && levelsMenu?.hidden && levelNames) {
      moreBtn.setAttribute("data-tooltip", levelNames);
    }
  }

  /** `moveFocus` puts the caret on the first row. Keyboard opens want that —
   *  it is the only way to reach the list. Mouse opens must NOT have it: the
   *  focus ring it draws reads as a box around the first entry that the reader
   *  never asked for and cannot dismiss, and the pointer is already where it
   *  needs to be. */
  function togglePanel(
    menu: HTMLElement,
    button: HTMLButtonElement,
    moveFocus = false,
  ): void {
    const opening = menu.hidden;
    closeMenus(opening ? menu : undefined);
    menu.hidden = !opening;
    button.setAttribute("aria-expanded", String(opening));

    // A tooltip naming the folded levels, stacked on top of the open panel
    // that lists those same levels, is the tooltip covering its own answer.
    // Drop it while the panel is up; `closeMenus` puts it back.
    if (menu === levelsMenu && moreBtn) {
      if (opening) moreBtn.removeAttribute("data-tooltip");
      else if (levelNames) moreBtn.setAttribute("data-tooltip", levelNames);
    }
    if (!opening) return;

    // The panels are siblings of the bar (it clips its overflow), so their
    // containing block is `.moss-nav-island` — the full-width fixed wrapper.
    placePanel(menu, button, island, bar);

    if (moveFocus) menu.querySelector("a")?.focus();
  }

  /** A button's `click` fires for Enter/Space too, and that is the one case
   *  that needs focus moved into the panel. `detail` is the click count: a
   *  real pointer press reports 1 or more, a keyboard-synthesized one 0. */
  const openedByKeyboard = (event: Event): boolean =>
    (event as MouseEvent).detail === 0;

  if (moreBtn && levelsMenu) {
    on(moreBtn, "click", (event) =>
      togglePanel(levelsMenu, moreBtn, openedByKeyboard(event)),
    );
  }
  if (sectionsBtn && sectionsMenu) {
    on(sectionsBtn, "click", (event) => {
      markCurrentSection();
      togglePanel(sectionsMenu, sectionsBtn, openedByKeyboard(event));
    });
  }

  // Escape closes and hands focus back to whatever opened the panel — losing
  // focus to <body> would drop a keyboard reader at the top of the document.
  on(document, "keydown", (event) => {
    if ((event as KeyboardEvent).key !== "Escape") return;
    const opener = panels.find(([menu]) => menu && !menu.hidden)?.[1];
    if (!opener) return;
    closeMenus();
    opener.focus();
  });

  on(document, "pointerdown", (event) => {
    const target = event.target as Element | null;
    if (target?.closest(".moss-nav-island")) return;
    closeMenus();
  });

  layoutTrail();
  onScroll();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initNavIsland);
} else {
  initNavIsland();
}

// A preview rebuild morphs the page in place rather than reloading it, so the
// island can be replaced by brand-new nodes none of the listeners above have
// ever seen — and the section list has to be rebuilt from the new headings.
document.addEventListener("moss-morph-patched", initNavIsland);
