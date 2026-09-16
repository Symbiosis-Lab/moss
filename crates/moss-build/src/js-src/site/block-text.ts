/**
 * Prose extraction from a DOM block, with a raw→clean offset map.
 *
 * ## What "clean" means
 *
 * The text a reader sees, not the text the markup contains. Two edits get it
 * there, and both are what the browser itself does when it lays the block out:
 *
 * - **A `<br>` becomes a newline.** It is the only thing in a block that means
 *   "start a new line", and it is an *element*, so a text-node walk cannot see
 *   it. That is why a shared poem printed as one run-on paragraph: moss renders
 *   a single newline as `<br />` by default (`[site].hard_line_breaks`, Obsidian
 *   parity), and every break in the poem was dropped here before the card ever
 *   got a chance to draw it.
 * - **Source whitespace collapses.** A run of spaces, tabs and newlines in the
 *   markup renders as one space, and whitespace at the start of a line renders
 *   as nothing. Without this the *other* newline — the one pulldown-cmark writes
 *   after `<br />` for legibility — would arrive as a second break, and a
 *   paragraph the author wrapped across two source lines (CommonMark mode, where
 *   that newline is a space) would arrive broken in half.
 *
 * Together these mean one rule for everything downstream: **`\n` in this text is
 * a hard line break the author asked for, and nothing else is.**
 *
 * ## The offset map
 *
 * Raw offsets are positions in `block.textContent` — the space in which
 * `Range.toString()` arithmetic happens, and the space selection boundaries
 * arrive in. `toClean` translates them, so a boundary computed against the
 * markup lands in the right place in the reader's text no matter how much this
 * function inserted or removed on the way.
 *
 * ## Why this is not in share-card/text.ts
 *
 * It lived there until 2026-08-04 and it was the single reason the share-card
 * bundle could not be split out of `theme.js`. `selection-actions.ts` runs on
 * every page load (it wires the selection popover), so its *static* imports
 * are eager code by definition — and it imported `cleanBlockText` from
 * `share-card/text.ts`, dragging that module's 14 KB of canvas wrapping,
 * justification and CJK-spacing machinery into every page along with it.
 *
 * The dependency was never real. This module walks a DOM subtree and returns
 * strings; it shares nothing with the canvas text engine but the word "text".
 * Anything selection-actions needs at runtime belongs here for that reason —
 * which is why `splitIntoLines` lives here too, next to the function that
 * decides what a line is.
 *
 * The `CardSegment`/`CardBlock` types still come from `share-card/text.ts`,
 * and that is fine: they are `import type`, erased before esbuild sees them,
 * so they create no runtime edge.
 *
 * See `docs/archive/2026-08-04-ship-what-the-site-needs.md` §4 Milestone B and
 * `docs/archive/2026-08-10-share-card-audit.md`.
 */

import type { CardSegment, CardLine } from "./share-card/text";

// DOM text that is not prose: heading permalink anchors (the trailing "#"
// that rendered as "二、#" on real cards) and math SVG <title> elements,
// whose textContent is raw TeX source.
const JUNK_SELECTOR = "a.moss-heading-anchor, svg title";

/** Whitespace the CSS `white-space: normal` cascade collapses. NBSP is not in
 * here on purpose — an author who typed one meant it to survive. */
function isCollapsibleSpace(ch: string): boolean {
  return ch === " " || ch === "\n" || ch === "\t" || ch === "\r" || ch === "\f";
}

/**
 * A block's prose text with junk removed, `<br>` as `\n`, and source
 * whitespace collapsed — plus a raw→clean offset map.
 */
export function cleanBlockText(block: Element): {
  text: string;
  toClean: (raw: number) => number;
} {
  let text = "";
  let pendingSpace = false;
  // map[i] = the clean offset that raw offset i lands on. Built entry by entry
  // rather than as an edit list because the edits are no longer only deletions:
  // a <br> inserts, whitespace collapses, and junk deletes, sometimes all three
  // in one block. An array of one number per character is exact by construction
  // and needs no reasoning to trust — and a block is a paragraph, not a book.
  const map: number[] = [];

  // SHOW_TEXT | SHOW_ELEMENT: the walk has to see <br>, which is precisely
  // what the text-only walk could not.
  const walker = document.createTreeWalker(
    block,
    NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT
  );
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n.nodeType === Node.ELEMENT_NODE) {
      if ((n as Element).nodeName === "BR") {
        // A break swallows any space that was heading toward it, and any that
        // follows it — a line never starts or ends on one.
        pendingSpace = false;
        text += "\n";
      }
      continue;
    }
    const data = (n as Text).data;
    const isJunk = !!(n as Text).parentElement?.closest(JUNK_SELECTOR);
    // Indexed by UTF-16 code unit, not by code point: the raw offsets this map
    // answers come from `Range.toString().length`, which counts units. Iterating
    // code points (`for…of`) puts one entry where an emoji needs two, and every
    // offset after the first astral character in a block lands a character
    // early — a highlight that starts mid-word.
    for (let i = 0; i < data.length; i++) {
      const ch = data[i];
      if (isJunk) {
        map.push(text.length);
        continue;
      }
      if (isCollapsibleSpace(ch)) {
        map.push(text.length);
        // Nothing yet, or we just broke a line: a leading space renders as
        // nothing, so it must not become one here either.
        if (text && !text.endsWith("\n")) pendingSpace = true;
        continue;
      }
      if (pendingSpace) {
        text += " ";
        pendingSpace = false;
      }
      map.push(text.length);
      text += ch;
    }
  }
  // A trailing `pendingSpace` is deliberately never flushed: trailing
  // whitespace renders as nothing.
  map.push(text.length);

  const toClean = (raw: number): number => {
    if (raw <= 0) return 0;
    if (raw >= map.length) return text.length;
    return map[raw];
  };

  return { text, toClean };
}

/**
 * Split segments at hard line breaks, preserving which parts of each line the
 * reader had selected.
 *
 * `cleanBlockText` guarantees `\n` means a break the author asked for, so this
 * is the one place that turns that guarantee into structure. Everything
 * downstream — measuring, wrapping, drawing — works a line at a time and never
 * has to know that `\n` was ever a character.
 *
 * A block always yields at least one line, so a caller never has to special-case
 * the empty result.
 */
export function splitIntoLines(segments: CardSegment[]): CardLine[] {
  const lines: CardLine[] = [[]];
  for (const seg of segments) {
    const parts = seg.text.split("\n");
    for (let i = 0; i < parts.length; i++) {
      if (i > 0) lines.push([]);
      if (parts[i]) {
        lines[lines.length - 1].push({
          text: parts[i],
          highlighted: seg.highlighted,
        });
      }
    }
  }
  return lines;
}
