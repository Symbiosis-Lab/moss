/**
 * masthead-fold.ts — the masthead breadcrumb folds like the island.
 *
 * The two trails used to shrink two different ways: the island folded whole
 * ancestors into a `…`, the masthead ellipsised every middle
 * segment down to an unreadable stub — on a phone a deep trail read as
 * "潮汐週報 / 獎.. / 寫.. / 第.. / 戰火下的文學抉…". Half an ancestor name
 * tells the reader nothing, so the masthead now consumes the same fold
 * (`breadcrumb-fold.ts`): middle segments either fit whole or fold whole into
 * a `…` button that names them on hover and opens them on click, and only
 * the last segment (the parent page — the masthead skips the current page)
 * is allowed to ellipsise.
 *
 * `nav.rs` emits the `…` button, its separator, and the levels panel only
 * when the trail is deep enough to have a middle (three or more crumbs);
 * everything ships `hidden`, so with JavaScript off the masthead is exactly
 * the shipped CSS ellipsis ladder — that ladder is the no-script base state,
 * not dead code. This module writes `data-fold-managed` on `.nav-left`
 * before its first measurement, which is what switches the middles from
 * "shrink with ellipsis" to "never shrink" (site.css) so the fold can
 * measure their honest widths.
 */

import { applyFold, inlineAvailableExtent, placePanel } from "./breadcrumb-fold";

/** Everything the previous `initMastheadFold()` bound, undone on the next call. */
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

export function initMastheadFold(): void {
  // Undo the previous wiring FIRST and unconditionally — a morph can replace
  // the nav with markup that has no fold controls, and listeners still
  // holding the old nodes would keep that dead subtree alive.
  teardown.forEach((undo) => undo());
  teardown = [];

  const navLeft = document.querySelector<HTMLElement>(".nav-left");
  if (!navLeft) return;
  const navContent = navLeft.closest<HTMLElement>(".nav-content");
  const moreBtn = navLeft.querySelector<HTMLButtonElement>(".moss-breadcrumb-more");
  const moreSep = navLeft.querySelector<HTMLElement>("[data-trail-more-separator]");
  const menu = navContent?.querySelector<HTMLElement>(".moss-breadcrumb-menu") ?? null;
  const crumbs = [...navLeft.querySelectorAll<HTMLElement>("[data-trail-crumb]")];
  // A shallow trail (or a plain site-name masthead) ships no fold controls at
  // all — nothing to do, and the CSS ladder never engages either because a
  // two-crumb trail's only shrinkable segment is the parent, which ellipsises
  // the same under both regimes.
  if (!navContent || !moreBtn || !moreSep || !menu || crumbs.length < 3) return;

  navLeft.setAttribute("data-fold-managed", "");

  /** The folded level names, kept so the tooltip can be restored after a close. */
  let levelNames = "";

  function layout(): void {
    levelNames = applyFold({
      trail: navLeft!,
      crumbs,
      moreBtn: moreBtn!,
      moreSep: moreSep!,
      menu: menu!,
      // `.nav-left` takes the row's free space (flex-grow 999) and clips its
      // overflow, so its own box IS the room on offer. applyFold reads this
      // before unhiding anything — unhiding first can bounce `.nav-right`
      // between rows and the measurement would chase a layout that stops
      // existing the moment the fold is re-applied. Under vertical writing
      // the trail's flex row runs top-to-bottom, so the room on offer is a
      // height, not a width — `inlineAvailableExtent` reads whichever one is
      // actually the trail's inline axis.
      available: () => inlineAvailableExtent(navLeft!),
    });
  }

  function closeMenu(): void {
    if (!menu!.hidden) {
      menu!.hidden = true;
      moreBtn!.setAttribute("aria-expanded", "false");
      // The `…` names its levels on hover again, now that they are off screen.
      if (levelNames) moreBtn!.setAttribute("data-tooltip", levelNames);
    }
  }

  on(moreBtn, "click", (event) => {
    const opening = menu!.hidden;
    menu!.hidden = !opening;
    moreBtn!.setAttribute("aria-expanded", String(opening));
    if (!opening) {
      if (levelNames) moreBtn!.setAttribute("data-tooltip", levelNames);
      return;
    }
    // A tooltip naming the folded levels, stacked on top of the open panel
    // that lists those same levels, is the tooltip covering its own answer.
    moreBtn!.removeAttribute("data-tooltip");
    // The panel is a sibling of `.nav-left` (it clips its overflow), so its
    // containing block is `.nav-content` — but the bar it belongs to is
    // `.nav-left`, the breadcrumb row alone. Those two boxes are the same on
    // desktop and very different on a phone, where `.nav-content` also holds
    // the nav links and icon row below: clamping to `.nav-content` hung the
    // folded levels off the bottom of the WHOLE header, a row and a half
    // below the `…` and on top of the article.
    placePanel(menu!, moreBtn!, navContent!, navLeft!);
    // Keyboard opens move focus to the first row — it is the only way to
    // reach the list. Pointer opens must not: `detail` is the click count,
    // 0 for a keyboard-synthesized click.
    if ((event as MouseEvent).detail === 0) menu!.querySelector("a")?.focus();
  });

  // Escape closes and hands focus back to the `…` — losing focus to <body>
  // would drop a keyboard reader at the top of the document.
  on(document, "keydown", (event) => {
    if ((event as KeyboardEvent).key !== "Escape" || menu!.hidden) return;
    closeMenu();
    moreBtn!.focus();
  });

  on(document, "pointerdown", (event) => {
    const target = event.target as Element | null;
    if (target?.closest(".moss-breadcrumb-menu, .moss-breadcrumb-more")) return;
    closeMenu();
  });

  // The panel closes when focus leaves it for the same reason it closes on an
  // outside click: the reader has moved on.
  on(navContent, "focusout", (event) => {
    const next = (event as FocusEvent).relatedTarget as Node | null;
    if (next && (menu!.contains(next) || next === moreBtn)) return;
    closeMenu();
  });

  // The trail resizes with the window, with the font, and with a sibling
  // appearing after a morph. Observing `.nav-left` catches all three —
  // `resize` alone catches only the first.
  if (typeof ResizeObserver !== "undefined") {
    const ro = new ResizeObserver(() => layout());
    ro.observe(navLeft);
    teardown.push(() => ro.disconnect());
  }
  on(window, "resize", () => layout(), { passive: true });

  layout();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initMastheadFold);
} else {
  initMastheadFold();
}

// A preview rebuild morphs the page in place rather than reloading it, so the
// nav can be replaced by brand-new nodes none of the listeners above have
// ever seen.
document.addEventListener("moss-morph-patched", initMastheadFold);
