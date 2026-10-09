/**
 * middle-truncate.ts — shorten a title by cutting out its middle.
 *
 * A heading's start says what the section is about and its end often says
 * which one it is ("… : part two", "… (2024)"), so a title that does not fit
 * keeps both and gives up the middle. An ordinary end-ellipsis would drop the
 * half that tells siblings apart.
 *
 * `truncateMiddle` is pure: the caller supplies `measure`, so the same function
 * serves the contents ruler and the sections button, and a test can pass any
 * width model it likes — jsdom has no canvas. `canvasMeasure` is the one
 * engine-facing helper, kept here so both callers measure the same way.
 */

/** The width of a string in the font it will be shown in. */
export type Measure = (text: string) => number;

/**
 * `text`, or its start + "…" + its end, whichever is the longest that still
 * measures within `maxWidth`.
 *
 * Cut by code point, never by UTF-16 unit: a unit boundary can fall inside a
 * surrogate pair and leave half an astral character (a rare Han extension
 * character, an emoji) in front of the ellipsis. The start gets the extra
 * character when the kept count is odd. Whitespace next to the ellipsis is
 * trimmed so it never reads "word …".
 *
 * When nothing fits, the shortest form (one character and the ellipsis) is
 * returned rather than an empty label.
 */
export function truncateMiddle(text: string, maxWidth: number, measure: Measure): string {
  if (measure(text) <= maxWidth) return text;
  const chars = [...text];
  if (chars.length < 2) return text;

  /** `kept` characters of the title, split start-heavy around the ellipsis. */
  const cut = (kept: number): string =>
    chars.slice(0, Math.ceil(kept / 2)).join("").trimEnd() +
    "…" +
    chars.slice(chars.length - Math.floor(kept / 2)).join("").trimStart();

  // Width grows with the kept count, so binary-search the largest that fits.
  let lo = 1;
  let hi = chars.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (measure(cut(mid)) <= maxWidth) lo = mid;
    else hi = mid - 1;
  }
  return cut(lo);
}

/**
 * A canvas-backed `Measure` in `el`'s computed font, or `null` where there is
 * no canvas (jsdom, or a browser that refuses the context) — callers then show
 * the whole title and let CSS clip it.
 *
 * The font is spelled out longhand rather than read from the `font` shorthand:
 * Firefox serialises that to an empty string, and an empty font would measure
 * everything in the canvas default and truncate to the wrong width.
 * Letter-spacing is carried over where the canvas supports it, because CJK
 * themes often set it and it is part of a label's real width.
 */
export function canvasMeasure(el: Element): Measure | null {
  const ctx = document.createElement("canvas").getContext?.("2d") ?? null;
  if (!ctx) return null;
  const cs = getComputedStyle(el);
  ctx.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  if ("letterSpacing" in ctx && cs.letterSpacing !== "normal") {
    (ctx as CanvasRenderingContext2D & { letterSpacing: string }).letterSpacing = cs.letterSpacing;
  }
  return (text) => ctx.measureText(text).width;
}
