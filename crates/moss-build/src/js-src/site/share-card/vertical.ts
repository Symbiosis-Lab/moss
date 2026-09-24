/**
 * The share card for a page set vertically (`body[data-typesetting="vertical"]`).
 *
 * A portrait canvas read right to left: the cover as a landscape band across
 * the full width at the top (a photograph of a mounted scroll — object-
 * position: center, because the inscription sits in the vertical middle of
 * the mount, not at its blank silk header); the quote below it as tall
 * columns running right to left; and, past a hairline at the reading end
 * (the far left, where a vertical reader finishes), the citation set
 * vertically too — title, then one combined author-and-domain column — with
 * the QR seal below it in the bottom-left corner. The card grows WIDER with
 * more text, never taller: `QUOTE_ZONE_HEIGHT` is a fixed block-size cap on
 * both the quote's and the meta's columns, independent of the card's total
 * height.
 *
 * The columns are drawn glyph by glyph, not by the engine: `canvas.style.
 * writingMode`/`textOrientation` used to carry this (relying on the browser
 * to lay `fillText` out vertically before a quarter-turn `ctx.rotate` stood
 * the column up), but a canvas 2D context is never laid out — the style
 * only matters where a renderer chooses to special-case it, and one major
 * engine does not. There the run drew flat, and the *whole line* still got
 * rotated a quarter turn, so every character came out sideways instead of
 * upright: correct column direction, wrong glyph orientation, on exactly the
 * engine nobody had checked. `drawUprightColumn` (below) replaces the trick
 * with `tokenize()`'s own token boundaries — the ones `wrapText` already
 * wraps by — walked one at a time down the column: a CJK glyph draws
 * upright at its own advance, a run of anything else (Latin, digits) rotates
 * as one sideways unit, and a CJK punctuation mark shifts toward the
 * top-right of its cell the way vertical typesetting sets it. `measureText`
 * still gives the column advance, so `wrapText`'s kinsoku wrapping is
 * unchanged — only how a wrapped line paints changed.
 *
 * Contract: takes `text` and, when `options.blocks` is given (long mode, a
 * real selection), wraps the quote from those blocks instead of from `text`
 * — each authored line becomes one or more columns, and a line outside the
 * highlighted selection paints at the same reduced alpha `blocks.ts` already
 * gives horizontal's context prose (`fadeAlpha`, shared with it). Without
 * `blocks` (short mode, or a capture that produced none), the quote still
 * renders from the plain `text`, at full ink throughout.
 */

import { type Palette, formatCardTitle, extractDomain, ellipsize } from "./bar";
import {
  wrapText,
  wrapSegments,
  spaceSegments,
  addCjkLatinSpacing,
  isShortQuote,
  quoteFontSize,
  isCjkChar,
  isCornerPunctuation,
  isRotatedPunctuation,
  charAlphasFromSpans,
  forEachTokenAlpha,
  type CardBlock,
  type LineSpan,
  type WrappedLine,
} from "./text";
import { rowFades, fadeAlpha, type RowFade } from "./blocks";
import { drawCoverImage } from "./cover";

/** One drawable quote column: the plain-text legacy path draws a string as
 * a whole; the blocks-aware path draws a `WrappedLine` (spans carrying their
 * own highlight) so context and selection can paint at different alphas. */
type QuoteColumn = string | WrappedLine;

function isSpanColumn(col: QuoteColumn): col is WrappedLine {
  return Array.isArray(col);
}

function columnText(col: QuoteColumn): string {
  return isSpanColumn(col) ? col.map((s) => s.text).join("") : col;
}

