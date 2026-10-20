/**
 * viewport.ts — the one place a floating surface is kept on screen.
 *
 * A moss page floats four small things over the text: a hover hint, a link
 * preview card, the selection popover, and the island's fold panels. Each one
 * is positioned from some anchor's `getBoundingClientRect()`, and each one can
 * therefore be asked to sit where it does not fit — a hint on a right-edge
 * button, a preview under a link at the end of a line, a popover centred on a
 * word in the left margin.
 *
 * Every one of those was found as its own bug and fixed on its own terms. By
 * the third fix there were three different clamps in the bundle at three
 * different levels of correctness: one measured the visible band properly, one
 * measured it including the scrollbar gutter, and one did not clamp at all, so
 * a selection near the left margin sent half the popover off the screen. The
 * clamp is not a per-component concern and stops being written per component
 * here.
 *
 * The arithmetic is deliberately dull. What is worth reading is `visibleWidth`.
 *
 * The same gutter decides `lockPageScroll`, which an overlay takes so the page
 * behind it neither scrolls nor shifts.
 */

/** Minimum clearance from any viewport edge, in px. */
export const GUTTER = 8;

/**
 * The width the reader can actually see.
 *
 * NOT `window.innerWidth`, and NOT `100vw`: both include the classic scrollbar
 * gutter, so on a desktop engine that reserves one they over-report the visible
 * band by its width and every clamp derived from them lands that far too far
 * right. `documentElement.clientWidth` is the band itself. (The desktop app's
 * editor portal tooltip reached the same conclusion independently, in its own
 * `viewportWidth` note.)
 */
export function visibleWidth(): number {
  return document.documentElement?.clientWidth || window.innerWidth;
}

/** The height the reader can actually see — `clientHeight` for the same reason. */
export function visibleHeight(): number {
  return document.documentElement?.clientHeight || window.innerHeight;
}

/**
 * The widest a surface may be and still clear both gutters.
 *
 * Worth setting as a `max-width` *before* measuring the surface, not after: a
 * surface that wraps is narrower than one that does not, so capping first can
 * change the very width the clamp below is about to read.
 */
export function maxSurfaceWidth(gutter: number = GUTTER): number {
  return Math.max(0, visibleWidth() - gutter * 2);
}

/**
 * The left edge to actually use for a `width`-wide surface that would like to
 * start at `desiredLeft`.
 *
 * When the surface is wider than the band, the left gutter wins — starting at
 * the left edge and overflowing right is readable in every writing direction
 * moss ships, while the other way round loses the beginning of the text.
 */
export function clampLeft(
  desiredLeft: number,
  width: number,
  gutter: number = GUTTER
): number {
  const maxLeft = Math.max(gutter, visibleWidth() - gutter - width);
  return Math.min(Math.max(gutter, desiredLeft), maxLeft);
}

/** Where a surface anchored to a rect should sit vertically. */
export interface VerticalSlot {
  /** The top edge to use, in viewport coordinates. */
  top: number;
  /** True when the surface ended up on the side it did not ask for. */
  flipped: boolean;
}

/**
 * Stack a `height`-tall surface against `rect`, on the preferred side when it
 * fits and the other side when it does not.
 *
 * Both sides are tried before either is clamped, because a clamped surface
 * overlaps its own anchor — which for a selection popover means covering the
 * words the reader just highlighted. Only when neither side fits (a viewport
 * shorter than the surface) does the result get pinned to the top gutter.
 */
export function verticalSlot(
  rect: { top: number; bottom: number },
  height: number,
  prefer: "above" | "below",
  gap: number = GUTTER,
  gutter: number = GUTTER
): VerticalSlot {
  const above = rect.top - height - gap;
  const below = rect.bottom + gap;
  const limit = visibleHeight();

  const aboveFits = above >= gutter;
  const belowFits = below + height <= limit - gutter;

  if (prefer === "above") {
    if (aboveFits) return { top: above, flipped: false };
    if (belowFits) return { top: below, flipped: true };
  } else {
    if (belowFits) return { top: below, flipped: false };
    if (aboveFits) return { top: above, flipped: true };
  }

  return { top: Math.max(gutter, Math.min(above, limit - gutter - height)), flipped: false };
}

/**
 * Stop the page behind an overlay from scrolling, without moving it.
 *
 * Hiding the root's overflow also hides a classic scrollbar (Windows, Linux,
 * macOS set to always show scrollbars), and the page then widens by the
 * scrollbar's width and shifts sideways under the overlay. While locked, the
 * root keeps the scrollbar's gutter with `scrollbar-gutter: stable` — but only
 * when the page had a scrollbar to begin with, or a page that never scrolled
 * would gain an empty gutter and shift the other way. A scrollbar the page
 * styles itself gets only the platform's gutter, which is none where the
 * platform overlays its scrollbars, so whatever width the root gains is padded
 * back. The root's own width says that, not `clientWidth`: once overflow is
 * hidden, `clientWidth` no longer subtracts a gutter that is still kept.
 *
 * Holds are keyed by owner and recorded in one object on `window`, because
 * the scripts that lock the page (menu, search, picture viewer) ship as
 * separate bundles, each with its own copy of this module. The page unlocks
 * when the last owner lets go, in any order; holding twice, or releasing an
 * owner that holds nothing, changes nothing.
 */
interface PageScrollState {
  owners: Set<string>;
  saved: { overflow: string; gutter: string; padding: string };
}

function pageScroll(): PageScrollState {
  const w = window as unknown as { __mossPageScroll?: PageScrollState };
  return (w.__mossPageScroll ??= { owners: new Set(), saved: { overflow: "", gutter: "", padding: "" } });
}

export function holdPageScroll(owner: string, on: boolean): void {
  const state = pageScroll();
  if (state.owners.has(owner) === on) return;
  const root = document.documentElement;
  if (on) {
    if (state.owners.size === 0) {
      state.saved = { overflow: root.style.overflow, gutter: root.style.scrollbarGutter, padding: root.style.paddingRight };
      const width = root.getBoundingClientRect().width;
      if (window.innerWidth - root.clientWidth > 0) root.style.scrollbarGutter = "stable";
      root.style.overflow = "hidden";
      const lost = root.getBoundingClientRect().width - width;
      if (lost > 0) root.style.paddingRight = `${(parseFloat(getComputedStyle(root).paddingRight) || 0) + lost}px`;
    }
    state.owners.add(owner);
  } else {
    state.owners.delete(owner);
    if (state.owners.size === 0) {
      root.style.overflow = state.saved.overflow;
      root.style.scrollbarGutter = state.saved.gutter;
      root.style.paddingRight = state.saved.padding;
    }
  }
}
