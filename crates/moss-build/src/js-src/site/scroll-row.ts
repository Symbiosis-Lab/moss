/**
 * scroll-row.ts — position dots and mouse-wheel scrolling for a
 * `:::grid N {scroll}` row.
 *
 * Up to ten cards get one dot each. Longer rows keep one stable dot per card
 * inside a clipped seven-slot viewport. The track slides as the active card
 * changes, while smaller edge dots signal that cards continue beyond the
 * visible window. Keeping each dot tied to one card avoids the disorienting
 * identity swap of repainting seven fixed buttons with new card numbers.
 * Dragging the indicator scrubs directly through the cards; discrete clicks
 * and keys retain smooth movement. The
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
import { isVertical, pageLanguage, type SiteLanguage } from "./writing-mode";

export {};

const ROW = ".moss-grid[data-scroll]";
const NAV = "moss-scroll-dots";
/** Up to this many cards get one dot each; a longer row gets a counter. */
const MAX_DOT_COUNT = 7;

/** "Previous" and "Next" in each site language, for the counter's buttons.
 * A Rust test (`scroll_row_labels_match_the_site_strings`) holds them to
 * `i18n/strings.rs`, the site's own copy. */
const LABELS: Record<SiteLanguage, { previous: string; next: string }> = {
  en: { previous: "Previous", next: "Next" },
  de: { previous: "Zurück", next: "Weiter" },
  "zh-hans": { previous: "上一个", next: "下一个" },
  "zh-hant": { previous: "上一個", next: "下一個" },
};

interface RowState {
  nav: HTMLElement | null;
  io: IntersectionObserver | null;
  cancelNavigation: (() => void) | null;
  /** Repaints the current indicator after the row scrolls. */
  onScroll: (() => void) | null;
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
    state = { nav: null, io: null, cancelNavigation: null, onScroll: null };
    rows.set(row, state);
    let frame = 0;
    row.addEventListener("scroll", () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        state!.onScroll?.();
      });
    }, { passive: true });
    row.addEventListener("wheel", (e) => {
      state!.cancelNavigation?.();
      onWheel(row, e);
    }, { passive: false });
    row.addEventListener("pointerdown", () => state!.cancelNavigation?.(), { passive: true });
    row.addEventListener("keydown", () => state!.cancelNavigation?.());
    if (typeof ResizeObserver === "function") {
      new ResizeObserver(() => fit(row, state!)).observe(row);
    }
  }
  buildIndicator(row, state);
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

function navigationStep(row: HTMLElement, key: string): -1 | 1 | null {
  if (isVertical(row)) {
    if (key === "ArrowDown") return 1;
    if (key === "ArrowUp") return -1;
    return null;
  }
  const rtl = getComputedStyle(row).direction === "rtl";
  if (key === "ArrowRight") return rtl ? -1 : 1;
  if (key === "ArrowLeft") return rtl ? 1 : -1;
  return null;
}

/** (Re)builds the indicator for `row` from its current children. Idempotent: a
 * rebuild first tears down whatever the last one made — the observer it
 * pointed at the old cards, and the nav element itself, which a morph may
 * already have removed — so it is safe to call on every init, not just the
 * first. */
