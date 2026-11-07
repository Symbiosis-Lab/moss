import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import {
  scanShortcodeBlocks,
  type ShortcodeBlock,
} from "./shortcode-parser";

interface DecoRange {
  from: number;
  to: number;
  line: boolean;
  cls: string;
}

export function shortcodeDecoRanges(blocks: ShortcodeBlock[]): DecoRange[] {
  const out: DecoRange[] = [];
  const walk = (block: ShortcodeBlock) => {
    out.push({ from: block.from, to: block.from, line: true, cls: "moss-sc-open" });
    if (block.closeFrom !== null) {
      out.push({ from: block.closeFrom, to: block.closeFrom, line: true, cls: "moss-sc-close" });
    }
    block.children.forEach(walk);
  };
  blocks.forEach(walk);
  return out.sort((a, b) => a.from - b.from || Number(b.line) - Number(a.line));
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
        return toDecorationSet(shortcodeDecoRanges(scanShortcodeBlocks(text)));
      }
    },
    { decorations: (v) => v.decorations },
  );
}
