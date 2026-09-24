/**
 * @lezer/markdown inline grammar extension for `[[wikilink]]` and `![[embed]]`
 * syntax. How `![[…]]` parses is load-bearing for four modules.
 *
 * Without this, `[[Research]]` mis-parses as a destination-less `Link` node
 * because the outer `[` starts a link bracket and the inner `[Research]`
 * becomes the link text. This parser runs BEFORE the built-in `Link` inline
 * parser (via `before: "Link"`) so it can claim `[[…]]` first, preventing the
 * partial-Link mis-parse entirely.
 *
 * The `!` form needs the same treatment for a sharper reason. Left to the
 * built-in parsers, `![[file.png]]` becomes an `Image` whose `[file.png]` is a
 * bogus inner `Link` — and that inner Link then trips CommonMark's
 * no-nested-links rule, invalidating any REAL link wrapping the embed. So
 * `[![[x.png]]](/url)` produced no `Link` node at all. Claiming the whole
 * `![[…]]` span here, before Image/Link are offered the `!`, fixes both: an
 * embed is identified by node NAME, and a link may wrap one.
 *
 * **Registration must stay `before: 'Link'`.** In `DefaultInline` Link precedes
 * Image, so `before: 'Image'` would let Link claim the first `[` of `[[Page]]`
 * and silently kill every plain wikilink. A test pins this.
 *
 * Node hierarchy emitted for `[[Page|Display text]]`:
 *
 *   Wikilink             [[Page|Display text]]   (full span)
 *     WikilinkMark       [[
 *     WikilinkTarget     Page                    (before the optional `|`)
 *     WikilinkMark       ]]
 *
 * …and for the embed form `![[Photo.png|55%]]`:
 *
 *   WikilinkEmbed        ![[Photo.png|55%]]      (full span, starts at the `!`)
 *     WikilinkMark       ![[                     (3 chars — the whole opener)
 *     WikilinkTarget     Photo.png               (before the optional `|`)
 *     WikilinkMark       ]]
 *
 * The two share child node names deliberately: `WikilinkMark` is already in
 * cm-live-preview's `CELL_HIDDEN` set, so a table cell hides `![[` for free,
 * and every consumer reads positions rather than assuming a 2-char mark.
 *
 * The `|` display-text/pothole separator and everything after it are inside the
 * span but not given a dedicated child node (a later task may add
 * WikilinkLabel). The target (page/asset reference) is always `WikilinkTarget`.
 *
 * Constraints:
 * - Wikilinks are single-line only (no `\n` allowed inside).
 * - An unclosed `[[` returns -1 so the grammar falls through normally.
 * - Inline parsers do NOT run inside fenced code blocks — the block parser
 *   claims those first, so no special guard is needed here.
 */

import type { MarkdownConfig, InlineContext } from '@lezer/markdown';

const BANG = 33;          // char code for `!`
const OPEN_BRACKET = 91;  // char code for `[`
const CLOSE_BRACKET = 93; // char code for `]`
const NEWLINE = 10;        // char code for `\n`

export const wikilinkConfig: MarkdownConfig = {
  defineNodes: [
    'Wikilink',
    'WikilinkMark',
    'WikilinkTarget',
    'WikilinkEmbed',
  ],

  parseInline: [
    {
      name: 'Wikilink',
      before: 'Link',

      parse(cx: InlineContext, next: number, pos: number): number {
        // `open` is the offset of the first `[`; for an embed the node still
        // STARTS at the `!`, so `WikilinkEmbed.from` is the same offset the
        // old `Image` mis-parse reported and no consumer span shifts.
        let open = pos;
        let embed = false;
        if (next === BANG) {
          // `![[` — claim it before Image/Link are offered this `!`.
          if (cx.char(pos + 1) !== OPEN_BRACKET || cx.char(pos + 2) !== OPEN_BRACKET) return -1;
          embed = true;
          open = pos + 1;
        } else if (next === OPEN_BRACKET) {
          if (cx.char(pos + 1) !== OPEN_BRACKET) return -1;
        } else {
          return -1;
        }

        // Scan forward for the closing `]]`, bailing on newlines.
        // open+2 skips past the opening `[[`.
        const end = cx.end;
        let i = open + 2;
        while (i < end) {
          const ch = cx.char(i);
          if (ch === NEWLINE) return -1; // crossed line boundary — not a wikilink
          if (ch === CLOSE_BRACKET && cx.char(i + 1) === CLOSE_BRACKET) {
            // Found closing `]]` at i..i+2.
            const wikilinkEnd = i + 2;

            // Content between `[[` and `]]`
            const contentFrom = open + 2;
            const contentTo = i;
            const rawContent = cx.slice(contentFrom, contentTo);

            // Split on the first unescaped `|` to separate target from label.
            const pipeIdx = rawContent.indexOf('|');
            const targetTo = pipeIdx === -1
              ? contentTo
              : contentFrom + pipeIdx;

            const children = [
              // From `pos`, so the opening mark covers `![[` for an embed.
              cx.elt('WikilinkMark', pos, contentFrom),
              cx.elt('WikilinkTarget', contentFrom, targetTo), // target text
              cx.elt('WikilinkMark', i, wikilinkEnd),          // ]]
            ];

            cx.addElement(cx.elt(embed ? 'WikilinkEmbed' : 'Wikilink', pos, wikilinkEnd, children));
            return wikilinkEnd;
          }
          i++;
        }

        // No closing `]]` found before end-of-section.
        return -1;
      },
    },
  ],
};
