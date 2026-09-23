/**
 * scroll-row.ts — position dots and mouse-wheel scrolling for a
 * `:::grid N {scroll}` row.
 *
 * One dot per card. The cards currently in view are lit and the first of
 * them is `aria-current`, so the dots show how many cards there are, where
 * the reader is, and how much of the row is visible at once. Clicking a dot
 * brings that card to the start of the row.
 *
 * Under horizontal typesetting the row scrolls sideways (its inline axis is
 * physical left-right); under vertical typesetting (`writing-mode:
 * vertical-rl`) the same row scrolls DOWN the line instead — its inline axis
 * runs top-to-bottom there — so "the row's scroll axis" is never assumed to
 * be a fixed physical direction. `isScrollableRow` below is the one place
 * that reads the actual axis (via `isVertical`) and checks BOTH that the
 * matching `overflow-x`/`overflow-y` is `auto`/`scroll` and that the box
 * actually overflows on it; the dots' visibility and the wheel handler both
 * go through it, so a row that merely LOOKS overflowed — CSS Overflow §3
 * forces the cross axis's computed overflow to stop being `visible` too,
 * once the scroll axis is non-`visible`, which can make its size comparison
 * true on an axis the row was never meant to scroll on — never gets treated
 * as scrollable on the wrong axis.
 *
 * A mouse wheel over a HORIZONTAL row scrolls it sideways here in JS: a
 * vertical wheel would otherwise scroll the page, and most readers never
 * learn Shift+wheel. Over a VERTICAL row a vertical wheel already scrolls it
 * natively — it is an ordinary `overflow-y: auto` box — so `onWheel` does
 * nothing to the scroll itself there; its only job is `stopPropagation()`,
 * so theme.ts's page-wide "remap wheel to horizontal scroll" listener
 * (bound on `document` for vertical typesetting) doesn't call
 * `preventDefault()` on the same event first and cancel the row's own
 * native scroll before it runs — a wheel event has one default action for
 * its whole dispatch, decided after every listener has run, so a later
 * ancestor listener can still veto an earlier target's native scroll. In
 * both axes the wheel is taken only while the row can still move that way,
 * so at either end it passes through and the page scrolls on — the row
 * never traps the reader. A trackpad's own sideways gesture, pinch-zoom
 * (ctrlKey) and touch are left to the browser everywhere.
 *
 * Progressive enhancement: the row scrolls without this script, and the dots
 * exist only once it runs. A row with nothing to scroll (all cards fit) keeps
 * its dots hidden.
 *
 * A preview morph reuses the `.moss-grid[data-scroll]` node itself but drops
 * the dots, which are not in the source HTML idiomorph reconciles against —
 * so `moss-morph-patched` runs init again on a row it has already seen. The
 * row's long-lived bindings (the wheel listener, the resize watch) are wired
 * once per row node, tracked in `rows` below the way `theme.ts`'s
 * `pill.dataset.initialized` tracks its own one-time bind; only the dots
 * themselves — and the intersection watch over them, which must point at
 * whichever cards exist now, not a stale array from the first attach — are
 * rebuilt every time, since an edit can change the card count.
 */
import { isVertical } from "./writing-mode";

export {};

const ROW = ".moss-grid[data-scroll]";
const DOTS = "moss-scroll-dots";

interface RowState {
  nav: HTMLElement | null;
  io: IntersectionObserver | null;
}

const rows = new WeakMap<HTMLElement, RowState>();

export function initScrollDots(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>(ROW).forEach(attach);
}

function attach(row: HTMLElement): void {
  let state = rows.get(row);
  if (!state) {
    state = { nav: null, io: null };
    rows.set(row, state);
    row.addEventListener("wheel", (e) => onWheel(row, e), { passive: false });
    if (typeof ResizeObserver === "function") {
      new ResizeObserver(() => fit(row, state!)).observe(row);
    }
  }
  buildDots(row, state);
}

/** See the file header: the one place that decides whether `row` is an
 * actual scroll container, on whichever axis it actually scrolls on. */
