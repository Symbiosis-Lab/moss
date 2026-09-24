/**
 * @lezer/markdown grammar extension for footnotes — `[^label]` markers and
 * `[^label]:` definition lines.
 *
 * ── Why this exists ─────────────────────────────────────────────────────
 * The build has parsed footnotes since `Options::ENABLE_FOOTNOTES`
 * in `ast/parser.rs`. The editor never has, and CommonMark does not leave
 * unknown brackets alone — it claims them. Measured against the real parser,
 * a footnoted paragraph came out of the editor mangled three different ways:
 *
 *   - `阿公[^2]告訴我` → `Link` node, so live preview hid the brackets and
 *     painted `^2` as an unclickable hyperlink.
 *   - `[^1]: 伊：代词他/她，i1。…` → a link reference DEFINITION, because a
 *     note body with no ASCII space is a valid link destination. The whole
 *     note was highlighted as a URL, and nothing inside it rendered.
 *   - `[^2]: 阿公：祖父，a1 gong1` → the space broke the destination, so the
 *     SAME construct fell back to a paragraph with a fake link in it.
 *
 * Two footnotes in one document rendered three ways, none of them footnotes.
 * Claiming the syntax here is what makes the editor honest about it.
 *
 * ── The build contract ──────────────────────────────────────────────────
 * This grammar must agree with pulldown-cmark, and the agreement that matters
 * most is a NEGATIVE one, asserted on the Rust side in
 * `crates/moss-core/tests/footnotes_and_strikethrough.rs`:
 *
 *   - A marker with no matching definition renders LITERALLY. `Use [^0-9] to
 *     strip non-digits.` ships as `[^0-9]`, not as a footnote. So this
 *     grammar emits a `FootnoteRef` node for the SHAPE, and the decision to
 *     render it as a marker belongs to the consumer, which must check the
 *     document's definitions first (cm-live-preview does). A regex character
 *     class in prose is the case that punishes getting this wrong.
 *   - A marker inside a code span stays literal (`` `[^1]` ``). Handled by
 *     registration order: `InlineCode` runs before `Emphasis`, and we
 *     register before `Emphasis`.
 *
 * ── Node hierarchy ──────────────────────────────────────────────────────
 *
 *   FootnoteRef            [^1]
 *     FootnoteMark         [^
 *     FootnoteLabel        1
 *     FootnoteMark         ]
 *
 *   FootnoteDefinition     [^1]: the note
 *     FootnoteMark         [^
 *     FootnoteLabel        1
 *     FootnoteMark         ]:
 *     Paragraph            the note
 *     …any further blocks the note holds…
 *
 * ── A definition is a CONTAINER, not a line ─────────────────────────────
 * pulldown-cmark treats a footnote definition exactly like a list item: the
 * note keeps going while lines stay indented, so it can hold several
 * paragraphs, a list, a quote. Measured against `crates/moss-core`, after
 * `[^1]: first line`:
 *
 *   second line             → continues the note's paragraph (lazy)
 *       second line         → continues the note's paragraph
 *   blank + `    second`    → a SECOND PARAGRAPH inside the note
 *   blank + `    - item`    → a LIST inside the note
 *   blank + `plain para`    → the note ends; `plain para` is top-level
 *
 * This grammar used to claim only the marker line, and the continuation then
 * parsed as whatever it looked like standing alone. That was not a cosmetic
 * limit: a line indented four spaces looks exactly like an indented code
 * block, so the author who indents CAREFULLY got a monospace code box in the
 * editor where their site renders note prose — the same "editor and build
 * disagree about what this is" defect the original footnote bug was.
 *
 * So `FootnoteDefinition` is a composite block whose continuation rule is
 * `ListItem`'s, with `CONTINUATION_INDENT` in place of the marker width.
 * Four is fixed rather than derived from the label, because that is what
 * pulldown does — `[^a-very-long-label]:` still continues at four spaces.
 * Body content is then parsed by the ordinary block parser inside the
 * container, which is where the lists and second paragraphs come from, and
 * why the body is no longer handed to `parseInline` by hand.
 *
 * The end condition is monotone — indentation, read once per line, and the
 * block always advances a line — so it terminates by construction.
 */

import type { MarkdownConfig, InlineContext, BlockContext, Line } from '@lezer/markdown';

const BRACKET_OPEN = 91;  // [
const BRACKET_CLOSE = 93; // ]
const CARET = 94;         // ^
const COLON = 58;         // :

