// Obsidian-native completion for moss frontmatter field names.
//
// Uses `EditorSuggest` (Obsidian's own suggest UI) rather than CM6
// `autocompletion()` — Obsidian already drives its suggest popups outside
// the CM6 extension system, and a second autocompletion instance races it.
// The candidate logic is pure (cm-frontmatter.ts `fieldCompletions`).
import {
  Editor,
  EditorPosition,
  EditorSuggest,
  EditorSuggestContext,
  EditorSuggestTriggerInfo,
  TFile,
} from "obsidian";
import { fieldCompletions, type FieldCompletion, type SchemaProvider } from "./cm-frontmatter";
import { editorFieldSpans } from "./core";
import type MossPlugin from "./main";

export class MossFieldSuggest extends EditorSuggest<FieldCompletion> {
  constructor(
    plugin: MossPlugin,
    private getSchema: SchemaProvider,
  ) {
    super(plugin.app);
  }

  onTrigger(cursor: EditorPosition, editor: Editor, _file: TFile | null): EditorSuggestTriggerInfo | null {
    const doc = editor.getValue();
    const before = editor.getLine(cursor.line).slice(0, cursor.ch);
    const m = /^([A-Za-z_-]+)$/.exec(before);
    if (!m) return null;
    // Complete the partial key so the core can decide whether it is frontmatter.
    const offset = editor.posToOffset(cursor);
    const lineEnd = offset + editor.getLine(cursor.line).length - cursor.ch;
    const candidate = doc.slice(0, offset) + ": null" + doc.slice(lineEnd);
    if (!editorFieldSpans(candidate).some((field) => field.line === cursor.line + 1 && field.key === m[1])) return null;
    return {
      start: { line: cursor.line, ch: 0 },
      end: cursor,
      query: m[1],
    };
  }

  getSuggestions(context: EditorSuggestContext): FieldCompletion[] {
    const editor = context.editor;
    const doc = editor.getValue();
    const from = editor.posToOffset(context.start);
    const to = from + editor.getLine(context.start.line).length;
    // Omit the incomplete line while asking the core which keys already exist.
    return fieldCompletions(this.getSchema(), doc.slice(0, from) + doc.slice(to), context.query);
  }

  renderSuggestion(item: FieldCompletion, el: HTMLElement): void {
    el.addClass("moss-field-suggestion");
    el.createDiv({ text: `${item.name} (${item.type})`, cls: "moss-field-suggestion-name" });
    if (item.description) {
      el.createDiv({ text: item.description, cls: "moss-field-suggestion-desc" });
    }
  }

  selectSuggestion(item: FieldCompletion): void {
    const context = this.context;
    if (!context) return;
    context.editor.replaceRange(item.insert, context.start, context.end);
    const pos = { line: context.start.line, ch: item.insert.length };
    context.editor.setCursor(pos);
    this.close();
  }
}
