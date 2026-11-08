/**
 * header-scroll.ts — `[site].header_scroll`.
 *
 * The stylesheet does the sticking (`body[data-header-scroll] > header`), so
 * `sticky` and `hide` both work without this file. What it adds is what CSS
 * cannot decide:
 *
 * 1. `--moss-header-height`, the header's measured height, which the root's
 *    `scroll-padding-top` reads so an anchored heading is not left under it.
 * 2. For `hide`: `data-hidden` on the header while the reader scrolls down
 *    past it, removed on any scroll up.
 *
 * It keeps its own scroll tracker rather than sharing the island's: that one
 * is private to `initNavIsland`, runs only on a page that carries an island,
 * and is entangled with its scrollspy. Where both exist the island's bar is
 * hidden by the stylesheet, so only one of them is ever on screen.
 */

/** Ignore scroll movement of this many pixels or fewer. */
const TOLERANCE = 5;

export function initHeaderScroll(): void {
  const mode = document.body.dataset.headerScroll;
  if (mode !== "sticky" && mode !== "hide") return;
  const header = document.querySelector<HTMLElement>("body > header");
  if (!header) return;

  const root = document.documentElement;
  const measure = (): void => {
    root.style.setProperty("--moss-header-height", `${header.offsetHeight}px`);
  };
  measure();
  if (typeof ResizeObserver !== "undefined") new ResizeObserver(measure).observe(header);
  if (mode !== "hide") return;

  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
  let lastY = window.scrollY;

  const show = (): void => header.removeAttribute("data-hidden");
  const mustStay = (): boolean =>
    reduced.matches ||
    header.contains(document.activeElement) ||
    header.querySelector(".mobile-open") !== null;

  const onScroll = (): void => {
    const y = window.scrollY;
    if (y <= header.offsetHeight || mustStay()) {
      show();
    } else if (y < lastY - TOLERANCE) {
      show();
    } else if (y > lastY + TOLERANCE) {
      header.setAttribute("data-hidden", "");
    } else {
      return;
    }
    lastY = y;
  };

  window.addEventListener("scroll", onScroll, { passive: true });
  header.addEventListener("focusin", show);
  reduced.addEventListener("change", show);
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initHeaderScroll);
} else {
  initHeaderScroll();
}
