import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { BlockSuggest } from "../src/syntax/block-suggest";
import { BLOCKS, isEditing, topBlocks } from "../src/syntax/blocks";

const doc = "intro\n\n:::grid\n![a](a.png)\n:::\n\n:::hero\nunclosed\n";

describe("BLOCKS", () => {
  it("are moss's authorable catalog", () => {
    const names = BLOCKS.map((b) => b.name);
    expect(names).toEqual(expect.arrayContaining(["grid", "hero", "gallery", "buttons", "form", "recent"]));
    expect(names).not.toContain("tagline");
    expect(names).not.toContain("apply");
  });
});

describe("topBlocks", () => {
  it("lists closed blocks moss renders only", () => {
    expect(topBlocks(doc).map((b) => b.name)).toEqual(["grid"]);
    expect(topBlocks(":::recent {count=5}\nNo posts yet.\n:::\n:::tagline\nHello\n:::\n").map((b) => b.name)).toEqual(["recent"]);
  });
});

describe("isEditing", () => {
  const [block] = topBlocks(doc);
  it("is true while the selection touches the block, false elsewhere", () => {
    expect(isEditing(block, [{ from: block.from + 3, to: block.from + 3 }])).toBe(true);
    expect(isEditing(block, [{ from: block.to, to: block.to }])).toBe(true);
    expect(isEditing(block, [{ from: 0, to: 0 }])).toBe(false);
    expect(isEditing(block, [{ from: block.to + 1, to: block.to + 1 }])).toBe(false);
  });
});

describe("BlockSuggest", () => {
  /** Obsidian's Editor over a CodeMirror state, the way the app exposes `editor.cm`. */
  function editorOver(text: string) {
    const cm = {
      state: EditorState.create({ doc: text }),
      dispatch(tr: { state: EditorState } | object) { cm.state = "state" in tr ? tr.state : cm.state.update(tr).state; },
    };
    const line = (n: number) => cm.state.doc.line(n + 1);
    return {
      cm,
      getLine: (n: number) => line(n).text,
      posToOffset: (pos: { line: number; ch: number }) => line(pos.line).from + pos.ch,
    };
  }

  it("offers moss's blocks for the typed name and writes the chosen one's template, first placeholder selected", () => {
    const editor = editorOver("intro\n:::gr");
    const suggest = new BlockSuggest({} as never);
    const context = { ...suggest.onTrigger({ line: 1, ch: 5 }, editor as never)!, editor };
    expect(context.query).toBe("gr");
    const [grid] = suggest.getSuggestions(context as never);
    expect(grid.name).toBe("grid");
    (suggest as unknown as { context: unknown }).context = context;
    suggest.selectSuggestion(grid);
    const { doc, selection } = editor.cm.state;
    expect(doc.toString()).toBe("intro\n:::grid 2\ncell one\n+++\ncell two\n:::");
    expect(doc.sliceString(selection.main.from, selection.main.to)).toBe("2");
  });
});
