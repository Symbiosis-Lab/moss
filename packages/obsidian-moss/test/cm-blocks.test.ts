// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { editorLivePreviewField } from "obsidian";
import { blockExtension } from "../src/syntax/cm-blocks";

const views: EditorView[] = [];
afterEach(() => views.splice(0).forEach((view) => view.destroy()));

function editor(doc: string): EditorView {
  const renderer = (markdown: string) => ({ sheet: {} as CSSStyleSheet, html: `<p>${markdown.length}</p>` });
  const view = new EditorView({
    state: EditorState.create({ doc, extensions: [editorLivePreviewField, blockExtension(renderer)] }),
    parent: document.body,
  });
  views.push(view);
  return view;
}

const widget = (view: EditorView) => view.dom.querySelector<HTMLElement>(".moss-block-widget");

it("a click on a block moved by an edit above it puts the cursor at the block's new start", () => {
  const view = editor("intro\n\n:::grid 2\none\n+++\ntwo\n:::\n");
  const host = widget(view)!;
  view.dispatch({ changes: { from: 0, insert: "a new first line\n" }, selection: { anchor: 0 } });
  expect(widget(view)).toBe(host);

  host.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));

  expect(view.state.selection.main.head).toBe(view.state.doc.toString().indexOf(":::grid"));
});

it("a cursor in the block shows its source, and leaving it shows the render again", () => {
  const view = editor("intro\n\n:::grid 2\none\n+++\ntwo\n:::\n");
  expect(widget(view)).not.toBeNull();
  view.dispatch({ selection: { anchor: view.state.doc.toString().indexOf("one") } });
  expect(widget(view)).toBeNull();
  expect(view.dom.querySelector("[data-moss-block=grid]")).not.toBeNull();
  view.dispatch({ selection: { anchor: 0 } });
  expect(widget(view)).not.toBeNull();
});