function buildIndicator(row: HTMLElement, state: RowState): void {
  state.io?.disconnect();
  state.nav?.remove();
  state.nav = null;
  state.io = null;
  state.cancelNavigation = null;
  state.onScroll = null;

  const cards = Array.from(row.children) as HTMLElement[];
  if (cards.length < 2) return;

  const nav = document.createElement("div");
  nav.className = NAV;
  nav.setAttribute("role", "group");
  const label = row.getAttribute("aria-label");
  if (label) nav.setAttribute("aria-label", label);
  nav.dataset.count = String(cards.length);
  const reduce = prefersReducedMotion();

  const inView = new Set<number>();
  let activeIndex = 0;
  // `lands` is the card the count will show once the row comes to rest: the
  // last card when the target is within one view of the end. Decided at the
  // press, so a frame of the glide with fewer cards in view cannot change it.
  let requested: { index: number; reached: boolean; lands: number } | null = null;
  const navigateTo = (index: number, behavior: ScrollBehavior = reduce ? "auto" : "smooth"): number => {
    const card = cards[index];
    if (!card) return activeIndex;
    const end = cards.length - 1;
    const lands = index >= cards.length - range().span ? end : index;
    activeIndex = index;
    requested = { index, reached: inView.has(index), lands };
    card.scrollIntoView({ block: "nearest", inline: "start", behavior });
    paint(index);
    return lands;
  };

  /** Where the row is: its first card (a requested card while a navigation
   * to it is under way), its last card in view, and how many cards one view
   * holds, counted from the cards actually in view. */
  const range = (): { first: number; last: number; span: number } => {
    const first = requested?.index ?? (inView.size ? Math.min(...inView) : activeIndex);
    const span = inView.size ? Math.max(...inView) - Math.min(...inView) + 1 : 1;
    return { first, last: Math.max(first, inView.size ? Math.max(...inView) : first), span };
  };
  const view = cards.length > MAX_DOT_COUNT
    ? buildCounter(row, nav, cards, navigateTo, range)
    : buildDots(row, nav, cards, navigateTo, () => activeIndex);
  row.after(nav);
  state.nav = nav;

  let reported = "";
  const paint = (requestedIndex?: number) => {
    const { first: seen, last } = range();
    const first = requestedIndex ?? seen;
    activeIndex = first;
    // The current card is the first in view, except once the last card is in
    // view: the reader has reached the end. Scroll snapping can rest the row
    // a few pixels short of its scroll limit, so the end is what is in view.
    // While a navigation is under way it is where the row will come to rest.
    const end = cards.length - 1;
    const current = requested ? requested.lands : inView.has(end) ? end : first;
    const position = { current: current + 1, first: first + 1, last: Math.max(first, last) + 1, count: cards.length };
    Object.assign(nav.dataset, { current: position.current, first: position.first, last: position.last });
    view.paint(first, current, inView);
    const key = `${position.current}|${position.first}|${position.last}`;
    if (key !== reported) {
      reported = key;
      row.dispatchEvent(new CustomEvent("moss-scroll-position", { bubbles: true, detail: position }));
    }
  };
  state.cancelNavigation = () => {
    if (!requested) return;
    requested = null;
    paint();
  };
  state.onScroll = () => paint();
  paint();
  if (typeof IntersectionObserver === "function") {
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const i = cards.indexOf(e.target as HTMLElement);
          if (e.isIntersecting) inView.add(i);
          else inView.delete(i);
        }
        if (requested) {
          if (inView.has(requested.index)) requested.reached = true;
          else if (requested.reached) requested = null;
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

/** What one kind of indicator draws for a position. */
interface IndicatorView {
  paint(first: number, current: number, inView: ReadonlySet<number>): void;
}

/** One dot per card, for a short row: the dots in view are lit and the first
 * of them is `aria-current`. One roving tab stop; dragging across the dots
 * scrubs through the cards. */
function buildDots(
  row: HTMLElement,
  nav: HTMLElement,
  cards: HTMLElement[],
  navigateTo: (index: number, behavior?: ScrollBehavior) => void,
  activeIndex: () => number,
): IndicatorView {
  nav.dataset.indicator = "dots";
  const viewport = document.createElement("div");
  viewport.className = `${NAV}-viewport`;
  nav.appendChild(viewport);
  const dots = cards.map((_, index) => {
    const dot = document.createElement("button");
    dot.type = "button";
    dot.dataset.cardIndex = String(index);
    dot.setAttribute("aria-label", `${index + 1} / ${cards.length}`);
    dot.addEventListener("click", () => navigateTo(index));
    dot.addEventListener("keydown", (e) => {
      const step = navigationStep(row, e.key);
      if (step === null && e.key !== "Home" && e.key !== "End") return;
      e.preventDefault();
      const next =
        e.key === "Home" ? 0 : e.key === "End" ? cards.length - 1 : Math.max(0, Math.min(cards.length - 1, index + (step ?? 0)));
      navigateTo(next);
      dots[next]?.focus();
    });
    viewport.appendChild(dot);
    return dot;
  });

  // UIPageControl-style continuous interaction: wait for a real drag so a
  // normal button click stays discrete, then scrub by horizontal distance.
  // Direct updates intentionally use `auto`; animating every crossed card
  // makes the content lag behind the pointer and flashes intermediate pages.
  let scrub: { pointerId: number; x: number; index: number; active: boolean } | null = null;
  let suppressClick = false;
  nav.addEventListener("click", (event) => {
    if (!suppressClick) return;
    event.preventDefault();
    event.stopPropagation();
  }, { capture: true });
  viewport.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    scrub = { pointerId: event.pointerId, x: event.clientX, index: activeIndex(), active: false };
  });
  viewport.addEventListener("pointermove", (event) => {
    if (!scrub || event.pointerId !== scrub.pointerId) return;
    const delta = event.clientX - scrub.x;
    if (!scrub.active && Math.abs(delta) < 6) return;
    if (!scrub.active) {
      scrub.active = true;
      nav.dataset.scrubbing = "";
      viewport.setPointerCapture?.(event.pointerId);
    }
    event.preventDefault();
    const width = viewport.getBoundingClientRect().width || cards.length * 24;
    const step = Math.round((delta / width) * (cards.length - 1));
    const index = Math.max(0, Math.min(cards.length - 1, scrub.index + step));
    if (index !== activeIndex()) navigateTo(index, "auto");
  });
  const endScrub = (event: PointerEvent) => {
    if (!scrub || event.pointerId !== scrub.pointerId) return;
    const wasActive = scrub.active;
    if (viewport.hasPointerCapture?.(event.pointerId)) viewport.releasePointerCapture(event.pointerId);
    scrub = null;
    delete nav.dataset.scrubbing;
    if (wasActive) {
      suppressClick = true;
      window.setTimeout(() => { suppressClick = false; }, 0);
    }
  };
  viewport.addEventListener("pointerup", endScrub);
  viewport.addEventListener("pointercancel", endScrub);

  return {
    paint(first, _current, inView) {
      const keepFocus = nav.contains(document.activeElement);
      dots.forEach((dot, index) => {
        dot.tabIndex = index === first ? 0 : -1;
        dot.classList.toggle("is-visible", inView.has(index));
        if (index === first) dot.setAttribute("aria-current", "true");
        else dot.removeAttribute("aria-current");
      });
      if (keepFocus) dots[first]?.focus({ preventScroll: true });
    },
  };
}

