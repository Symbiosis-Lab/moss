/**
 * hint-place.ts — keep every hover hint on screen.
 *
 * The site's hint is a CSS pseudo-element pill on `[data-tooltip]` (site.css),
 * anchored by default to its host's start edge. That anchor is wrong somewhere
 * for every static choice: a start-anchored pill on a right-edge host runs off
 * the viewport, an end-anchored one on a left-edge host does the same in the
 * other direction, and a mid-bar host with a long hint — the island's `…`
 * button, whose hint is the whole folded trail — can crop either way. Two
 * one-off edge-flips had already accumulated before the third case arrived,
 * which is the tell that the anchor should never have been per-case.
 *
 * So: on hover or focus entry, measure the pill and the host once and set a
 * per-host offset (`--moss-hint-x`) that lands the pill inside the viewport,
 * clamped to a small gutter. The clamp itself lives in `../viewport` — this is
 * one of four surfaces that float over a page, and it is not the only one that
 * had to learn where the visible band ends. site.css caps the pill at the viewport's width
 * and lets it wrap, so even a hint longer than the screen stays readable.
 * The placement is recomputed on every entry and the pill only exists while
 * hovered or focused, so there is nothing to tear down and nothing to re-init
 * after a morph — this module holds no node references at all.
 *
 * With JS off, no hints exist at all: every `data-tooltip` is JS-promoted
 * (breadcrumb-hint.ts, breadcrumb-fold.ts), the island never renders, and the
 * nav toggles carry `aria-label` only. So whenever a hint can show, this
 * module has run — no static-anchor fallback survives to drift from it.
 */

import { clampLeft, maxSurfaceWidth } from "../viewport";

/**
 * Minimum clearance from either viewport edge, in px.
 *
 * Wider than `viewport.ts`'s shared default: a hint is a transient pill the
 * reader is not aiming at, so it can afford to sit further in than a preview
 * card or a popover the reader is about to click.
 */
const GUTTER = 12;

/** Measure and place one host's hint pill inside the viewport. */
function place(host: HTMLElement): void {
  // Cap the pill against the visible band before measuring it — the
  // stylesheet's `100vw` fallback cannot see the scrollbar. Setting this first
  // matters: it can change the width the very next line reads.
  host.style.setProperty("--moss-hint-max-w", `${maxSurfaceWidth(GUTTER)}px`);

  // The pill's box is always rendered (it hides via `opacity: 0`, never
  // `display: none`), and its max-width means the computed width already
  // accounts for wrapping — so this read is the pill's real on-screen width,
  // PROVIDED it is read as a border box. `getComputedStyle().width` resolves to
  // the *used* value, which is the CONTENT box whatever `box-sizing` says, so
  // it omits the pill's `padding: 4px 8px`. Clamping on that number placed the
  // pill 16px too far right and it cropped at the screen edge — the exact bug
  // this module exists to end, surviving inside the fix for it. There is no
  // element to call `getBoundingClientRect()` on here (a pseudo-element has no
  // node), so the padding and border have to be added back by hand.
  const pill = getComputedStyle(host, "::after");
  const px = (value: string): number => parseFloat(value) || 0;
  const pillWidth =
    px(pill.width) +
    px(pill.paddingLeft) +
    px(pill.paddingRight) +
    px(pill.borderLeftWidth) +
    px(pill.borderRightWidth);

  const hostLeft = host.getBoundingClientRect().left;
  const desiredLeft = clampLeft(hostLeft, pillWidth, GUTTER);

  host.style.setProperty("--moss-hint-x", `${desiredLeft - hostLeft}px`);
  host.setAttribute("data-hint-placed", "");
}

/**
 * Hosts currently carrying `data-hint-suppressed`.
 *
 * Held as a Set rather than re-queried, because `onMove` runs at pointer-move
 * frequency and would otherwise match an attribute selector against the whole
 * document 60–120 times a second to discover that nothing is suppressed —
 * which is the case essentially always.
 */
const suppressed = new Set<HTMLElement>();

function suppress(host: HTMLElement): void {
  host.setAttribute("data-hint-suppressed", "");
  suppressed.add(host);
}

function unsuppress(host: HTMLElement): void {
  host.removeAttribute("data-hint-suppressed");
  suppressed.delete(host);
}

function onEnter(event: Event): void {
  const target = event.target as Element | null;
  const host = target?.closest?.<HTMLElement>("[data-tooltip]");
  if (!host) return;
  // A hovering pointer has arrived from outside, so whatever a previous finger
  // or Escape did to this host is over. `pointerover` bubbles, so crossing
  // between two children of the same host fires this too — and that is motion
  // *within* the host, not an arrival, so it must not revive a dismissed pill.
  const from = (event as PointerEvent).relatedTarget as Node | null;
  if (!from || !host.contains(from)) unsuppress(host);
  place(host);
}

/**
 * A finger press suppresses this host's pill until a real pointer returns.
 *
 * site.css already gates the pill on `@media (any-hover: hover)`, which is the
 * right query — it keeps hints working on an iPad driving a trackpad, where
 * the narrower `(hover: hover)` would report `none` and silently drop them.
 * The cost of that choice is the mirror case: a touchscreen laptop passes
 * `any-hover` and can still strand a pill under a finger, because touch fires
 * `pointerover` and then never un-hovers. A media query can only describe the
 * pointers *attached*; `pointerType` is the only signal for the one actually
 * in use, so the last word belongs here rather than in CSS.
 */
function onPress(event: Event): void {
  if ((event as PointerEvent).pointerType !== "touch") return;
  const target = event.target as Element | null;
  const host = target?.closest?.<HTMLElement>("[data-tooltip]");
  if (host) suppress(host);
}

/**
 * Escape dismisses whatever hint is currently showing (WCAG 1.4.13
 * "dismissible") by reusing the same `data-hint-suppressed` attribute a
 * touch press already sets — the CSS rule that hides the pill on it exists
 * regardless of why it was set.
 *
 * Queries `:hover`/`:focus-visible` rather than tracking "the last placed
 * host": a hint can be showing from either hover or keyboard focus, and this
 * suppresses it however it got there.
 */
function onEscape(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  document
    .querySelectorAll<HTMLElement>("[data-tooltip]:hover, [data-tooltip]:focus-visible")
    .forEach(suppress);
}

/**
 * Release a suppressed host once the pointer has actually left it.
 *
 * `onEnter` handles the arrival case, but a host the pointer never left fires
 * no further `pointerover`, so something has to notice the departure. That is
 * this — and it has to be departure, not motion. Escape must dismiss the pill
 * for as long as the reader stays on the host (WCAG 1.4.13 "dismissible"), and
 * a mouse resting on a trackpad drifts a pixel constantly; keying the release
 * off movement alone would put the pill straight back and defeat the very
 * criterion the Escape handler exists for.
 *
 * Touch is excluded outright: `onPress` suppresses on `pointerdown`, and a tap
 * that slides even slightly emits `pointermove` from the same finger, which
 * would undo the suppression before the finger lifts.
 */
function onMove(event: PointerEvent): void {
  if (suppressed.size === 0) return;
  if (event.pointerType === "touch") return;
  for (const host of [...suppressed]) {
    // A morph can replace a suppressed host; drop it rather than hold the node.
    if (!host.isConnected || !host.matches(":hover, :focus-visible")) unsuppress(host);
  }
}

document.addEventListener("pointerover", onEnter, true);
document.addEventListener("focusin", onEnter, true);
document.addEventListener("pointerdown", onPress, true);
document.addEventListener("keydown", onEscape);
document.addEventListener("pointermove", onMove);
