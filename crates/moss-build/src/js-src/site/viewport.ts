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
