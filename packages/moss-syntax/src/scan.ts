/**
 * Text-driven shortcode block scanner — the parse over a plain string that
 * hosts without a `@lezer/markdown` seam need (Obsidian's markdown language
 * is closed to grammar extensions; see
 * packages/obsidian-moss/src/syntax/shortcode-parser.ts, the stub this
 * replaces).
 *
 * The line predicates are REUSED from ./shortcode.ts, not copied: an open
 * fence is exactly `SHORTCODE_OPEN_RE` + `isOpenMatch`, a close fence is
 * exactly `isCloseFence`, and open↔close pairing uses the same arity stack as
 * the editor's tree walk (`collectShortcodeBlocks` in
 * ./cm6/cm-shortcode-block.ts): a close fence of arity N closes
 * the TOPMOST open of arity N (`::::` inside `:::` nests); opens above the
 * matched one never got a close and are reported as unclosed children.
 *
 * Two deliberate differences from the Lezer path, both on degenerate input:
 *
 * - **Unclosed blocks are reported, not dropped.** `collectShortcodeBlocks`
 *   discards an open that never closes (a decoration must not half-render);
 *   a host linting while the author is still typing needs the block BEFORE
 *   its close fence exists, so here it comes back with `closeFrom: null` and
 *   `to` at the end of its last body line.
 * - **No markdown block context.** A pure line scan cannot know a `:::`
 *   sits inside a fenced code block or blockquote; the grammar (which runs
 *   inside a real markdown parse) does. On documents without those wrappers
 *   the two agree exactly — `scan.test.ts` cross-checks against the Lezer
 *   parse over the shared structure corpus.
 *
 * CRLF is tolerated the same way the predicates tolerate it (they trim):
 * a trailing `\r` counts as line terminator, so `from`/`to` offsets never
 * include it.
 */

import { SHORTCODE_OPEN_RE, isOpenMatch, isCloseFence } from './shortcode.js';
import { SHORTCODES } from './contract/shortcodes.generated.js';

/** One `:::name … :::` block found in a text. Offsets are absolute character
 *  offsets into the string passed to `scanShortcodeBlocks`. */
export interface ShortcodeBlock {
  /** Offset of the open-fence line start. */
  from: number;
  /** Offset of the close-fence line end (== last body offset when unclosed). */
  to: number;
  name: string;
  /** Attrs text after the name, trimmed (`{cols=3}`, `2`, `''` when none). */
  attrs: string;
  /** Colon count of the fence (3 for `:::`, 4 for `::::`, …). */
  arity: number;
  /** Offset of the close-fence line start; null when unclosed. */
  closeFrom: number | null;
  children: ShortcodeBlock[];
}

interface OpenMarker {
  from: number;
  arity: number;
  name: string;
  attrs: string;
  children: ShortcodeBlock[];
}

/** Scan `text` for shortcode blocks. Returns the TOP-LEVEL blocks; each
 *  carries its nested `children` (document order, parent before children). */
export function scanShortcodeBlocks(text: string): ShortcodeBlock[] {
  const stack: OpenMarker[] = [];
  const out: ShortcodeBlock[] = [];
  /** End offset of the most recently scanned line (excludes `\r`/`\n`). */
  let prevLineEnd = 0;

  /** An open that never got its close: report it, don't drop it. */
  const unclosed = (o: OpenMarker, lastEnd: number): ShortcodeBlock => ({
    from: o.from,
    to: lastEnd,
    name: o.name,
    attrs: o.attrs,
    arity: o.arity,
    closeFrom: null,
    children: o.children,
  });

  let lineFrom = 0;
  const n = text.length;
  while (lineFrom <= n) {
    if (lineFrom === n && n > 0 && text[n - 1] === '\n') break; // no phantom last line
    let nl = text.indexOf('\n', lineFrom);
    if (nl === -1) nl = n;
    const hasCr = nl > lineFrom && text[nl - 1] === '\r';
    const lineEnd = hasCr ? nl - 1 : nl;
    const line = text.slice(lineFrom, lineEnd);

    const m = SHORTCODE_OPEN_RE.exec(line);
    if (isOpenMatch(m)) {
      stack.push({
        from: lineFrom,
        arity: m![2].length,
        name: m![3] ?? '',
        attrs: m![4].trim(),
        children: [],
      });
    } else if (isCloseFence(line)) {
      const arity = line.trim().length;
      for (let i = stack.length - 1; i >= 0; i--) {
        if (stack[i].arity === arity) {
          const removed = stack.splice(i);
          const open = removed[0];
          // Opens between the matched one and its close never closed: fold
          // them into a chain (each opened inside the previous one's body)
          // ending as children of the matched block.
          for (let j = removed.length - 1; j >= 1; j--) {
            removed[j - 1].children.push(unclosed(removed[j], prevLineEnd));
          }
          const block: ShortcodeBlock = {
            from: open.from,
            to: lineEnd,
            name: open.name,
            attrs: open.attrs,
            arity: open.arity,
            closeFrom: lineFrom,
            children: open.children,
          };
          if (stack.length > 0) stack[stack.length - 1].children.push(block);
          else out.push(block);
          break;
        }
        // No open of this arity → the close line is ordinary text (matches
        // the tree walk, which ignores an unpaired ShortcodeCloseLine).
      }
    }

    prevLineEnd = lineEnd;
    if (nl >= n) break;
    lineFrom = nl + 1;
  }

  // Whatever is still open at EOF: fold bottom-up (each later open sits in
  // the previous one's body); the bottom-most is top-level.
  for (let j = stack.length - 1; j >= 1; j--) {
    stack[j - 1].children.push(unclosed(stack[j], prevLineEnd));
  }
  if (stack.length > 0) out.push(unclosed(stack[0], prevLineEnd));

  return out;
}

/** A known shortcode name, with a one-line doc for diagnostics/completion. */
export interface ShortcodeSpec {
  name: string;
  doc: string;
}

/** Flatten CM6 snippet tab-stops (`${1:photo.jpg}` → `photo.jpg`). */
const flattenTemplate = (t: string): string => t.replace(/\$\{\d+:([^}]*)\}/g, '$1');

/**
 * The authorable shortcode vocabulary, derived from the generated catalog
 * (Rust SSOT: crates/moss-core/src/contract/shortcodes.rs). Non-authorable
 * kinds (`apply`) parse and render but are never offered to authors, so they
 * are excluded here — this list exists for unknown-name diagnostics and
 * completion, both author-facing.
 */
export const KNOWN_SHORTCODES: ReadonlyArray<ShortcodeSpec> = SHORTCODES
  .filter((s) => s.authorable)
  .map((s) => ({
    name: s.name,
    doc: `:::${flattenTemplate(s.canonicalTemplate).split('\n')[0]}`,
  }));