/**
 * Draw one already-wrapped column upright, top to bottom, at a fixed x.
 * `line` is a single `wrapText` output line — the column's full text, right
 * to left across columns but top to bottom within one. Walking `tokenize()`
 * mirrors how `wrapText` measured it, so the sum of what this draws is the
 * same advance `measureText(line).width` already promised the layout.
 *
 * A lone CJK character draws upright, centred on its own advance box —
 * except two punctuation sub-cases vertical typesetting treats specially:
 * ，。、 nudge toward the top-right corner (`isCornerPunctuation`), and
 * brackets/quote-corners/dashes/ellipsis rotate a quarter turn as their own
 * sideways run (`isRotatedPunctuation`) exactly like a Latin word does —
 * their ink has a horizontal extent (a corner stroke, a long dash) that
 * reads as lying-down clutter if left upright. ：；！？ take neither
 * treatment: they read as vertical strokes or dots already, so they draw
 * upright and centred like an ordinary CJK character. Anything else — a
 * Latin word, a run of digits, an inline URL fragment — draws as one
 * sideways unit via `drawSidewaysRun`, matching `text-orientation: mixed`'s
 * treatment of a non-CJK run.
 *
 * `alphaAt`, when given, returns the opacity for the character at a given
 * index into `line` (codepoints, matching `[...line]`) — the blocks-aware
 * caller uses it to dim a context character without a second draw pass;
 * omitted, every character paints at whatever `ctx.globalAlpha` already is.
 */
function drawUprightColumn(
  ctx: CanvasRenderingContext2D,
  line: string,
  x: number,
  top: number,
  alphaAt?: (charIndex: number) => number
): void {
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  let y = top;
  const charAlphas = alphaAt ? [...line].map((_, i) => alphaAt(i)) : null;
  forEachTokenAlpha(line, charAlphas, (tok, tokChars, tokAlphas, uniform) => {
    const advance = ctx.measureText(tok).width;
    if (tok === " " || tok === "　") {
      y += advance;
      return;
    }
    if (tokAlphas && uniform) ctx.globalAlpha = tokAlphas[0];

    if (tokChars.length === 1 && isCjkChar(tok) && isRotatedPunctuation(tok)) {
      drawSidewaysRun(ctx, tok, x, y + advance / 2);
    } else if (tokChars.length === 1 && isCjkChar(tok) && isCornerPunctuation(tok)) {
      // Top-right of the cell, not centred — the offset a dedicated
      // vertical glyph would otherwise carve into the character itself.
      // A small fraction of the advance: enough to read as "corner-set"
      // without pushing the mark's own ink outside the column's own
      // one-em window (a punctuation-heavy column is still one glyph wide).
      ctx.fillText(tok, x + advance * 0.15, y - advance * 0.15);
    } else if (tokChars.length === 1 && isCjkChar(tok)) {
      ctx.fillText(tok, x, y + advance / 2);
    } else if (uniform) {
      drawSidewaysRun(ctx, tok, x, y + advance / 2);
    } else {
      // A highlight boundary falls inside this run: draw it character by
      // character so each can carry its own alpha, same fallback
      // `drawJustifiedWrappedLine` takes for a mixed-alpha token.
      let rx = 0;
      for (let k = 0; k < tokChars.length; k++) {
        ctx.globalAlpha = tokAlphas![k];
        const cw = ctx.measureText(tokChars[k]).width;
        drawSidewaysRun(ctx, tokChars[k], x, y + rx + cw / 2);
        rx += cw;
      }
    }
    if (tokAlphas) ctx.globalAlpha = 1;
    y += advance;
  });
}

/** Adapt a `WrappedLine` (spans, each with its own `highlighted` flag) to
 * `drawUprightColumn`'s per-character alpha callback, via the same
 * span-to-per-char flattening `drawJustifiedWrappedLine` uses for the
 * horizontal card (`charAlphasFromSpans`, text.ts). */
function drawUprightColumnSpans(
  ctx: CanvasRenderingContext2D,
  spans: LineSpan[],
  x: number,
  top: number,
  getAlpha: (span: LineSpan) => number
): void {
  const charAlphas = charAlphasFromSpans(spans, getAlpha);
  drawUprightColumn(ctx, spans.map((s) => s.text).join(""), x, top, (i) => charAlphas[i]);
}

