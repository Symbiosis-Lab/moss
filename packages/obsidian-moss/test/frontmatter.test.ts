import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  nearestFieldName,
  parseFrontmatterBlock,
  validateFrontmatter,
} from "../src/frontmatter";
import { parseDescribeJson } from "../src/schema";

const SCHEMA = parseDescribeJson(
  readFileSync(join(__dirname, "../src/describe-snapshot.json"), "utf8"),
);

function diags(doc: string) {
  const block = parseFrontmatterBlock(doc);
  expect(block).not.toBeNull();
  return validateFrontmatter(block!, SCHEMA);
}

describe("parseFrontmatterBlock", () => {
  it("returns null without frontmatter", () => {
    expect(parseFrontmatterBlock("# Hello\n")).toBeNull();
    expect(parseFrontmatterBlock("\n---\ntitle: x\n---\n")).toBeNull();
  });

  it("returns null for an unclosed fence", () => {
    expect(parseFrontmatterBlock("---\ntitle: x\n")).toBeNull();
  });

  it("finds fields with exact offsets", () => {
    const doc = "---\ntitle: Hello\ndraft: true\n---\n# Body\n";
    const block = parseFrontmatterBlock(doc)!;
    expect(block.fields.map((f) => f.name)).toEqual(["title", "draft"]);
    const [title, draft] = block.fields;
    expect(doc.slice(title.keyFrom, title.keyTo)).toBe("title");
    expect(doc.slice(title.valueFrom, title.valueTo)).toBe("Hello");
    expect(doc.slice(draft.valueFrom, draft.valueTo)).toBe("true");
    expect(draft.kind).toBe("boolean");
  });

  it("classifies inline values", () => {
    const doc = [
      "---",
      "a: text",
      'b: "true"',
      "c: 42",
      "d: 4.5",
      "e: [x, y]",
      "f: {k: v}",
      "g:",
      "---",
      "",
    ].join("\n");
    const kinds = Object.fromEntries(
      parseFrontmatterBlock(doc)!.fields.map((f) => [f.name, f.kind]),
    );
    expect(kinds).toEqual({
      a: "string",
      b: "string",
      c: "integer",
      d: "number",
      e: "array",
      f: "object",
      g: "empty",
    });
    expect(parseFrontmatterBlock(doc)!.fields[1].quoted).toBe(true);
  });

  it("classifies block lists and nested maps", () => {
    const doc = ["---", "tags:", "  - a", "  - b", "cascade:", "  draft: true", "---", ""].join("\n");
    const fields = parseFrontmatterBlock(doc)!.fields;
    expect(fields.find((f) => f.name === "tags")?.kind).toBe("array");
    expect(fields.find((f) => f.name === "cascade")?.kind).toBe("object");
  });

  it("ignores indented (nested) keys at top level", () => {
    const doc = ["---", "cascade:", "  draft: true", "---", ""].join("\n");
    expect(parseFrontmatterBlock(doc)!.fields.map((f) => f.name)).toEqual(["cascade"]);
  });
});

describe("validateFrontmatter", () => {
  it("accepts a clean document", () => {
    expect(
      diags("---\ntitle: Hello\ndate: 2026-08-11\ndraft: true\nweight: 3\ntags: [a, b]\n---\n"),
    ).toEqual([]);
  });

  it("flags an unknown field with a suggestion", () => {
    const d = diags("---\ntitel: Hello\n---\n");
    expect(d).toHaveLength(1);
    expect(d[0].kind).toBe("unknown-field");
    expect(d[0].suggestion).toBe("title");
    expect(d[0].message).toContain("did you mean `title`");
  });

  it("flags an unknown field without a close neighbour, plainly", () => {
    const d = diags("---\nzzqy_thing: 1\n---\n");
    expect(d).toHaveLength(1);
    expect(d[0].suggestion).toBeUndefined();
    expect(d[0].message).toContain("ignored");
  });

  it("stays quiet on Obsidian-native fields", () => {
    expect(diags("---\naliases: [x]\ncssclasses: [wide]\n---\n")).toEqual([]);
  });

  it("flags a quoted boolean the way an author can act on", () => {
    const d = diags('---\ndraft: "yes"\n---\n');
    expect(d).toHaveLength(1);
    expect(d[0].kind).toBe("wrong-type");
    expect(d[0].message).toContain("true or false");
  });

  it("flags non-integer weight", () => {
    expect(diags("---\nweight: heavy\n---\n")[0].kind).toBe("wrong-type");
    expect(diags("---\nweight: 1.5\n---\n")[0].message).toContain("whole number");
    expect(diags("---\nweight: 3\n---\n")).toEqual([]);
  });

  it("flags structures where text is expected", () => {
    expect(diags("---\ntitle: [a, b]\n---\n")[0].kind).toBe("wrong-type");
  });

  it("accepts scalars for array fields (one-element list), flags clear non-lists", () => {
    expect(diags("---\ntags: solo\n---\n")).toEqual([]);
    expect(diags("---\ntags: true\n---\n")[0].kind).toBe("wrong-type");
  });

  it("checks closed enums but leaves open unions alone", () => {
    expect(diags("---\nsort: date\n---\n")).toEqual([]);
    const d = diags("---\nsort: alphabetical\n---\n");
    expect(d[0].kind).toBe("not-in-enum");
    expect(d[0].message).toContain("date, weight, title");
    // Open unions (children accepts bools and wikilinks): never flagged.
    expect(diags("---\nchildren: false\n---\n")).toEqual([]);
    expect(diags("---\nchildren: [[News]]\n---\n")).toEqual([]);
  });

  it("says nothing about a field still being typed (empty value)", () => {
    expect(diags("---\ndraft:\n---\n")).toEqual([]);
  });

  it("flags scalar where a nested map is expected", () => {
    expect(diags("---\ncascade: yes\n---\n")[0].message).toContain("nested map");
  });
});

describe("nearestFieldName", () => {
  it("finds close names within distance 2", () => {
    expect(nearestFieldName("titel", SCHEMA)).toBe("title");
    expect(nearestFieldName("Tags", SCHEMA)).toBe("tags");
    expect(nearestFieldName("drafts", SCHEMA)).toBe("draft");
  });

  it("gives up beyond distance 2", () => {
    expect(nearestFieldName("completely-different", SCHEMA)).toBeUndefined();
  });
});
