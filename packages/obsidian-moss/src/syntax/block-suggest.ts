import { snippet } from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import { KNOWN_SHORTCODES } from "@symbiosis-lab/moss-syntax";
import { EditorSuggest, type Editor, type EditorPosition, type EditorSuggestContext, type EditorSuggestTriggerInfo } from "obsidian";
import { BLOCKS } from "./blocks";

type Block = (typeof BLOCKS)[number];

export class BlockSuggest extends EditorSuggest<Block> {
  onTrigger(cursor: EditorPosition, editor: Editor): EditorSuggestTriggerInfo | null {
    const match = /^:::([a-z]*)$/.exec(editor.getLine(cursor.line).slice(0, cursor.ch));
    if (!match) return null;
    return { start: { line: cursor.line, ch: 0 }, end: cursor, query: match[1] };
  }

  getSuggestions(context: EditorSuggestContext): Block[] {
    return BLOCKS.filter((block) => block.name.startsWith(context.query));
  }

  renderSuggestion(block: Block, el: HTMLElement): void {
    el.createDiv({ text: KNOWN_SHORTCODES.find((spec) => spec.name === block.name)?.doc ?? `:::${block.name}`, cls: "moss-field-suggestion-name" });
  }

  /** Writes the block's template with its tab stops, as moss's own editor does; the first placeholder is selected. */
  selectSuggestion(block: Block): void {
    const context = this.context;
    // Obsidian exposes the CM view on Editor at runtime, outside its public types.
    const view = (context?.editor as (Editor & { cm?: EditorView }) | undefined)?.cm;
    if (!context || !view) return;
    const { editor, start, end } = context;
    snippet(`:::${block.canonicalTemplate}`)(view, { label: block.name }, editor.posToOffset(start), editor.posToOffset(end));
    this.close();
  }
}