/** Rotate one run a quarter turn and draw it centred at (x, y) — a plain 2D
 * transform, so unlike the old `text-orientation` trick it needs no engine
 * cooperation to render sideways. Sets its own alignment so it is correct
 * regardless of what the caller left on `ctx` — a non-CJK token inside
 * `drawUprightColumn` needs exactly this centred placement to land in its
 * own advance cell. */
function drawSidewaysRun(ctx: CanvasRenderingContext2D, text: string, x: number, y: number): void {
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(Math.PI / 2);
  ctx.fillText(text, 0, 0);
  ctx.restore();
}

export interface VerticalCardContext {
  palette: Palette;
  serifFont: string;
  isDark: boolean;
  coverImg?: HTMLImageElement | null;
  qrImg?: HTMLImageElement | null;
  blocks?: CardBlock[];
}

const MIN_WIDTH = 220;
const PADDING = 36;

/** The cover band spans the full card width at the top. */
const BAND_HEIGHT = 150;
const BAND_GAP = 20;

/**
 * The block-size CAP shared by the quote's columns and the meta columns
 * beside them — the width `wrapText` wraps against and the width `ellipsize`
 * truncates meta lines against. A quote or a title only wraps into another
 * column once its own column would exceed this; it never grows the cap
 * itself (the card would grow taller instead of wider). This is the
 * decoupling the v1 layout was missing: there, one `CARD_HEIGHT` drove both
 * the card's total height and, indirectly through the bottom bar's variable
 * height, the column cap.
 *
 * The card's actual zone height is `zoneHeight` below — `min(cap, tallest
 * column actually drawn)` — so a short quote gets a short, narrow card and
 * only a quote that reaches the cap makes the card wide instead of tall.
 */
const QUOTE_ZONE_HEIGHT = 250;

/** Reading-end meta block: a hairline, then the title and author·domain columns. */
const HAIRLINE_GAP = 22;
const META_GAP = 22;
const META_COL_STEP = 24;
const META_FONT = 12;

/**
 * The QR seal, bottom-left, below the meta column. A module needs >=4 device
 * px at scale=2, so a >=66px content box clears the largest (33-module) code
 * moss generates; 72 clears it with margin while staying seal-sized.
 */
const QR_SIZE = 72;
const QR_FRAME = 6;
const FOOT_GAP = 20;

/** A meta column's text, or its ellipsis-truncated form if it overflows the
 * shared `QUOTE_ZONE_HEIGHT` cap — empty input stays empty (an unset author
 * draws no column at all, rather than an empty one). `ctx.font` must already
 * be set to the column's own size. */
function fitOrEllipsize(ctx: CanvasRenderingContext2D, str: string, tail: string, cap: number): string {
  if (!str) return "";
  return ctx.measureText(str).width <= cap ? str : ellipsize(ctx, str, tail, cap);
}

