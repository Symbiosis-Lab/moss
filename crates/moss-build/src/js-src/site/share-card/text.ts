/**
 * Share-card text engine — kinsoku-aware wrapping, justification, and
 * CJK↔Latin spacing. Split from share-card.ts (ratchet size budget);
 * share-card.ts re-exports the public pieces.
 */

/**
 * The capture→render data contract. Defined here — the leaf of the module
 * graph — so selection-actions (capture) and share-card (render) can both
 * import it without re-creating the share-card ↔ selection-actions cycle.
 *
 * The shape carries the two things a reader can see and the card kept losing:
 * **which lines the author wrote** (a block is lines, not a string — see
 * `splitIntoLines` in block-text.ts) and **what kind of block it is** (`tag`,
 * which was captured and tested for a year while nothing read it, so a heading,
 * a list item and a paragraph all printed as identical justified prose).
 */
export interface CardSegment {
  text: string;
  highlighted: boolean; // true = the selected quote
}

/** One hard line: the author's line, before any wrapping to the card's width. */
export type CardLine = CardSegment[];

export interface CardBlock {
  lines: CardLine[];
  tag: string; // "P", "H2", "BLOCKQUOTE", "LI", "PRE", etc.
  /** List marker for an LI — "•" or "3." — as the page shows it. */
  marker?: string;
}

// ── Text wrapping (kinsoku-aware, token-based) ───────────────────────────

// Characters that must not start a line: CJK closing/middle punctuation
// plus their ASCII cousins — a mixed-script line must not open with a
// bare period or comma.
const NO_START = new Set(
  (
    "。，、；：！？）》」』】〕…—·～‥”’" +
    ".,;:!?)]"
  ).split("")
);

// Characters that must not end a line (opening punctuation, CJK + ASCII)
const NO_END = new Set(
  ("（《「『【〔“‘" + "([").split("")
);

// Han, kana, hangul, CJK punctuation, and full-width forms — scripts whose
// characters wrap independently (one token per character). BMP-only: ext-B+
// ideographs (non-BMP) fall into unbreakable runs, an accepted rarity.
// CJK punctuation (U+3000-303F) needs no range of its own: it already sits
// inside the U+2E80-9FFF range below. It had one, which is what CodeQL's
// js/overly-large-range check flagged. The two forms match identically
// across all of Unicode, so dropping the redundant range is a no-op.
const CJK_CHAR_RE =
  /[⺀-鿿豈-﫿＀-￯가-힯]/;

const SPACE_CHARS = new Set([" ", " "]);

/** Exported for the vertical card: it draws a CJK glyph upright and rotates
 * everything else, so it needs the same script test `tokenize` wraps by. */
export function isCjkChar(ch: string): boolean {
  return CJK_CHAR_RE.test(ch);
}

/**
 * ，。、 — the marks vertical typesetting sets in the top-right quadrant of
 * their cell rather than centred. No font-glyph swap (a `FE10-FE19`/
 * `FE30-FE44` "vertical presentation form" lookup): those code points are
 * missing from most CJK fonts a reader actually has installed, and a
 * missing glyph renders as a tofu box — worse than an upright mark sitting
 * in the wrong corner of its cell.
 */
const CORNER_PUNCTUATION = new Set(["，", "。", "、"]);

export function isCornerPunctuation(ch: string): boolean {
  return CORNER_PUNCTUATION.has(ch);
}

/**
 * Brackets, quote-corners and dashes — marks with a visible horizontal
 * extent (a corner stroke, a long dash) that reads as sideways-lying
 * clutter if left upright in a vertical column. Vertical typesetting
 * rotates these a quarter turn, exactly like a Latin run: 「」『』《》
 * 〈〉（）［］｛｝【】〔〕 (U+3008–3011, U+3014–301B, plus the paired ASCII
 * forms this file already tracks for kinsoku) and the two horizontal marks
 * — (em dash) and … (ellipsis). ：；！？ are deliberately absent — they read
 * as vertical strokes/dots already and vertical typesetting keeps them
 * upright and centred, not rotated and not corner-shifted.
 */
const ROTATED_PUNCTUATION = new Set([
  "「", "」", "『", "』", "《", "》", "〈", "〉",
  "（", "）", "［", "］", "｛", "｝", "【", "】", "〔", "〕",
  "(", ")", "[", "]",
  "—", "…",
]);

export function isRotatedPunctuation(ch: string): boolean {
  return ROTATED_PUNCTUATION.has(ch);
}

/**
 * Split text into unbreakable wrap tokens: each CJK character stands
 * alone, a run of anything else non-space (a Latin word, a number, an
 * inline URL) stays whole, and each space is its own token. This is what
 * keeps English words from breaking mid-word at every line end.
 */
