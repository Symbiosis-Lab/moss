// Shortcode decoration extension — live.
//
// Blocks come from `@symbiosis-lab/moss-syntax` (via syntax/shortcode-parser),
// the same scanner moss's own editor uses, so what Obsidian highlights and what
// moss builds can never disagree about what a fence is.
//
// One known gap, inherited from the text-driven scan and documented there: a
// pure line scan has no markdown block context, so a `:::` written inside a
// fenced code block is still seen as a fence. Teaching this file otherwise
// would put a second opinion about markdown structure in the plugin, which is
// exactly what the shared package exists to prevent.
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import {
  KNOWN_SHORTCODES,
  scanShortcodeBlocks,
  type ShortcodeBlock,
} from "./shortcode-parser";

interface DecoRange {
  from: number;
  to: number;
  line: boolean;
  cls: string;
}

/**
 * Pure: blocks → decoration ranges. Line decorations for the fences (styled
 * like moss's own editor: tag the open line, fade the close line) and a mark
 * on unknown names. Exported so the mapping is testable without a view.
 *
 * Three shapes the scanner really produces, each handled here:
 *
 * - **Unclosed** (`closeFrom: null`) — the block is still marked on its open
 *   line, plus `moss-sc-unclosed`; nothing is emitted for a close fence that
 *   does not exist. Authors see the flag while they are still typing.
 * - **Nested** — children hang off their parent, so the walk recurses instead
 *   of assuming a flat list.
 * - **Nameless** (`:::{...}`) — no name to judge, so no unknown-name mark.
 *
 * Offsets are absolute and line-start based, which is what `Decoration.line`
 * requires; the name mark steps over the fence's indent and colons.
 */
export function shortcodeDecoRanges(blocks: ShortcodeBlock[], docText: string): DecoRange[] {
  const known = new Set(KNOWN_SHORTCODES.map((s) => s.name));
  const out: DecoRange[] = [];
  const walk = (block: ShortcodeBlock) => {
    out.push({ from: block.from, to: block.from, line: true, cls: "moss-sc-open" });
    if (block.closeFrom !== null) {
      out.push({ from: block.closeFrom, to: block.closeFrom, line: true, cls: "moss-sc-close" });
    } else {
      out.push({ from: block.from, to: block.from, line: true, cls: "moss-sc-unclosed" });
    }
    if (block.name !== "" && !known.has(block.name)) {
      const nameFrom = block.from + block.arity + leadingIndent(docText, block.from);
      out.push({ from: nameFrom, to: nameFrom + block.name.length, line: false, cls: "moss-sc-unknown" });
    }
    block.children.forEach(walk);
  };
  blocks.forEach(walk);
  return out.sort((a, b) => a.from - b.from || Number(b.line) - Number(a.line));
}

function leadingIndent(docText: string, lineFrom: number): number {
  let n = 0;
  while (lineFrom + n < docText.length && (docText[lineFrom + n] === " " || docText[lineFrom + n] === "\t")) n++;
  return n;
}

function toDecorationSet(ranges: DecoRange[]): DecorationSet {
  return Decoration.set(
    ranges.map((r) =>
      r.line
        ? Decoration.line({ class: r.cls }).range(r.from)
        : Decoration.mark({ class: r.cls }).range(r.from, r.to),
    ),
    true,
  );
}

export function shortcodeExtension(): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = this.build(view);
      }
      update(u: ViewUpdate) {
        if (u.docChanged) this.decorations = this.build(u.view);
      }
      private build(view: EditorView): DecorationSet {
        const text = view.state.doc.toString();
        return toDecorationSet(shortcodeDecoRanges(scanShortcodeBlocks(text), text));
      }
    },
    { decorations: (v) => v.decorations },
  );
}
