/**
 * Setting a captured selection as blocks — the card's answer to "what did the
 * page look like".
 *
 * The card does not re-implement the site stylesheet. It reads the same tag the
 * page rendered and picks a treatment that says the same thing: a heading is
 * bigger and unjustified, a quote is indented behind a rule, a list item hangs
 * off its marker, a code block is monospaced and never justified or CJK-spaced
 * (in code a space is data). Anything it does not recognise is body prose.
 *
 * The unit of layout is the **authored line**, not the block. `block-text.ts`
 * guarantees a `\n` in captured text is a break the author asked for, and
 * `splitIntoLines` turns those into `CardBlock.lines`; here each of them is
 * wrapped, justified and drawn on its own. That is the whole difference between
 * a poem and a paragraph — and the reason a four-line stanza used to arrive as
 * one run-on line. See `docs/archive/2026-08-10-share-card-audit.md`.
 */

import {
  type CardBlock,
  type LineSpan,
  type WrappedLine,
  wrapSegments,
  spaceSegments,
  drawJustifiedWrappedLine,
} from "./text";
import type { Palette } from "./bar";

/**
 * How far a wrapped continuation is set in from the line it continues. Only
 * hand-broken blocks get it: in a poem it is the difference between "the poet
 * ended the line here" and "the card ran out of room".
 */
const VERSE_HANGING_INDENT = 14;

/** How a block is set, derived from its HTML tag. */
interface BlockStyle {
  fontSize: number;
  lineStep: number;
  bold: boolean;
  italic: boolean;
  mono: boolean;
  /** Left inset for the whole block, in card pixels. */
  indent: number;
  /** Draw a vertical rule in the gutter (blockquote). */
  rule: boolean;
  justify: boolean;
  /** Extra space above, over and above the inter-block gap. */
  gapBefore: number;
}

/** One block, measured: everything the draw pass needs and nothing it re-derives. */
export interface BlockLayout {
  style: BlockStyle;
  /** One entry per hard line the author wrote; each is that line, wrapped. */
  hardLines: WrappedLine[][];
  hasHighlight: boolean;
  marker?: string;
  /** Continuation indent for a hand-broken block; 0 for ordinary prose. */
  hang: number;
}

export interface BlockMetrics {
  bodyFontSize: number;
  serifFont: string;
  maxWidth: number;
  paragraphGap: number;
}

function blockStyle(tag: string, bodyFontSize: number): BlockStyle {
  const base = (over: Partial<BlockStyle> = {}): BlockStyle => {
    const fontSize = over.fontSize ?? bodyFontSize;
    return {
      fontSize,
      lineStep: fontSize * 1.85,
      bold: false,
      italic: false,
      mono: false,
      indent: 0,
      rule: false,
      justify: true,
      gapBefore: 0,
      ...over,
    };
  };

  switch (tag) {
    case "H1":
    case "H2":
      return base({ fontSize: bodyFontSize * 1.35, bold: true, justify: false, gapBefore: 6 });
    case "H3":
    case "H4":
      return base({ fontSize: bodyFontSize * 1.18, bold: true, justify: false, gapBefore: 4 });
    case "H5":
    case "H6":
      return base({ bold: true, justify: false, gapBefore: 4 });
    case "BLOCKQUOTE":
      return base({ italic: true, indent: 14, rule: true });
    case "LI":
    case "DD":
      return base({ indent: 18 });
    case "PRE":
      return base({ fontSize: bodyFontSize * 0.85, mono: true, justify: false, indent: 6 });
    case "FIGCAPTION":
      return base({ fontSize: bodyFontSize * 0.85, justify: false });
    case "DT":
      return base({ bold: true });
    default:
      return base();
  }
}

function blockFont(style: BlockStyle, serifFont: string): string {
  const family = style.mono
    ? "ui-monospace, SFMono-Regular, Menlo, monospace"
    : serifFont;
  return `${style.italic ? "italic " : ""}${style.bold ? "600 " : ""}${style.fontSize}px ${family}`;
}

/**
 * Wrap every authored line of every block and total the height they need.
 *
 * Measuring and drawing wrap at the same width by construction — they share
 * this result — so the height reserved is always the height drawn.
 */
export function layoutBlocks(
  ctx: CanvasRenderingContext2D,
  blocks: CardBlock[],
  m: BlockMetrics
): { rows: BlockLayout[]; height: number } {
  const rows: BlockLayout[] = [];
  let height = 0;

  for (let bi = 0; bi < blocks.length; bi++) {
    const block = blocks[bi];
    const style = blockStyle(block.tag, m.bodyFontSize);
    ctx.font = blockFont(style, m.serifFont);
    // Verse only: a block the author broke by hand gets a hanging indent, so a
    // reader can tell the poet's breaks from the card's width.
    const hang = block.lines.length > 1 ? VERSE_HANGING_INDENT : 0;
    const measure = m.maxWidth - style.indent - hang;
    const hardLines = block.lines.map((line) =>
      wrapSegments(ctx, style.mono ? line : spaceSegments(line), measure)
    );
    rows.push({
      style,
      hardLines,
      hasHighlight: block.lines.some((line) => line.some((s) => s.highlighted)),
      marker: block.marker,
      hang,
    });

    height += style.gapBefore;
    for (const wrapped of hardLines) {
      // An empty hard line is a blank line the author typed; it still occupies one.
      height += Math.max(1, wrapped.length) * style.lineStep;
    }
    if (bi < blocks.length - 1) height += m.paragraphGap;
  }

  return { rows, height };
}

