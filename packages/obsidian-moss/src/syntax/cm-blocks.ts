// `:::` blocks in Live Preview, behaving like Obsidian's callouts: a render while the cursor is
// elsewhere, the source with a label chip while it is inside.
import { editorLivePreviewField } from "obsidian";
import { RangeSetBuilder, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { contextChanged, type NoteOf, type SiteNote } from "../site-context";
import type { BlockRender } from "./block-render";
import { isEditing, topBlocks } from "./blocks";
import type { ShortcodeBlock } from "./shortcode-parser";

/** `doc` is the document the editor shows in; `note` is the editor's note as its site sees it. */
export type BlockRenderer = (markdown: string, doc: Document, note: SiteNote) => BlockRender | null;

class BlockWidget extends WidgetType {
  constructor(private markdown: string, private note: SiteNote, private renderer: BlockRenderer) { super(); }

  // A site read again renders the same Markdown differently.
  eq(other: BlockWidget): boolean { return other.markdown === this.markdown && other.note.site === this.note.site; }

  toDOM(view: EditorView): HTMLElement {
    const doc = view.dom.ownerDocument;
    const host = doc.createElement("div");
    host.className = "moss-block-widget";
    // A shadow root keeps the render's stylesheet from reaching the note, and the note's from reaching it.
    const root = host.attachShadow({ mode: "open" });
    const rendered = this.renderer(this.markdown, doc, this.note);
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
  note: SiteNote | null;
  blocks: ShortcodeBlock[];
  /** Which blocks the selection is in, one flag per block. */
  editing: string;
  decorations: DecorationSet;
}

const editingOf = (blocks: ShortcodeBlock[], state: EditorState) =>
  blocks.map((block) => (isEditing(block, state.selection.ranges) ? "1" : "0")).join("");

function decorate(state: EditorState, blocks: ShortcodeBlock[], note: SiteNote | null, renderer: BlockRenderer): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const block of blocks) {
    if (note && !isEditing(block, state.selection.ranges)) {
      builder.add(block.from, block.to, Decoration.replace({
        widget: new BlockWidget(state.doc.sliceString(block.from, block.to), note, renderer), block: true,
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

function scan(state: EditorState, renderer: BlockRenderer, noteOf: NoteOf): Blocks {
  const live = state.field(editorLivePreviewField);
  const note = live ? noteOf(state) : null;
  const blocks = note ? topBlocks(state.doc.toString()) : [];
  return { live, note, blocks, editing: editingOf(blocks, state), decorations: decorate(state, blocks, note, renderer) };
}

/** Renders the `:::` blocks of notes a moss folder holds. */
export function blockExtension(renderer: BlockRenderer, noteOf: NoteOf): Extension {
  return StateField.define<Blocks>({
    create: (state) => scan(state, renderer, noteOf),
    update: (value, tr) => {
      if (tr.docChanged || tr.reconfigured || tr.state.field(editorLivePreviewField) !== value.live
        || tr.effects.some((effect) => effect.is(contextChanged))) return scan(tr.state, renderer, noteOf);
      if (!tr.selection) return value;
      // A cursor move only swaps a render for its source, or back, when it enters or leaves a block.
      const editing = editingOf(value.blocks, tr.state);
      return editing === value.editing ? value : { ...value, editing, decorations: decorate(tr.state, value.blocks, value.note, renderer) };
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}
