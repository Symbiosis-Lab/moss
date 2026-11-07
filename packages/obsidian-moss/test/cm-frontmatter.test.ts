import { describe, expect, it } from "vitest";
import { diagnosticsToDecorations, fieldCompletions, fieldInfoText } from "../src/cm-frontmatter";
import { initializeCore, MossSite, type EditorDiagnostic } from "../src/core";
import { composedSchema, type SitePlugin } from "../src/schema";

initializeCore();
const site = new MossSite(JSON.stringify({ files: ["note.md"], lang: "en" }));
const schema = composedSchema();

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
      { severity: 3, message: "info", line: 2 },
      { severity: 4, message: "hint", line: 3 },
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
    const plugins: SitePlugin[] = [{ name: "sample", fields: {
      title: { type: "boolean", description: "Must not replace title" },
      review_score: { type: "integer", description: "Reader review_score" },
    } }, { name: "second", fields: { review_score: { type: "string" } } }];
    const composed = composedSchema(plugins, ["board"]);
    expect(composed.fields.get("title")?.type).toBe("string");
    expect(fieldInfoText(composed, "review_score")).toContain("Reader review_score");
    expect(composed.fields.get("children_style")?.enumValues).toContain("board");
    const contributed = new MossSite(JSON.stringify({ files: [], lang: "en", plugins, listing_styles: ["board"] }));
    expect(JSON.parse(contributed.check("page.md", "---\ntitle: Example\nreview_score: 3\nchildren_style: board\n---\n"))).toEqual([]);
    contributed.free();
  });
});