export function buildVerticalCardCanvas(
  text: string,
  title: string,
  author: string,
  ctxOpts: VerticalCardContext
): HTMLCanvasElement {
  const { palette, serifFont, isDark, coverImg, qrImg, blocks } = ctxOpts;
  const scale = 2;
  const isShort = isShortQuote(text);
  const fontSize = quoteFontSize(isShort);
  const columnStep = fontSize * 1.9;
  const paragraphGap = columnStep * 0.6;

  const topPad = coverImg ? BAND_HEIGHT + BAND_GAP : PADDING;
  const footHeight = qrImg ? FOOT_GAP + QR_SIZE + QR_FRAME * 2 + PADDING : PADDING;

  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    // No context to measure content against: fall back to the cap.
    canvas.width = MIN_WIDTH * scale;
    canvas.height = (topPad + QUOTE_ZONE_HEIGHT + footHeight) * scale;
    return canvas;
  }

  // ── Measure pass ──────────────────────────────────────────────────────

  ctx.font = `${fontSize}px ${serifFont}`;

  // Long mode with a real selection wraps the quote from the captured
  // blocks — one authored line at a time, same as the horizontal card's
  // `layoutBlocks` — so a line outside the highlight can fade the way
  // horizontal's context prose does. Short mode (and a capture that
  // produced no blocks) keeps the plain-text path: one `「quote」` or one
  // paragraph-per-`\n` run, at full ink.
  const useBlocks = !isShort && !!blocks && blocks.length > 0;

  let paragraphs: QuoteColumn[][];
  let paragraphFades: (RowFade | null)[];

  if (useBlocks) {
    const fades = rowFades(
      blocks!.map((block) => ({
        hasHighlight: block.lines.some((line) => line.some((s) => s.highlighted)),
      }))
    );
    const built = blocks!
      .map((block, bi) => {
        const cols: WrappedLine[] = [];
        for (const line of block.lines) {
          cols.push(...wrapSegments(ctx, spaceSegments(line), QUOTE_ZONE_HEIGHT));
        }
        return { cols, fade: fades[bi] };
      })
      .filter((b) => b.cols.length > 0);
    paragraphs = built.map((b) => b.cols);
    paragraphFades = built.map((b) => b.fade);
  } else {
    const spaced = addCjkLatinSpacing(text);
    paragraphs = (isShort ? [`「${spaced}」`] : spaced.split("\n"))
      .map((p) => wrapText(ctx, p, QUOTE_ZONE_HEIGHT))
      .filter((lines) => lines.length > 0);
    paragraphFades = paragraphs.map(() => null);
  }

  const columnCount = paragraphs.reduce((n, p) => n + p.length, 0);
  // The ink the columns occupy, not the advance they consume: the last column
  // is one glyph wide, not one step (carried forward from the v1 layout).
  const textWidth =
    (columnCount - 1) * columnStep +
    fontSize +
    Math.max(0, paragraphs.length - 1) * paragraphGap;

  // Meta columns: the title (《…》, from formatCardTitle), then one combined
  // "author · domain" line — the same two registers bar.ts draws for the
  // horizontal card's bottom bar (title block, then one meta line), just
  // stacked along the column axis instead of the block axis.
  const { text: formattedTitle } = formatCardTitle(title);
  ctx.font = `${META_FONT}px ${serifFont}`;
  const titleFits = fitOrEllipsize(
    ctx,
    formattedTitle,
    formattedTitle.endsWith("》") ? "…》" : "…",
    QUOTE_ZONE_HEIGHT
  );
  const domain = extractDomain();
  const metaRaw = author ? `${author} · ${domain}` : domain;
  const metaFits = fitOrEllipsize(ctx, metaRaw, "…", QUOTE_ZONE_HEIGHT);

  const metaCount = [titleFits, metaFits].filter(Boolean).length;
  const metaWidth = metaCount ? HAIRLINE_GAP + META_GAP + metaCount * META_COL_STEP + 8 : 0;

  const w = Math.max(MIN_WIDTH, PADDING + textWidth + metaWidth + PADDING);

  // The actual zone height: the tallest column anyone will draw in it, capped
  // at QUOTE_ZONE_HEIGHT. Quote columns can be shorter than the cap (kinsoku
  // aside, `wrapText` only guarantees "at most", not "exactly"); meta columns
  // are measured post-ellipsize, so already <= the cap too.
  ctx.font = `${fontSize}px ${serifFont}`;
  let quoteColumnHeight = 0;
  for (const lines of paragraphs) {
    for (const line of lines) {
      quoteColumnHeight = Math.max(quoteColumnHeight, ctx.measureText(columnText(line)).width);
    }
  }
  ctx.font = `${META_FONT}px ${serifFont}`;
  let metaColumnHeight = 0;
  if (titleFits) metaColumnHeight = Math.max(metaColumnHeight, ctx.measureText(titleFits).width);
  if (metaFits) metaColumnHeight = Math.max(metaColumnHeight, ctx.measureText(metaFits).width);
  const zoneHeight = Math.min(QUOTE_ZONE_HEIGHT, Math.max(quoteColumnHeight, metaColumnHeight));
  const h = topPad + zoneHeight + footHeight;

  canvas.width = w * scale;
  canvas.height = h * scale;
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  ctx.scale(scale, scale);

  // ── Background and cover band ─────────────────────────────────────────

  ctx.fillStyle = palette.bg;
  ctx.fillRect(0, 0, w, h);
  if (coverImg) drawCoverImage(ctx, coverImg, w, BAND_HEIGHT);

  // ── Quote columns, right to left ────────────────────────────────────────

  ctx.font = `${fontSize}px ${serifFont}`;
  ctx.fillStyle = palette.text;

  let x = w - PADDING - fontSize / 2;
  let top = topPad;
  if (isShort) {
    const longest = Math.max(...paragraphs[0].map((l) => ctx.measureText(columnText(l)).width));
    // Never negative: kinsoku hangs a closing mark past the measure by
    // design, so a column can be longer than zoneHeight.
    top = topPad + Math.max(0, (zoneHeight - longest) / 2);
  }
  for (let pi = 0; pi < paragraphs.length; pi++) {
    const lines = paragraphs[pi];
    const fade = paragraphFades[pi];
    const total = lines.length;
    let drawn = 0;
    for (const line of lines) {
      if (fade && isSpanColumn(line)) {
        drawUprightColumnSpans(ctx, line, x, top, (span) => fadeAlpha(fade, span, drawn, total));
      } else {
        drawUprightColumn(ctx, columnText(line), x, top);
      }
      x -= columnStep;
      drawn++;
    }
    x -= paragraphGap;
  }
  ctx.globalAlpha = 1;

  // ── Reading-end meta: hairline, then title / author·domain columns ─────

  // Left edge of the meta block, for the QR seal below to align to (defaults
  // to the card's own left padding when there is no meta at all).
  let metaLeftEdge = PADDING;

  if (metaCount) {
    const quoteLeftEdge = w - PADDING - textWidth;
    const hairlineX = quoteLeftEdge - HAIRLINE_GAP;

    ctx.strokeStyle = palette.rule;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(hairlineX, topPad);
    ctx.lineTo(hairlineX, topPad + zoneHeight);
    ctx.stroke();

    let mx = hairlineX - META_GAP - META_COL_STEP / 2;
    const drawMetaColumn = (str: string, color: string): void => {
      if (!str) return;
      ctx.font = `${META_FONT}px ${serifFont}`;
      ctx.fillStyle = color;
      drawUprightColumn(ctx, str, mx, topPad);
      // The last column drawn is the leftmost — its ink starts roughly half a
      // font size left of its own translate point. That is the meta block's
      // left edge, which the QR seal below aligns to.
      metaLeftEdge = mx - META_FONT / 2;
      mx -= META_COL_STEP;
    };

    drawMetaColumn(titleFits, palette.meta);
    drawMetaColumn(metaFits, palette.muted);
  }

  // ── QR seal, bottom-left ────────────────────────────────────────────────

  if (qrImg) {
    const frameSize = QR_SIZE + QR_FRAME * 2;
    // Left-aligned to the meta block above it, not the card's own padding —
    // meta and QR read as one block at the reading end. Never further right
    // than the card's padding allows, in case the meta block sits close to
    // the card's right-hand content.
    const frameX = Math.min(metaLeftEdge, w - PADDING - frameSize);
    const frameY = h - PADDING - frameSize;

    ctx.strokeStyle = palette.rule;
    ctx.lineWidth = 1;
    ctx.strokeRect(frameX + 0.5, frameY + 0.5, frameSize - 1, frameSize - 1);

    // Dark cards get a paper plate under an ink-on-paper code — an inverted
    // (light-on-dark) QR fails in WeChat's scanner, the one that matters
    // most for this card's audience (see bar.ts's drawBottomBar, the same
    // rule for the horizontal card).
    if (isDark) {
      ctx.fillStyle = "#f5f0e6";
      ctx.fillRect(frameX + 1, frameY + 1, frameSize - 2, frameSize - 2);
    }
    ctx.drawImage(qrImg, frameX + QR_FRAME, frameY + QR_FRAME, QR_SIZE, QR_SIZE);
  }

  return canvas;
}
