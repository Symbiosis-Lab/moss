// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { editorLivePreviewField } from "obsidian";
import { contextChanged, type NoteOf, type SiteNote } from "../src/site-context";
import { blockExtension } from "../src/syntax/cm-blocks";
import { shortcodeExtension } from "../src/syntax/cm-shortcode";

const views: EditorView[] = [];
afterEach(() => views.splice(0).forEach((view) => view.destroy()));

const sameSite = {};
const note = (site: object = sameSite) => ({ root: "/", path: "note.md", site, schema: { fields: new Map() } }) as unknown as SiteNote;

function editor(doc: string, noteOf: NoteOf = () => note()): EditorView {
  const renderer = (markdown: string, _doc: Document, note: SiteNote) => ({ sheet: {} as CSSStyleSheet, html: `<p>${markdown.length} ${(note.site as { name?: string }).name}</p>` });
  const view = new EditorView({
    state: EditorState.create({ doc, extensions: [editorLivePreviewField, blockExtension(renderer, noteOf)] }),
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

it("leaves the blocks of a note outside every moss folder as written, until a moss folder holds it", () => {
  let inSite = false;
  const view = editor("intro\n\n:::grid 2\none\n+++\ntwo\n:::\n", () => (inSite ? note() : null));
  expect(widget(view)).toBeNull();
  expect(view.dom.querySelector("[data-moss-block]")).toBeNull();
  inSite = true;
  view.dispatch({ effects: contextChanged.of() });
  expect(widget(view)).not.toBeNull();
});

it("marks no shortcode lines in a note outside every moss folder, until a moss folder holds it", () => {
  let inSite = false;
  const view = new EditorView({ state: EditorState.create({ doc: ":::grid 2\none\n:::\n", extensions: [shortcodeExtension(() => (inSite ? note() : null))] }), parent: document.body });
  views.push(view);
  expect(view.dom.querySelector(".moss-sc-open")).toBeNull();
  inSite = true;
  view.dispatch({ effects: contextChanged.of() });
  expect(view.dom.querySelector(".moss-sc-open")).not.toBeNull();
});

it("draws a block again once its site is read again", () => {
  let site = { name: "old" };
  const view = editor("intro\n\n:::grid 2\none\n:::\n", () => note(site));
  expect(widget(view)!.shadowRoot!.innerHTML).toContain("old");
  site = { name: "new" };
  view.dispatch({ effects: contextChanged.of() });
  expect(widget(view)!.shadowRoot!.innerHTML).toContain("new");
});
