import { expect, it } from "vitest";
import { parseFrontmatterBlock } from "../src/frontmatter";

it("finds top-level field ranges without interpreting values", () => {
  const text = '---\ntitle: Hello\ncascade:\n  draft: true\n---\n# Body';
  const block = parseFrontmatterBlock(text)!;
  expect(block.fields.map((field) => field.name)).toEqual(["title", "cascade"]);
  const title = block.fields[0];
  expect(text.slice(title.keyFrom, title.keyTo)).toBe("title");
  expect(text.slice(title.valueFrom, title.valueTo)).toBe("Hello");
});

it("handles CRLF offsets and absent or incomplete frontmatter", () => {
  const text = "---\r\ntitle: Hello\r\n---\r\n";
  const field = parseFrontmatterBlock(text)!.fields[0];
  expect(text.slice(field.keyFrom, field.keyTo)).toBe("title");
  expect(parseFrontmatterBlock("# Hello")).toBeNull();
  expect(parseFrontmatterBlock("---\ntitle: x\n")).toBeNull();
});