/** A count and ‹ › buttons, for a row too long for dots: "26 / 55", the
 * current card's number. The count is sized once for its widest value, so it
 * never changes width as the reader scrolls. The buttons move one view; on a
 * right-to-left row they swap sides, and on a vertical row they point along
 * it. */
function buildCounter(
  row: HTMLElement,
  nav: HTMLElement,
  cards: HTMLElement[],
  navigateTo: (index: number) => number,
  range: () => { first: number; span: number },
): IndicatorView {
  nav.dataset.indicator = "count";
  if (isVertical(row)) nav.dataset.axis = "block";
  const rtl = getComputedStyle(row).direction === "rtl";
  if (rtl) nav.dir = "rtl";
  const words = LABELS[pageLanguage()];
  const button = (cls: string, text: string, name: string): HTMLButtonElement => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = cls;
    b.textContent = text;
    b.setAttribute("aria-label", name);
    return b;
  };
  const prev = button(`${NAV}-prev`, rtl ? "›" : "‹", words.previous);
  const next = button(`${NAV}-next`, rtl ? "‹" : "›", words.next);
  const count = document.createElement("span");
  count.className = `${NAV}-count`;
  const now = document.createElement("span");
  const widest = document.createElement("span");
  widest.className = `${NAV}-count-size`;
  widest.setAttribute("aria-hidden", "true");
  widest.textContent = `${cards.length} / ${cards.length}`;
  count.append(now, widest);
  const spoken = document.createElement("span");
  spoken.className = "visually-hidden";
  spoken.setAttribute("aria-live", "polite");
  nav.append(prev, count, next, spoken);

  const nameOf = (card: HTMLElement): string =>
    card.getAttribute("aria-label") ||
    card.querySelector(".moss-card-title, h1, h2, h3, h4, h5, h6")?.textContent?.trim() ||
    "";
  const step = (direction: -1 | 1): void => {
    const { first, span } = range();
    const target = Math.max(0, Math.min(cards.length - 1, first + direction * span));
    const shown = navigateTo(target);
    const name = nameOf(cards[shown]);
    // Cleared first, so pressing again at the same card is announced again.
    spoken.textContent = "";
    requestAnimationFrame(() => {
      spoken.textContent = `${name ? `${name}, ` : ""}${shown + 1} / ${cards.length}`;
    });
  };
  prev.addEventListener("click", () => {
    if (prev.getAttribute("aria-disabled") !== "true") step(-1);
  });
  next.addEventListener("click", () => {
    if (next.getAttribute("aria-disabled") !== "true") step(1);
  });

  return {
    paint(first, current) {
      now.textContent = `${current + 1} / ${cards.length}`;
      prev.setAttribute("aria-disabled", String(first === 0));
      next.setAttribute("aria-disabled", String(current === cards.length - 1));
    },
  };
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
