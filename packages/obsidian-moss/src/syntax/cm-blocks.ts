// `:::` blocks in Live Preview, behaving like Obsidian's callouts: a render while the cursor is
// elsewhere, the source with a label chip while it is inside.
import { editorLivePreviewField } from "obsidian";
import { RangeSetBuilder, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import type { BlockRender } from "./block-render";
import { isEditing, topBlocks } from "./blocks";
import type { ShortcodeBlock } from "./shortcode-parser";

/** `doc` is the document the editor shows in. */
export type BlockRenderer = (markdown: string, doc: Document) => BlockRender | null;

class BlockWidget extends WidgetType {
  constructor(private markdown: string, private renderer: BlockRenderer) { super(); }

  eq(other: BlockWidget): boolean { return other.markdown === this.markdown; }

  toDOM(view: EditorView): HTMLElement {
    const doc = view.dom.ownerDocument;
    const host = doc.createElement("div");
    host.className = "moss-block-widget";
    // A shadow root keeps the render's stylesheet from reaching the note, and the note's from reaching it.
    const root = host.attachShadow({ mode: "open" });
    const rendered = this.renderer(this.markdown, doc);
    if (rendered) {
      root.adoptedStyleSheets = [rendered.sheet];
      root.innerHTML = rendered.html;
    }
    host.addEventListener("mousedown", (event) => {
      event.preventDefault();
      // An edit above moves the block but keeps this DOM, so its place is read now.
      view.dispatch({ selection: { anchor: view.posAtDOM(host) }, scrollIntoView: true });
      view.focus();
    });
    return host;
  }

  ignoreEvent(): boolean { return true; }
}

interface Blocks {
  live: boolean;
  blocks: ShortcodeBlock[];
  /** Which blocks the selection is in, one flag per block. */
  editing: string;
  decorations: DecorationSet;
}

const editingOf = (blocks: ShortcodeBlock[], state: EditorState) =>
  blocks.map((block) => (isEditing(block, state.selection.ranges) ? "1" : "0")).join("");

function decorate(state: EditorState, blocks: ShortcodeBlock[], renderer: BlockRenderer): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const block of blocks) {
    if (!isEditing(block, state.selection.ranges)) {
      builder.add(block.from, block.to, Decoration.replace({
        widget: new BlockWidget(state.doc.sliceString(block.from, block.to), renderer), block: true,
      }));
      continue;
    }
    const open = state.doc.lineAt(block.from);
    builder.add(open.from, open.from, Decoration.line({
      class: "moss-block-open", attributes: { "data-moss-block": block.name },
    }));
  }
  return builder.finish();
}

function scan(state: EditorState, renderer: BlockRenderer): Blocks {
  const live = state.field(editorLivePreviewField);
  const blocks = live ? topBlocks(state.doc.toString()) : [];
  return { live, blocks, editing: editingOf(blocks, state), decorations: decorate(state, blocks, renderer) };
}

export function blockExtension(renderer: BlockRenderer): Extension {
  return StateField.define<Blocks>({
    create: (state) => scan(state, renderer),
    update: (value, tr) => {
      if (tr.docChanged || tr.reconfigured || tr.state.field(editorLivePreviewField) !== value.live) return scan(tr.state, renderer);
      if (!tr.selection) return value;
      // A cursor move only swaps a render for its source, or back, when it enters or leaves a block.
      const editing = editingOf(value.blocks, tr.state);
      return editing === value.editing ? value : { ...value, editing, decorations: decorate(tr.state, value.blocks, renderer) };
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}
