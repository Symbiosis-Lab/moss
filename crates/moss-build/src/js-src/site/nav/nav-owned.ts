/**
 * The masthead's `data-nav-*` state: its modes, their one writer, and the read
 * of whether the theme lays the masthead out itself. The build inlines this
 * file right after the masthead (emit/scripts.rs), so the read below runs
 * before the first paint, once the stylesheets in `<head>` have arrived, and a
 * theme that owns its masthead never paints under the float. nav-split.ts
 * imports it for the rest, so theme.js repeats the read on load, harmlessly.
 */

const MODES = ["compact", "split", "stack", "scroll", "flat"] as const;
export type Mode = (typeof MODES)[number];

/** The one writer of `.nav-content`'s `data-nav-*`: at most one mode, plus `data-nav-owned` for a masthead its theme lays out. */
export function setMode(el: HTMLElement, mode: Mode | null, owned = false): void {
  for (const m of MODES) el.removeAttribute(`data-nav-${m}`);
  if (mode) el.setAttribute(`data-nav-${mode}`, "");
  el.toggleAttribute("data-nav-owned", owned);
}

/**
 * Marks a masthead its theme lays out itself: `.nav-right` flattened, or the
 * row held to one line (`flex-wrap: nowrap`, often with a fixed height). Read
 * with the float layout off, since the float flattens `.nav-right` too. Clears
 * any mode. nav-split.ts re-reads it on every layout, since a theme may
 * flatten only below some width.
 */
export function readOwned(navContent: HTMLElement): { owned: boolean; flattened: boolean } {
  setMode(navContent, null, true);
  const navRight = navContent.querySelector<HTMLElement>(".nav-right");
  const flattened = navRight !== null && getComputedStyle(navRight).display === "contents";
  const owned = flattened || getComputedStyle(navContent).flexWrap === "nowrap";
  setMode(navContent, null, owned);
  return { owned, flattened };
}

const navContent = document.querySelector<HTMLElement>(".nav-content");
if (navContent) {
  const { owned, flattened } = readOwned(navContent);
  // A row the theme holds to one line and that overflows scrolls its links:
  // nav-split.ts's choice where no menu button shows, off a phone (which first
  // tries tight gaps), on a horizontal page.
  const style = getComputedStyle(navContent);
  const menu = navContent.querySelector<HTMLElement>(".mobile-menu-button");
  if (
    owned &&
    !flattened &&
    (!menu || getComputedStyle(menu).display === "none") &&
    style.getPropertyValue("--moss-nav-phone").trim() !== "1" &&
    !style.writingMode.startsWith("vertical") &&
    navContent.scrollWidth > navContent.clientWidth
  ) {
    setMode(navContent, "scroll", true);
  }
}