function isScrollableRow(row: HTMLElement): boolean {
  const cs = getComputedStyle(row);
  if (isVertical(row)) {
    if (cs.overflowY !== "auto" && cs.overflowY !== "scroll") return false;
    return row.scrollHeight > row.clientHeight + 1;
  }
  if (cs.overflowX !== "auto" && cs.overflowX !== "scroll") return false;
  return row.scrollWidth > row.clientWidth + 1;
}

function fit(row: HTMLElement, state: RowState): void {
  if (state.nav) state.nav.hidden = !isScrollableRow(row);
}

/** (Re)builds the dots for `row` from its current children. Idempotent: a
 * rebuild first tears down whatever the last one made — the observer it
 * pointed at the old cards, and the nav element itself, which a morph may
 * already have removed — so it is safe to call on every init, not just the
 * first. */
function buildDots(row: HTMLElement, state: RowState): void {
  state.io?.disconnect();
  state.nav?.remove();
  state.nav = null;
  state.io = null;

  const cards = Array.from(row.children) as HTMLElement[];
  if (cards.length < 2) return;

  const nav = document.createElement("div");
  nav.className = DOTS;
  nav.setAttribute("role", "group");
  const label = row.getAttribute("aria-label");
  if (label) nav.setAttribute("aria-label", label);

  const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
  const dots = cards.map((card, i) => {
    const dot = document.createElement("button");
    dot.type = "button";
    // Language-neutral on purpose: a site in any language reads "3 / 8".
    dot.setAttribute("aria-label", `${i + 1} / ${cards.length}`);
    dot.addEventListener("click", () =>
      card.scrollIntoView({ block: "nearest", inline: "start", behavior: reduce ? "auto" : "smooth" }),
    );
    nav.appendChild(dot);
    return dot;
  });
  row.after(nav);
  state.nav = nav;

  const inView = new Set<number>();
  const paint = () => {
    const first = inView.size ? Math.min(...inView) : -1;
    dots.forEach((dot, i) => {
      dot.classList.toggle("is-visible", inView.has(i));
      if (i === first) dot.setAttribute("aria-current", "true");
      else dot.removeAttribute("aria-current");
    });
  };
  if (typeof IntersectionObserver === "function") {
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const i = cards.indexOf(e.target as HTMLElement);
          if (e.isIntersecting) inView.add(i);
          else inView.delete(i);
        }
        paint();
      },
      { root: row, threshold: 0.6 },
    );
    cards.forEach((c) => io.observe(c));
    state.io = io;
  }

  fit(row, state);
}

/** Pixels a wheel event asks to move, whatever unit it was reported in. */
function wheelPixels(e: WheelEvent, row: HTMLElement): number {
  if (e.deltaMode === 1) return e.deltaY * 16; // lines
  if (e.deltaMode === 2) return e.deltaY * row.clientWidth; // pages
  return e.deltaY;
}

export function onWheel(row: HTMLElement, e: WheelEvent): void {
  if (e.ctrlKey || Math.abs(e.deltaX) >= Math.abs(e.deltaY)) return;
  if (!isScrollableRow(row)) return;
  if (isVertical(row)) {
    const max = row.scrollHeight - row.clientHeight;
    const delta = wheelPixels(e, row);
    if ((delta < 0 && row.scrollTop <= 0) || (delta > 0 && row.scrollTop >= max - 1)) return;
    // No preventDefault, no scrollTop write: the browser already does this
    // natively. Only keep it from also reaching theme.ts's document-level
    // handler (see file header).
    e.stopPropagation();
    return;
  }
  // RTL rows report a negative scrollLeft; mirror the wheel so "down" still
  // moves toward the row's end.
  const rtl = getComputedStyle(row).direction === "rtl";
  const max = row.scrollWidth - row.clientWidth;
  const pos = Math.abs(row.scrollLeft);
  const delta = wheelPixels(e, row);
  if ((delta < 0 && pos <= 0) || (delta > 0 && pos >= max - 1)) return;
  e.preventDefault();
  row.scrollLeft += rtl ? -delta : delta;
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => initScrollDots());
} else {
  initScrollDots();
}
document.addEventListener("moss-morph-patched", () => initScrollDots());
