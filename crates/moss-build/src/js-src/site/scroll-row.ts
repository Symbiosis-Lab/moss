/**
 * scroll-row.ts — position dots and mouse-wheel scrolling for a
 * `:::grid N {scroll}` row.
 *
 * Up to ten cards get one dot each. Longer rows use a seven-slot moving window
 * in the style of Apple's page controls: the active dot is prominent and the
 * smaller edge dots signal that cards continue beyond the visible window. The
 * control has one roving tab stop, so it never adds one tab stop per card.
 * Cards currently in view are lit and the first of them is
 * `aria-current`. Clicking a dot brings that card to the start of the row.
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
 * learn Shift+wheel. The redirect only ever changes which axis the wheel
 * moves — the motion itself is the browser's own smooth `scrollTo`, aimed at
 * a running target rather than the row's live `scrollLeft`, so native
 * `scroll-snap` still settles the row on a card edge instead of a script
 * fighting it frame by frame. The running target, not `scrollLeft`, is what
 * a second notch adds to: every engine tested (Chromium, Firefox, WebKit)
 * reports `scrollLeft` mid-animation as a stale, not-yet-caught-up value, so
 * a fast flick that adds each notch to that live value loses most of its
 * distance — measured at 22-32% of the intended travel for 5 rapid notches
 * where a running target lands within 1px. Over a VERTICAL row a vertical
 * wheel already scrolls it natively — it is an ordinary `overflow-y: auto`
 * box — so `onWheel` does nothing to the scroll itself there; its only job
 * is `stopPropagation()`, so theme.ts's page-wide "remap wheel to
 * horizontal scroll" listener (bound on `document` for vertical typesetting)
 * doesn't call `preventDefault()` on the same event first and cancel the
 * row's own native scroll before it runs — a wheel event has one default
 * action for its whole dispatch, decided after every listener has run, so a
 * later ancestor listener can still veto an earlier target's native scroll.
 * In both axes the wheel is taken only while the row can still move that
 * way, so at either end it passes through and the page scrolls on — the row
 * never traps the reader. A trackpad's own sideways gesture, pinch-zoom
 * (ctrlKey) and touch are left to the browser everywhere.
 *
 * A row also only ever takes a wheel gesture that STARTS over it. A gesture
 * already scrolling the page doesn't stop doing that just because the
 * cursor drifts over a row mid-flick — if it did, the row would trap an
 * ordinary page scroll the moment it passed under the pointer. `trackRow`,
 * a passive capture listener on `window`, watches every wheel event on the
 * page (not just the ones over a row) and records which element — a row or
 * nothing — the current gesture belongs to, resetting that record whenever
 * a wheel arrives more than `GESTURE_GAP_MS` after the last one seen
 * anywhere. `onWheel` only acts when that record names its own row.
 *
 * Progressive enhancement: the row scrolls without this script, and the dots
 * exist only once it runs. A row with nothing to scroll at the CURRENT
 * viewport — either because it never overflows (`data-fits`, on a wide
 * enough screen) or, in principle, any other row that stops overflowing —
 * keeps its dots hidden and, since `fit()` below toggles both off the same
 * check, is also pulled out of the tab order, so a `{scroll}` row that fits
 * its columns is never a pointless keyboard stop on a screen wide enough to
 * show every card already.
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
const MAX_DOT_COUNT = 10;
const DYNAMIC_DOT_COUNT = 7;

interface RowState {
  nav: HTMLElement | null;
  io: IntersectionObserver | null;
}

const rows = new WeakMap<HTMLElement, RowState>();

/** Below this many milliseconds since the last wheel event seen anywhere,
 * a new one still belongs to the same gesture — see the file header. */
const GESTURE_GAP_MS = 200;

/** The row (or `null` for "the page, or no row") the current wheel gesture
 * belongs to, and when it was last extended. Written only by `trackRow`. */
let gestureRow: HTMLElement | null = null;
let gestureAt = -Infinity;

function trackRow(e: Event): void {
  const now = performance.now();
  if (now - gestureAt > GESTURE_GAP_MS) {
    const target = e.target;
    gestureRow = target instanceof Element ? target.closest<HTMLElement>(ROW) : null;
  }
  gestureAt = now;
}
window.addEventListener("wheel", trackRow, { capture: true, passive: true });

/** Test-only: a unit test that dispatches real wheel events to exercise
 * `trackRow` would otherwise leak gesture state into whichever test runs
 * next. Never called from production code. */
export function __resetWheelGestureForTests(): void {
  gestureRow = null;
  gestureAt = -Infinity;
}

/** Per-row running scroll target for the smooth glide (see file header):
 * successive wheel notches within one gesture add to this rather than to
 * the row's live `scrollLeft`, which every tested engine reports behind the
 * actual animation target while it's still moving. Reset once the row
 * itself has gone `GESTURE_GAP_MS` without a wheel event. */
const scrollTargets = new WeakMap<HTMLElement, { left: number; at: number }>();

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

/** A `{scroll}` row whose cards fit its column count (`data-fits`) ships
 * `tabindex="0"` in the HTML so a no-JS view stays keyboard-scrollable at a
 * narrow width, but on a wide screen it isn't a scroll container at all —
 * only the runtime knows which side of the breakpoint the reader is on, so
 * this toggles the tab stop together with the dots, off the same
 * `isScrollableRow` check, on both init and resize. A row that always
 * scrolls (no `data-fits`) is scrollable at every width `isScrollableRow`
 * can observe, so this is a no-op for it — it keeps `tabindex="0"`. */
