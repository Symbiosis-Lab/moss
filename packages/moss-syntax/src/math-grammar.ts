/**
 * @lezer/markdown inline grammar extension for `$…$` / `$$…$$` math SOURCE
 * spans — HIGHLIGHT-ONLY (design D8).
 * The editor styles the raw TeX; it renders NO glyphs. The preview pane
 * already shows the build's real typeset output.
 *
 * ── The delimiter contract ──────────────────────────────────────────────
 * This grammar must agree, span-for-span, with the build's pulldown-cmark
 * (Options::ENABLE_MATH). The agreement is CONTRACT-TESTED against the
 * cross-language golden vectors in
 * crates/moss-core/tests/fixtures/math-delimiters.vectors.json
 * (see cm-math-lezer.test.ts here, tests/math_delimiters.rs on the Rust
 * side). If this grammar disagrees with a vector, fix the GRAMMAR — never
 * the vectors. The rules, as measured (never reasoned from a spec):
 *
 * - An opening `$` must NOT be followed by whitespace (`$ x $` is not math).
 * - A closing `$` must be IMMEDIATELY preceded by a non-whitespace byte.
 *   `$5 and $10` survives only because the second `$` follows a space;
 *   `$5-$10` and `一个$5，两个$10` DO become math (`-` and `，` are
 *   non-whitespace).
 * - The FIRST unescaped `$` after the opener is the ONLY closer candidate:
 *   if it is invalid (inline: preceded by whitespace; display: not a `$$`
 *   pair) the whole opener FAILS — the scan does NOT continue to a later
 *   `$` (measured: `$a $b$ c$` → inline "b", not "a $b"; `$$a$b$$` →
 *   inline "a", not display "a$b"; `$$ $ $$` → no math). Escaped `\$` is
 *   skipped entirely and is not a candidate.
 * - Inline `$` math CAN span a soft line break within a paragraph
 *   (measured: `$a\nb$` → InlineMath "a\nb"), so unlike the Wikilink
 *   parser we do NOT bail on newline.
 * - `\$` never opens or closes: the built-in Escape parser consumes `\$`
 *   before us (we register before Emphasis, NOT before Escape), and the
 *   closer scan skips backslash-escaped characters itself.
 * - Code spans and fenced code are lexed first and never yield math —
 *   InlineCode also runs before us, and block parsing claims fences.
 *
 * ── Why `$$` display math is an INLINE parser, not a block parser ───────
 * pulldown-cmark's display math is an inline-level construct (measured:
 * `foo $$x^2$$ bar` mid-line IS display math; `$$\n\frac{a}{b}\n$$` is ONE
 * span whose TeX keeps its newlines; a blank line — a paragraph break —
 * stops it; unmatched `$$x$` falls back to inline `$x$`). Only an inline
 * parser scanning across soft breaks reproduces those spans exactly. This
 * also sidesteps the fence-delimited-composite-block infinite-loop hazard
 * documented in cm-shortcode-lezer.ts (:12-16) entirely — we never enter
 * block parsing.
 *
 * Node hierarchy for `$E=mc^2$` (and analogously `$$…$$`):
 *
 *   InlineMath | DisplayMath     $E=mc^2$      (full span, TeX between marks)
 *     MathMark                   $   (or $$)
 *     MathMark                   $   (or $$)
 *
 * The TeX payload has no dedicated child node — consumers slice between the
 * marks. Styling is applied by cm-live-preview (`.cm-lp-math` mark), NOT by
 * cm-highlight (highlightNodeTypes only styles standard tags).
 */

import type { MarkdownConfig, InlineContext } from '@lezer/markdown';

const DOLLAR = 36;    // char code for `$`
const BACKSLASH = 92; // char code for `\`
const SPACE = 32;
const TAB = 9;
const NEWLINE = 10;

/** Whitespace in the sense of the measured delimiter rules. `cx.char` returns
 *  -1 past the end of the inline section, which is correctly NOT whitespace
 *  (an opener at end-of-section simply finds no closer). */
function isWhitespace(ch: number): boolean {
  return ch === SPACE || ch === TAB || ch === NEWLINE;
}

/** `$$…$$`: no whitespace rules on either side (the TeX keeps surrounding
 *  spaces/newlines verbatim — `$$ x^2 $$` → " x^2 "). The FIRST unescaped `$`
 *  after the opener is the only closer candidate: it must start a `$$` pair,
 *  or the opener FAILS (measured: `$$a$b$$` → inline "a" from the retry at
 *  pos+1, NOT display "a$b"; `$$ $ $$` → no math). The scan may cross soft
 *  line breaks; the inline section ends at a blank line, matching pulldown's
 *  paragraph-break behavior. */
function parseDisplay(cx: InlineContext, pos: number): number {
  const end = cx.end;
  let i = pos + 2;
  while (i < end) {
    const ch = cx.char(i);
    if (ch === BACKSLASH) { i += 2; continue; } // `\$` is not a candidate
    if (ch === DOLLAR) {
      // First candidate decides: not a `$$` pair → the opener fails, and
      // lezer retries at pos+1 where the second `$` can open INLINE math
      // (measured: `$$x$` → inline "x", `$$$a$$` → display "a" from pos+1).
      if (cx.char(i + 1) !== DOLLAR) return -1;
      return cx.addElement(cx.elt('DisplayMath', pos, i + 2, [
        cx.elt('MathMark', pos, pos + 2),
        cx.elt('MathMark', i, i + 2),
      ]));
    }
    i++;
  }
  return -1; // unclosed: same retry-at-pos+1 fallback as above
}

/** `$…$`: opener not followed by whitespace; the FIRST unescaped `$` after
 *  the opener is the only closer candidate — preceded by whitespace means the
 *  whole opener FAILS, the scan does NOT continue to a later `$` (measured:
 *  `$a $b$ c$` → inline "b", not "a $b"). May span soft line breaks — see
 *  the module header. */
function parseInlineMath(cx: InlineContext, pos: number): number {
  const after = cx.char(pos + 1);
  if (after < 0 || isWhitespace(after)) return -1;
  const end = cx.end;
  let i = pos + 1; // first iteration is `after`: non-`$` (routed) and non-ws
  while (i < end) {
    const ch = cx.char(i);
    if (ch === BACKSLASH) { i += 2; continue; } // `\$` is not a candidate
    if (ch === DOLLAR) {
      if (isWhitespace(cx.char(i - 1))) return -1; // first candidate decides
      return cx.addElement(cx.elt('InlineMath', pos, i + 1, [
        cx.elt('MathMark', pos, pos + 1),
        cx.elt('MathMark', i, i + 1),
      ]));
    }
    i++;
  }
  return -1;
}

export const mathConfig: MarkdownConfig = {
  defineNodes: [
    'InlineMath',
    'DisplayMath',
    'MathMark',
  ],

  parseInline: [
    {
      name: 'Math',
      // Before Emphasis so a `$…$` span claims any `*`/`_` inside it before
      // the emphasis delimiters can pair (measured: `*a $b* c$` → math
      // "b* c", no emphasis). Deliberately NOT before Escape or InlineCode:
      // those must keep consuming `\$` and `` `$…$` `` first (vectors
      // "a\$5 and \$10" and "`$E=mc^2$`").
      before: 'Emphasis',

      parse(cx: InlineContext, next: number, pos: number): number {
        if (next !== DOLLAR) return -1;
        return cx.char(pos + 1) === DOLLAR
          ? parseDisplay(cx, pos)
          : parseInlineMath(cx, pos);
      },
    },
  ],
};
