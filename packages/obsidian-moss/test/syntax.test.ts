// The plugin's half of the shared-parser contract: scanner output → CM6
// decorations. The scanner itself is tested in @symbiosis-lab/moss-syntax;
// what matters here is that the mapping survives the shapes it really emits
// (unclosed, nested, nameless, indented) and that names are judged against the
// generated vocabulary rather than a local list.
import { describe, expect, it } from "vitest";
import { shortcodeDecoRanges, shortcodeExtension } from "../src/syntax/cm-shortcode";
import { KNOWN_SHORTCODES, scanShortcodeBlocks } from "../src/syntax/shortcode-parser";

/** Decorate a document the way the ViewPlugin does, in one step. */
const decorate = (doc: string) => shortcodeDecoRanges(scanShortcodeBlocks(doc), doc);

describe("shortcode vocabulary", () => {
  it("comes from the shared package, not a local list", () => {
    const names = KNOWN_SHORTCODES.map((s) => s.name);
    expect(names).toContain("grid");
    expect(names).toContain("hero");
    expect(names).toContain("gallery");
    // `apply` is non-authorable in the Rust catalog, so authors never see it.
    expect(names).not.toContain("apply");
  });

  it("carries a one-line doc for each name", () => {
    for (const spec of KNOWN_SHORTCODES) {
      expect(spec.doc.startsWith(`:::${spec.name}`)).toBe(true);
      expect(spec.doc).not.toContain("\n");
    }
  });
});

describe("shortcode decorations", () => {
  it("marks the fences of a closed, known block and flags nothing", () => {
    const doc = ":::grid {cols=2}\ncell\n:::\n";
    const ranges = decorate(doc);
    expect(ranges).toContainEqual({ from: 0, to: 0, line: true, cls: "moss-sc-open" });
    expect(doc.slice(22, 25)).toBe(":::");
    expect(ranges).toContainEqual({ from: 22, to: 22, line: true, cls: "moss-sc-close" });
    expect(ranges.some((r) => r.cls === "moss-sc-unknown")).toBe(false);
    expect(ranges.some((r) => r.cls === "moss-sc-unclosed")).toBe(false);
  });

  it("underlines a name the catalog does not know", () => {
    const doc = ":::wat\nx\n:::\n";
    const ranges = decorate(doc);
    expect(doc.slice(3, 6)).toBe("wat");
    expect(ranges).toContainEqual({ from: 3, to: 6, line: false, cls: "moss-sc-unknown" });
  });

  it("steps over indentation when locating the name", () => {
    const doc = "  :::grid\nx\n  :::\n";
    const ranges = decorate(doc);
    expect(doc.slice(5, 9)).toBe("grid");
    // `grid` is known, so the offset is proved by the close fence landing at
    // the indented line's START, not its colons.
    expect(ranges).toContainEqual({ from: 12, to: 12, line: true, cls: "moss-sc-close" });
    expect(ranges.some((r) => r.cls === "moss-sc-unknown")).toBe(false);
  });

  it("flags an unclosed block without inventing a close fence", () => {
    const doc = ":::grid\nstill typing\n";
    const blocks = scanShortcodeBlocks(doc);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].closeFrom).toBeNull();

    const ranges = decorate(doc);
    expect(ranges).toContainEqual({ from: 0, to: 0, line: true, cls: "moss-sc-open" });
    expect(ranges).toContainEqual({ from: 0, to: 0, line: true, cls: "moss-sc-unclosed" });
    expect(ranges.some((r) => r.cls === "moss-sc-close")).toBe(false);
  });

  it("descends into nested blocks", () => {
    const doc = ":::grid\n::::hero {image=a.jpg}\ninner\n::::\n:::\n";
    const blocks = scanShortcodeBlocks(doc);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].children).toHaveLength(1);

    const open = decorate(doc).filter((r) => r.cls === "moss-sc-open");
    expect(open).toHaveLength(2); // outer + nested, not just the top level
    expect(open.map((r) => r.from)).toEqual([0, 8]);
  });

  it("flags an unclosed child inside a closed parent", () => {
    const doc = ":::grid\n::::hero\norphan\n:::\n";
    const ranges = decorate(doc);
    expect(ranges).toContainEqual({ from: 0, to: 0, line: true, cls: "moss-sc-open" });
    expect(ranges).toContainEqual({ from: 8, to: 8, line: true, cls: "moss-sc-unclosed" });
  });

  it("judges nothing on a nameless fence", () => {
    const doc = ":::{cols=2}\nx\n:::\n";
    expect(scanShortcodeBlocks(doc)[0].name).toBe("");
    expect(decorate(doc).some((r) => r.cls === "moss-sc-unknown")).toBe(false);
  });

  it("finds nothing in prose", () => {
    expect(decorate("just some text\nwith :: colons\n")).toEqual([]);
  });

  it("emits ranges in document order, line decorations first", () => {
    const ranges = decorate(":::wat\nx\n:::\n");
    const froms = ranges.map((r) => r.from);
    expect([...froms].sort((a, b) => a - b)).toEqual(froms);
  });
});

describe("shortcode extension", () => {
  it("is registered as a real extension now that the parser is wired", () => {
    const ext = shortcodeExtension();
    expect(ext).not.toEqual([]);
    expect(ext).toBeTruthy();
  });
});