function fit(row: HTMLElement, state: RowState): void {
  const scrollable = isScrollableRow(row);
  if (state.nav) state.nav.hidden = !scrollable;
  row.tabIndex = scrollable ? 0 : -1;
}

function prefersReducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

/** (Re)builds the indicator for `row` from its current children. Idempotent: a
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
  const dynamicMode = cards.length > MAX_DOT_COUNT;
  nav.dataset.indicator = dynamicMode ? "dynamic" : "dots";
  nav.setAttribute("role", "group");
  const label = row.getAttribute("aria-label");
  if (label) nav.setAttribute("aria-label", label);

  const reduce = prefersReducedMotion();
  const dotCount = dynamicMode ? Math.min(DYNAMIC_DOT_COUNT, cards.length) : cards.length;
  const dots = Array.from({ length: dotCount }, () => {
    const dot = document.createElement("button");
    dot.type = "button";
    dot.addEventListener("click", () => {
      const index = Number(dot.dataset.cardIndex);
      navigateTo(index);
    });
    dot.addEventListener("keydown", (e) => {
      const navigationKey =
        e.key === "ArrowRight" ||
        e.key === "ArrowDown" ||
        e.key === "ArrowLeft" ||
        e.key === "ArrowUp" ||
        e.key === "Home" ||
        e.key === "End";
      if (!navigationKey) return;
      e.preventDefault();
      const current = Number(dot.dataset.cardIndex);
      const next =
        e.key === "Home"
          ? 0
          : e.key === "End"
            ? cards.length - 1
            : e.key === "ArrowRight" || e.key === "ArrowDown"
              ? Math.min(cards.length - 1, current + 1)
              : Math.max(0, current - 1);
      if (cards[next]) {
        navigateTo(next);
        dots.find((candidate) => candidate.dataset.cardIndex === String(next))?.focus();
      }
    });
    nav.appendChild(dot);
    return dot;
  });
  row.after(nav);
  state.nav = nav;

  const inView = new Set<number>();
  let activeIndex = 0;
  const windowStart = (index: number): number => {
    if (!dynamicMode) return 0;
    const half = Math.floor(dotCount / 2);
    return Math.max(0, Math.min(index - half, cards.length - dotCount));
  };
  const navigateTo = (index: number): void => {
    const card = cards[index];
    if (!card) return;
    activeIndex = index;
    card.scrollIntoView({ block: "nearest", inline: "start", behavior: reduce ? "auto" : "smooth" });
    paint(index);
  };
  const paint = (requestedIndex?: number) => {
    const keepIndicatorFocus = nav.contains(document.activeElement);
    const first = requestedIndex ?? (inView.size ? Math.min(...inView) : activeIndex);
    activeIndex = first;
    const start = windowStart(first);
    dots.forEach((dot, slot) => {
      const index = start + slot;
      dot.dataset.cardIndex = String(index);
      // Numeric labels stay meaningful without imposing an interface language.
      dot.setAttribute("aria-label", `${index + 1} / ${cards.length}`);
      dot.tabIndex = index === first ? 0 : -1;
      dot.classList.toggle("is-visible", inView.has(index));
      dot.classList.toggle("is-edge-start", dynamicMode && start > 0 && slot === 0);
      dot.classList.toggle("is-edge-end", dynamicMode && start + dotCount < cards.length && slot === dotCount - 1);
      if (index === first) dot.setAttribute("aria-current", "true");
      else dot.removeAttribute("aria-current");
    });
    if (keepIndicatorFocus) {
      dots.find((dot) => dot.dataset.cardIndex === String(first))?.focus({ preventScroll: true });
    }
  };
  paint();
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
  // A gesture already scrolling the page (or a different row) doesn't
  // become this row's just because the cursor drifts over it — see the
  // file header's `trackRow` paragraph. `gestureAt` only advances on a real
  // dispatch, so a unit test that calls `onWheel` directly without ever
  // dispatching through `window` always reads as stale here, i.e. owned.
  const stale = performance.now() - gestureAt > GESTURE_GAP_MS;
  if (!stale && gestureRow !== row) return;
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
  const delta = wheelPixels(e, row);
  const now = performance.now();
  const prior = scrollTargets.get(row);
  // `base` is this row's own running target, not `row.scrollLeft`: see the
  // file header for why reading the live value mid-glide silently drops
  // most of a fast flick's distance. A row this notch hasn't touched
  // recently starts fresh from wherever it actually is.
  const base = prior && now - prior.at <= GESTURE_GAP_MS ? prior.left : row.scrollLeft;
  const pos = Math.abs(base);
  if ((delta < 0 && pos <= 0) || (delta > 0 && pos >= max - 1)) {
    scrollTargets.delete(row);
    return;
  }
  // Clamp the target itself, not just the boundary check above: a single
  // large notch (fast wheel, OS scroll acceleration) can add more than the
  // remaining distance in one step. An unclamped target would overshoot
  // past `max`, and a reversed notch right after would have to "unwind"
  // that overshoot before the row visibly moved at all.
  const uncappedAbs = Math.min(max, Math.max(0, pos + delta));
  const left = rtl ? -uncappedAbs : uncappedAbs;
  scrollTargets.set(row, { left, at: now });
  e.preventDefault();
  row.scrollTo({ left, behavior: prefersReducedMotion() ? "auto" : "smooth" });
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => initScrollDots());
} else {
  initScrollDots();
}
document.addEventListener("moss-morph-patched", () => initScrollDots());