export function tokenize(text: string): string[] {
  const tokens: string[] = [];
  let run = "";
  for (const ch of [...text]) {
    if (SPACE_CHARS.has(ch) || isCjkChar(ch)) {
      if (run) {
        tokens.push(run);
        run = "";
      }
      tokens.push(ch);
    } else {
      run += ch;
    }
  }
  if (run) tokens.push(run);
  return tokens;
}

/** Count of CJK characters — justification is a CJK register; lines with
 * fewer than 2 stay ragged-right. */
function cjkCount(text: string): number {
  let n = 0;
  for (const ch of text) if (isCjkChar(ch)) n++;
  return n;
}

function trimLineEnd(line: string): string {
  let end = line.length;
  while (end > 0 && SPACE_CHARS.has(line[end - 1])) end--;
  return line.slice(0, end);
}

export function wrapText(
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number
): string[] {
  const lines: string[] = [];
  let current = "";

  const flush = () => {
    const trimmed = trimLineEnd(current);
    if (trimmed) lines.push(trimmed);
    current = "";
  };

  for (const tok of tokenize(text)) {
    // A token wider than the measure (a long URL) hard-splits by character
    // — overflow would be worse.
    if (ctx.measureText(tok).width > maxWidth) {
      for (const ch of [...tok]) {
        const test = current + ch;
        if (ctx.measureText(test).width > maxWidth && current) {
          flush();
          current = ch;
        } else {
          current = test;
        }
      }
      continue;
    }

    // A closing mark that arrives after a kinsoku flush still may not open a
    // line. `。」` tokenizes as two characters: the 。 hung past the measure,
    // and the 」 then found the line already flushed and opened the next one
    // alone. Hanging it too is not the answer — two marks past the measure is
    // more overflow than the layout leaves room for, and on the vertical card
    // the second one landed on the title rule. So 追い出し instead: take the
    // whole trailing punctuation run, and the character it belongs to, down to
    // the new line, which then opens on a real character.
    if (!current && lines.length && [...tok].every((c) => NO_START.has(c))) {
      let prev = lines[lines.length - 1];
      let carried = tok;
      while (prev.length > 1 && NO_START.has(prev[prev.length - 1])) {
        carried = prev[prev.length - 1] + carried;
        prev = prev.slice(0, -1);
      }
      if (prev.length > 1) {
        carried = prev[prev.length - 1] + carried;
        prev = prev.slice(0, -1);
      }
      lines[lines.length - 1] = prev;
      current = carried;
      continue;
    }

    const test = current + tok;
    if (ctx.measureText(test).width > maxWidth && current) {
      // Kinsoku: a punctuation token (lone char or a run like —— / …… /
      // "),") can't open a line — keep it here, overflow allowed
      if ([...tok].every((c) => NO_START.has(c))) {
        current = test;
        flush();
        continue;
      }
      // Kinsoku: a line can't end on opening punctuation — pull it down
      const last = current[current.length - 1];
      if (current.length > 1 && NO_END.has(last)) {
        current = current.slice(0, -1);
        flush();
        current = last + tok;
        continue;
      }
      flush();
      if (!SPACE_CHARS.has(tok)) current = tok; // never open a line with a space
    } else {
      current = test;
    }
  }
  const trimmed = trimLineEnd(current);
  if (trimmed) lines.push(trimmed);
  return lines;
}

// ── Justification helpers ────────────────────────────────────────────────

/**
 * Draw a line justified to fill maxWidth, distributing extra space at
 * token gaps — between CJK characters and around words, never inside a
 * Latin word. Latin-dominant lines (fewer than 2 CJK characters) and last
 * lines stay ragged.
 */
export function drawJustifiedLine(
  ctx: CanvasRenderingContext2D,
  line: string,
  x: number,
  y: number,
  maxWidth: number,
  isLastLine: boolean
): void {
  const tokens = tokenize(line);
  if (isLastLine || tokens.length <= 1 || cjkCount(line) < 2) {
    ctx.fillText(line, x, y);
    return;
  }
  const naturalWidth = ctx.measureText(line).width;
  const gap = (maxWidth - naturalWidth) / (tokens.length - 1);

  let cx = x;
  for (const tok of tokens) {
    ctx.fillText(tok, cx, y);
    cx += ctx.measureText(tok).width + gap;
  }
}

/** Flatten a span list's per-span alpha into one alpha per codepoint, in
 * order — the "which character gets how faded" answer both the horizontal
 * (`drawJustifiedWrappedLine`) and vertical (`drawUprightColumn`, in
 * vertical.ts) per-character alpha paths need before they can tokenize. */
