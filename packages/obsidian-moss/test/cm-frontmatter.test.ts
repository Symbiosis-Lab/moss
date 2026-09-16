import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  computeDiagnostics,
  diagnosticsToDecorations,
  fieldCompletions,
  fieldInfoText,
} from "../src/cm-frontmatter";
import { parseDescribeJson } from "../src/schema";

const SCHEMA = parseDescribeJson(
  readFileSync(join(__dirname, "../src/describe-snapshot.json"), "utf8"),
);

describe("computeDiagnostics", () => {
  it("is empty for docs without frontmatter", () => {
    expect(computeDiagnostics("# Hi\n", SCHEMA)).toEqual([]);
  });

  it("carries offsets a decoration can use", () => {
    const doc = "---\ntitel: x\n---\n";
    const [d] = computeDiagnostics(doc, SCHEMA);
    expect(doc.slice(d.from, d.to)).toBe("titel");
  });
});

describe("diagnosticsToDecorations", () => {
  it("builds a sorted decoration set with the lint classes", () => {
    const doc = '---\ndraft: "yes"\ntitel: x\n---\n';
    const set = diagnosticsToDecorations(computeDiagnostics(doc, SCHEMA));
    const seen: Array<{ from: number; cls: string | undefined }> = [];
    const iter = set.iter();
    while (iter.value) {
      seen.push({ from: iter.from, cls: (iter.value.spec as { class?: string }).class });
      iter.next();
    }
    expect(seen.map((s) => s.cls)).toEqual(["moss-fm-wrong-type", "moss-fm-unknown"]);
    expect(seen[0].from).toBeLessThan(seen[1].from);
  });
});

describe("fieldInfoText", () => {
  it("explains a schema field with type, group and description", () => {
    const text = fieldInfoText(SCHEMA, "draft")!;
    expect(text).toMatch(/^draft: boolean — This Page/);
    expect(text).toContain("Hidden from all listings");
  });

  it("names Obsidian fields as ignored by moss", () => {
    expect(fieldInfoText(SCHEMA, "aliases")).toContain("moss ignores it");
  });

  it("returns null for genuinely unknown names", () => {
    expect(fieldInfoText(SCHEMA, "zzqy")).toBeNull();
  });
});

describe("fieldCompletions", () => {
  it("offers schema fields matching the prefix, with insert text", () => {
    const items = fieldCompletions(SCHEMA, "---\n\n---\n", "ti");
    expect(items.map((i) => i.name)).toContain("title");
    const title = items.find((i) => i.name === "title")!;
    expect(title.insert).toBe("title: ");
    expect(title.description).toMatch(/Title of the page/);
  });

  it("omits fields already present and internal fields", () => {
    const items = fieldCompletions(SCHEMA, "---\ntitle: x\n---\n", "");
    expect(items.map((i) => i.name)).not.toContain("title");
    expect(items.every((i) => !i.name.startsWith("_"))).toBe(true);
  });
});
