import type { Editor, EditorSuggestContext } from "obsidian";
import { MossFieldSuggest } from "../src/field-suggest";
import type MossPlugin from "../src/main";
import { describe, expect, it } from "vitest";
import { diagnosticsToDecorations, fieldCompletions, fieldInfoText } from "../src/cm-frontmatter";
import { editorFieldSpans, initializeCore, MossSite, type EditorDiagnostic } from "../src/core";
import { siteSchema, type SitePlugin } from "../src/schema";

initializeCore();
const site = new MossSite(JSON.stringify({ files: ["note.md"], lang: "en" }));
const schema = siteSchema(site);

function decorations(text: string, diagnostics: EditorDiagnostic[]) {
  const result = [];
  const cursor = diagnosticsToDecorations(text, diagnostics).iter();
  while (cursor.value) {
    result.push({ from: cursor.from, to: cursor.to, cls: cursor.value.spec.class });
    cursor.next();
  }
  return result;
}

describe("core diagnostics in CodeMirror", () => {
  it("marks top-level keys and decorates body lines using real wasm", () => {
    const text = '---\ntitel: x\ndraft: "yes"\n---\n\n[[missing]]\n';
    const diagnostics: EditorDiagnostic[] = JSON.parse(site.check("note.md", text));
    const ranges = decorations(text, diagnostics);
    expect(ranges).toContainEqual({ from: 4, to: 9, cls: "moss-diag-hint" });
    const draft = ranges.find((range) => text.slice(range.from, range.to) === "draft");
    expect(draft?.cls).toBe("moss-diag-error");
    const link = text.indexOf("[[missing]]");
    expect(ranges).toContainEqual({ from: link, to: link, cls: "moss-diag-warning" });
  });

  it("uses hint styling for both information and hint severity, including empty lines", () => {
    const text = "first\n\nlast";
    expect(decorations(text, [
      { severity: 3, message: "info", line: 2, path: null, column: null },
      { severity: 4, message: "hint", line: 3, path: null, column: null },
    ])).toEqual([
      { from: 6, to: 6, cls: "moss-diag-hint" },
      { from: 7, to: 7, cls: "moss-diag-hint" },
    ]);
  });

  it("checks body shortcodes even without frontmatter", () => {
    const text = ":::nonesuch\nhello\n:::\n";
    const diagnostics = JSON.parse(site.check("note.md", text));
    expect(diagnostics.length).toBeGreaterThan(0);
    expect(decorations(text, diagnostics).some((range) => range.from === range.to)).toBe(true);
  });
});

describe("schema descriptions and completion", () => {
  it("reads built-in descriptions from wasm", () => {
    expect(fieldInfoText(schema, "draft")).toContain("draft: boolean");
    expect(fieldInfoText(schema, "zzqy")).toBeNull();
    expect(fieldCompletions(schema, "---\n\n---\n", "ti").find((field) => field.name === "title")?.insert).toBe("title: ");
    expect(fieldCompletions(schema, "---\ntitle: x\n---\n", "").map((field) => field.name)).not.toContain("title");
  });

  it("matches the core's additive plugin composition", () => {
    const plugins: SitePlugin[] = [{ name: "z-last", fields: { review_score: { type: "string" } } },
      { name: "a-first", fields: {
        title: { type: "boolean", description: "Must not replace title" },
        review_score: { type: "integer", description: "Reader review score" },
      } }];
    const contributed = new MossSite(JSON.stringify({ files: [], lang: "en", plugins, listing_styles: ["board"] }));
    const composed = siteSchema(contributed);
    expect(composed.fields.get("title")?.type).toBe("string");
    expect(fieldInfoText(composed, "review_score")).toContain("Reader review score");
    expect(fieldCompletions(composed, "---\n\n---\n", "review")[0].type).toBe("integer");
    expect(composed.fields.get("children_style")?.enumValues).toContain("board");
    expect(JSON.parse(contributed.schemaJson()).frontmatter.fields.review_score.source).toBe("a-first");
    expect(JSON.parse(contributed.check("page.md", "---\ntitle: Example\nreview_score: 3\nchildren_style: board\n---\n"))).toEqual([]);
    const text = "---\nreview_score: invalid\n---\n";
    expect(decorations(text, JSON.parse(contributed.check("page.md", text))))
      .toContainEqual({ from: 4, to: 16, cls: "moss-diag-error" });
    contributed.free();
  });
});

it.each([
  ['---\ntitle: café 🌿\n"draft": wrong\n---\n', '"draft"'],
  ['\uFEFFtitle: café 🌿\r\ndraft: wrong\r\n---\r\n', 'draft'],
])("uses canonical fields and UTF-16 offsets for %s", (text, spelling) => {
  const field = editorFieldSpans(text).find((field) => field.key === "draft")!;
  expect(field.from).toBe(text.indexOf(spelling));
  expect(text.slice(field.from, field.to)).toBe(spelling);
  expect(decorations(text, JSON.parse(site.check("note.md", text))))
    .toContainEqual({ from: field.from, to: field.to, cls: "moss-diag-error" });
  expect(fieldInfoText(schema, field.key)).toContain("draft: boolean");
  expect(fieldCompletions(schema, text, "").map((field) => field.name)).not.toContain("draft");
});

it.each([
  ['---\n"title": café 🌿\nti\n---\n', 2, true],
  ['title: café 🌿\nti\n---\n', 1, true],
  ['# Body\nti\n---\n', 1, false],
])("triggers completion using canonical frontmatter boundaries for %s", (text, line, inside) => {
  const lines = text.split("\n");
  const editor = {
    getValue: () => text,
    getLine: (line: number) => lines[line],
    posToOffset: ({ line, ch }: { line: number; ch: number }) => lines.slice(0, line).reduce((n, text) => n + text.length + 1, 0) + ch,
  } as Editor;
  const suggest = new MossFieldSuggest({ app: {} } as MossPlugin, () => schema);
  const trigger = suggest.onTrigger({ line, ch: 2 }, editor, null);
  expect(trigger !== null).toBe(inside);
  if (trigger) {
    const suggestions = suggest.getSuggestions({ ...trigger, editor } as EditorSuggestContext);
    expect(suggestions.map((field) => field.name)).not.toContain("title");
  }
});