export function charAlphasFromSpans(spans: LineSpan[], getAlpha: (span: LineSpan) => number): number[] {
  const charAlphas: number[] = [];
  for (const span of spans) {
    const alpha = getAlpha(span);
    for (const _ of [...span.text]) charAlphas.push(alpha);
  }
  return charAlphas;
}

/**
 * Tokenize `text` (`tokenize()`'s own boundaries) and, for each token, hand
 * the caller its characters' alphas (from `charAlphas`, indexed by codepoint
 * position, or `null` when the caller has no per-character alpha at all)
 * plus whether they're all equal — the "does this token need to fall back to
 * per-character drawing, or can it paint as one run" decision that a highlight
 * boundary landing mid-token forces on both the horizontal justified line and
 * the vertical column. Positioning stays with the caller: a token draws at a
 * justified x with a token-gap on the horizontal card and down a fixed-width
 * column with punctuation sub-cases on the vertical one, and those two shapes
 * don't share a draw primitive to factor out.
 */
export function forEachTokenAlpha(
  text: string,
  charAlphas: number[] | null,
  visit: (tok: string, tokChars: string[], tokAlphas: number[] | null, uniform: boolean) => void
): void {
  let ci = 0;
  for (const tok of tokenize(text)) {
    const tokChars = [...tok];
    const tokAlphas = charAlphas ? tokChars.map((_, k) => charAlphas[ci + k]) : null;
    const uniform = tokAlphas ? tokAlphas.every((a) => a === tokAlphas[0]) : true;
    visit(tok, tokChars, tokAlphas, uniform);
    ci += tokChars.length;
  }
}

/**
 * Draw a WrappedLine (spans with varying opacity) with the same token-gap
 * justification. A token that straddles a highlight boundary draws its
 * characters back-to-back (no internal gap) with their own alphas.
 */
export function drawJustifiedWrappedLine(
  ctx: CanvasRenderingContext2D,
  spans: LineSpan[],
  x: number,
  y: number,
  maxWidth: number,
  isLastLine: boolean,
  getAlpha: (span: LineSpan) => number
): void {
  const fullText = spans.map((s) => s.text).join("");
  const tokens = tokenize(fullText);

  if (isLastLine || tokens.length <= 1 || cjkCount(fullText) < 2) {
    let cx = x;
    for (const span of spans) {
      ctx.globalAlpha = getAlpha(span);
      ctx.fillText(span.text, cx, y);
      cx += ctx.measureText(span.text).width;
    }
    return;
  }

  const charAlphas = charAlphasFromSpans(spans, getAlpha);
  const naturalWidth = ctx.measureText(fullText).width;
  const gap = (maxWidth - naturalWidth) / (tokens.length - 1);

  let cx = x;
  forEachTokenAlpha(fullText, charAlphas, (tok, tokChars, tokAlphas, uniform) => {
    if (uniform) {
      ctx.globalAlpha = tokAlphas![0];
      ctx.fillText(tok, cx, y);
    } else {
      let tx = cx;
      for (let k = 0; k < tokChars.length; k++) {
        ctx.globalAlpha = tokAlphas![k];
        ctx.fillText(tokChars[k], tx, y);
        tx += ctx.measureText(tokChars[k]).width;
      }
    }
    cx += ctx.measureText(tok).width + gap;
  });
}

// ── Segment-aware line wrapping ──────────────────────────────────────────

export interface LineSpan {
  text: string;
  highlighted: boolean;
}

export type WrappedLine = LineSpan[];

interface TaggedChar {
  char: string;
  highlighted: boolean;
}

/**
 * Wrap an array of CardSegments into lines, splitting spans at wrap
 * points. Token-aware (a Latin word never breaks mid-word, even when the
 * highlight boundary falls inside it) with kinsoku across segment
 * boundaries.
 */