/**
 * The parts of a block that are not text: a list marker in the gutter, a
 * blockquote's rule. Drawn after the lines, so it can span the block's full
 * measured height.
 */
function drawDecoration(
  ctx: CanvasRenderingContext2D,
  palette: Palette,
  row: BlockLayout,
  padding: number,
  top: number,
  bottom: number,
  serifFont: string
): void {
  if (row.style.rule) {
    ctx.globalAlpha = 0.25;
    ctx.fillStyle = palette.text;
    // `top` is already the glyph top — the canvas draws with
    // `textBaseline = "top"` — so the rule needs no baseline correction. It got
    // one anyway, which lifted every quote's rule a full font-size above the
    // words it marks and into the card's padding.
    ctx.fillRect(padding, top, 2, bottom - top);
    ctx.globalAlpha = 1;
  }
  if (row.marker) {
    ctx.globalAlpha = 0.55;
    ctx.fillStyle = palette.text;
    ctx.font = blockFont({ ...row.style, bold: false, italic: false }, serifFont);
    ctx.fillText(row.marker, padding, top);
    ctx.globalAlpha = 1;
  }
}

/**
 * Which rows fall before/after the highlighted run — computed once per card
 * so the horizontal blocks and the vertical card's columns (`vertical.ts`)
 * read the same verdict for "is this row context, and on which side".
 */
export interface RowFade {
  hasHighlight: boolean;
  isBefore: boolean;
  isAfter: boolean;
}

export function rowFades(rows: { hasHighlight: boolean }[]): RowFade[] {
  const highlighted = rows
    .map((r, i) => (r.hasHighlight ? i : -1))
    .filter((i) => i >= 0);
  return rows.map((r, bi) => ({
    hasHighlight: r.hasHighlight,
    isBefore: !r.hasHighlight && highlighted.length > 0 && bi < highlighted[0],
    isAfter:
      !r.hasHighlight && highlighted.length > 0 && bi > highlighted[highlighted.length - 1],
  }));
}

/**
 * A span's opacity within its row: the row holding the highlight draws its
 * own selected spans full strength and its own unselected spans at 0.35; a
 * row before/after that one fades across its own drawn progress in the
 * 0.18–0.30 band; any other row (no highlight anywhere on the card) sits
 * flat at 0.3. `drawn`/`total` are the row's own count of lines (horizontal)
 * or columns (vertical) emitted so far.
 */
export function fadeAlpha(fade: RowFade, span: LineSpan, drawn: number, total: number): number {
  if (fade.hasHighlight) return span.highlighted ? 1.0 : 0.35;
  const progress = total > 1 ? drawn / (total - 1) : 1;
  if (fade.isBefore) return 0.18 + 0.12 * progress;
  if (fade.isAfter) return 0.3 - 0.12 * (total > 1 ? drawn / (total - 1) : 0);
  return 0.3;
}

/**
 * Draw the measured blocks starting at `y`, and return the y they end on.
 *
 * Opacity carries the reader's eye: the selected block is full strength (its
 * unselected remainder at 0.35), and the context around it fades away from the
 * selection in the 0.18–0.30 band — low enough to recede, high enough to
 * survive a chat app's recompression.
 */
export function drawBlocks(
  ctx: CanvasRenderingContext2D,
  rows: BlockLayout[],
  palette: Palette,
  padding: number,
  startY: number,
  m: BlockMetrics
): number {
  let y = startY;
  const fades = rowFades(rows);

  for (let bi = 0; bi < rows.length; bi++) {
    const { style, hardLines, hang } = rows[bi];
    const fade = fades[bi];
    ctx.font = blockFont(style, m.serifFont);
    const measure = m.maxWidth - style.indent - hang;
    const x = padding + style.indent;

    y += style.gapBefore;
    const blockTop = y;
    // The fade runs the height of the block, not of each line inside it.
    const total = hardLines.reduce((n, l) => n + Math.max(1, l.length), 0);
    let drawn = 0;

    for (const wrapped of hardLines) {
      if (wrapped.length === 0) {
        y += style.lineStep;
        drawn++;
        continue;
      }
      for (let li = 0; li < wrapped.length; li++) {
        const getAlpha = (span: LineSpan): number => fadeAlpha(fade, span, drawn, total);

        ctx.fillStyle = palette.text;
        // A hanging indent moves the continuation lines, not the first one, so
        // the first line has `hang` more room to fill. Wrapping stays at the
        // narrow measure for every line (never overflows); only the justified
        // width follows the line's own x, which is what keeps the right edge
        // straight instead of notching the first line inward.
        drawJustifiedWrappedLine(
          ctx,
          wrapped[li],
          x + (li > 0 ? hang : 0),
          y,
          li > 0 ? measure : measure + hang,
          // Ragged at the end of the AUTHOR's line, not of the block:
          // justifying the last line of a verse would stretch it across the
          // full measure.
          li === wrapped.length - 1 || !style.justify,
          getAlpha
        );
        y += style.lineStep;
        drawn++;
      }
    }

    ctx.globalAlpha = 1.0;
    drawDecoration(ctx, palette, rows[bi], padding, blockTop, y, m.serifFont);
    if (bi < rows.length - 1) y += m.paragraphGap;
  }

  ctx.globalAlpha = 1.0;
  return y;
}