/**
 * Columns a continuation line must be indented to stay inside the note.
 *
 * Fixed at four, not derived from the marker's width the way a list item
 * derives it from `- `. That is pulldown's rule and it is the one an author
 * can hold in their head: every footnote continues at the same indent,
 * whatever its label is called.
 */
const CONTINUATION_INDENT = 4;

/**
 * Scan a `[^label]` starting at `pos`, returning the offset just past the
 * closing `]`, or -1. `charAt` returns -1 past the end.
 *
 * A label may hold anything but `[`, `]` and a newline, and may not be empty
 * — `[^]` is not a footnote. Both bracket characters are excluded so a
 * nested-bracket typo fails fast instead of swallowing the rest of the line.
 */
function scanLabel(charAt: (i: number) => number, pos: number, end: number): number {
  if (charAt(pos) !== BRACKET_OPEN || charAt(pos + 1) !== CARET) return -1;
  let i = pos + 2;
  while (i < end) {
    const ch = charAt(i);
    if (ch === BRACKET_CLOSE) return i === pos + 2 ? -1 : i + 1; // reject `[^]`
    if (ch === BRACKET_OPEN || ch === 10 /* \n */ || ch < 0) return -1;
    i++;
  }
  return -1;
}

export const footnoteConfig: MarkdownConfig = {
  defineNodes: [
    'FootnoteRef',
    'FootnoteMark',
    'FootnoteLabel',
    {
      name: 'FootnoteDefinition',
      block: true,
      // `ListItem`'s continuation rule (@lezer/markdown's DefaultSkipMarkup),
      // with a fixed indent. A blank line keeps the note open — it is how a
      // note gets a second paragraph — and the first non-blank line that
      // falls short of the indent closes it.
      composite(_cx: BlockContext, line: Line, value: number): boolean {
        if (line.next < 0) return true; // blank
        if (line.indent < line.baseIndent + value) return false;
        line.moveBaseColumn(line.baseIndent + value);
        return true;
      },
    },
  ],

  parseInline: [
    {
      name: 'FootnoteRef',
      // Before Emphasis for the same reason math is: a `*` inside a label
      // must not pair with one outside it. This still leaves Escape,
      // Entity and InlineCode ahead of us, so `\[^1]` and `` `[^1]` ``
      // keep their literal meaning — the Rust suite asserts both.
      before: 'Emphasis',

      parse(cx: InlineContext, next: number, pos: number): number {
        if (next !== BRACKET_OPEN) return -1;
        const to = scanLabel((i) => cx.char(i), pos, cx.end);
        if (to < 0) return -1;
        return cx.addElement(cx.elt('FootnoteRef', pos, to, [
          cx.elt('FootnoteMark', pos, pos + 2),      // [^
          cx.elt('FootnoteLabel', pos + 2, to - 1),  // label
          cx.elt('FootnoteMark', to - 1, to),        // ]
        ]));
      },
    },
  ],

  parseBlock: [
    {
      name: 'FootnoteDefinition',
      // Before LinkReference, which is the parser that used to eat these
      // lines whole. LinkReference sits first in the default block-parser
      // order, so this runs first of all — hence the explicit indent guard
      // below: four spaces still means indented code, not a footnote.
      before: 'LinkReference',

      // Returns `null`, not `true`: the container is open and the rest of
      // THIS line is the note's first block, which the ordinary block loop
      // parses once we hand it a base column past the `]:`.
      parse(cx: BlockContext, line: Line): boolean | null {
        if (line.indent - line.baseIndent >= 4) return false;
        const text = line.text;
        const off = line.pos;
        const labelEnd = scanLabel((i) => (i < text.length ? text.charCodeAt(i) : -1), off, text.length);
        if (labelEnd < 0 || text.charCodeAt(labelEnd) !== COLON) return false;

        const from = cx.lineStart + off;
        // One space after the colon is the marker's, not the note's — hand the
        // body to the block loop starting where the prose does.
        const bodyStart = text.charCodeAt(labelEnd + 1) === 32 ? labelEnd + 2 : labelEnd + 1;

        cx.startComposite('FootnoteDefinition', line.basePos, CONTINUATION_INDENT);
        cx.addElement(cx.elt('FootnoteMark', from, from + 2));                  // [^
        cx.addElement(cx.elt('FootnoteLabel', from + 2, cx.lineStart + labelEnd - 1));
        cx.addElement(cx.elt('FootnoteMark', cx.lineStart + labelEnd - 1, cx.lineStart + labelEnd + 1)); // ]:
        line.moveBase(bodyStart);
        return null;
      },
    },
  ],
};