export function wrapSegments(
  ctx: CanvasRenderingContext2D,
  segments: CardSegment[],
  maxWidth: number
): WrappedLine[] {
  // Flatten segments into a tagged character array
  const chars: TaggedChar[] = [];
  for (const seg of segments) {
    if (!seg.text) continue;
    for (const ch of [...seg.text]) {
      chars.push({ char: ch, highlighted: seg.highlighted });
    }
  }
  if (chars.length === 0) return [];

  // Group into unbreakable tagged tokens (mirrors tokenize())
  const tokens: TaggedChar[][] = [];
  let run: TaggedChar[] = [];
  for (const tc of chars) {
    if (SPACE_CHARS.has(tc.char) || isCjkChar(tc.char)) {
      if (run.length) {
        tokens.push(run);
        run = [];
      }
      tokens.push([tc]);
    } else {
      run.push(tc);
    }
  }
  if (run.length) tokens.push(run);

  const lines: WrappedLine[] = [];
  let lineChars: TaggedChar[] = [];
  let lineText = "";

  function flushLine(cs: TaggedChar[]): void {
    // Drop trailing spaces at the break
    let end = cs.length;
    while (end > 0 && SPACE_CHARS.has(cs[end - 1].char)) end--;
    const kept = cs.slice(0, end);
    if (kept.length === 0) return;

    // Group consecutive chars with the same highlighted value into spans
    const spans: LineSpan[] = [];
    let spanText = "";
    let spanHighlighted = kept[0].highlighted;
    for (const tc of kept) {
      if (tc.highlighted !== spanHighlighted) {
        if (spanText) spans.push({ text: spanText, highlighted: spanHighlighted });
        spanText = tc.char;
        spanHighlighted = tc.highlighted;
      } else {
        spanText += tc.char;
      }
    }
    if (spanText) spans.push({ text: spanText, highlighted: spanHighlighted });
    lines.push(spans);
  }

  for (const tok of tokens) {
    const tokText = tok.map((c) => c.char).join("");

    // Oversized token: hard-split by character
    if (ctx.measureText(tokText).width > maxWidth) {
      for (const tc of tok) {
        const test = lineText + tc.char;
        if (ctx.measureText(test).width > maxWidth && lineText) {
          flushLine(lineChars);
          lineChars = [tc];
          lineText = tc.char;
        } else {
          lineChars.push(tc);
          lineText = test;
        }
      }
      continue;
    }

    const test = lineText + tokText;
    if (ctx.measureText(test).width > maxWidth && lineText) {
      // Kinsoku: a punctuation token (lone char or a run) can't open a line
      if (tok.every((c) => NO_START.has(c.char))) {
        lineChars.push(...tok);
        flushLine(lineChars);
        lineChars = [];
        lineText = "";
        continue;
      }
      // Kinsoku: a line can't end on opening punctuation
      if (
        lineChars.length > 1 &&
        NO_END.has(lineChars[lineChars.length - 1].char)
      ) {
        const pulled = lineChars.pop()!;
        flushLine(lineChars);
        lineChars = [pulled, ...tok];
        lineText = pulled.char + tokText;
        continue;
      }
      flushLine(lineChars);
      if (tok.length === 1 && SPACE_CHARS.has(tok[0].char)) {
        lineChars = [];
        lineText = "";
      } else {
        lineChars = [...tok];
        lineText = tokText;
      }
    } else {
      lineChars.push(...tok);
      lineText = test;
    }
  }
  flushLine(lineChars);

  return lines;
}

// ── CJK-Latin spacing ───────────────────────────────────────────────────

/** Insert thin space (U+2009) at CJK↔Latin/numeric boundaries */
export function addCjkLatinSpacing(text: string): string {
  return text
    .replace(
      /([\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff])([A-Za-z0-9])/g,
      "$1\u2009$2"
    )
    .replace(
      /([A-Za-z0-9])([\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff])/g,
      "$1\u2009$2"
    );
}

const SEG_CJK_END = /[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff]$/;
const SEG_CJK_START = /^[\u4e00-\u9fff\u3400-\u4dbf\uf900-\ufaff]/;
const SEG_LATIN_END = /[A-Za-z0-9]$/;
const SEG_LATIN_START = /^[A-Za-z0-9]/;

/**
 * Per-segment CJK↔Latin spacing PLUS the boundaries BETWEEN segments — a
 * highlight edge that coincides with a script change (selecting an English
 * term inside Chinese prose) previously lost its thin space.
 */
export function spaceSegments(segments: CardSegment[]): CardSegment[] {
  const out = segments.map((s) => ({ ...s, text: addCjkLatinSpacing(s.text) }));
  for (let i = 1; i < out.length; i++) {
    const prev = out[i - 1].text;
    const cur = out[i].text;
    if (!prev || !cur) continue;
    if (
      (SEG_CJK_END.test(prev) && SEG_LATIN_START.test(cur)) ||
      (SEG_LATIN_END.test(prev) && SEG_CJK_START.test(cur))
    ) {
      out[i] = { ...out[i], text: "\u2009" + out[i].text };
    }
  }
  return out;
}


/**
 * Short mode is the big centred 「quote」 — a treatment for one line of text.
 * A selection containing a hard line break is never that, however few
 * characters it has: a four-line poem is 30 characters and short mode used to
 * throw its blocks away, print it as one centred run and lose every break.
 * Both the horizontal and the vertical card decide by this one rule.
 */
export function isShortQuote(text: string): boolean {
  return text.length <= 40 && !text.includes("\n");
}

/** Body size in px: short mode reads big, long mode reads as text. */
export function quoteFontSize(isShort: boolean): number {
  return isShort ? 22 : 16;
}
