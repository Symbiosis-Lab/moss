import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { parseDescribeJson } from "../src/schema";

// The bundled snapshot IS a real `moss describe --json` capture (0.14.1);
// parsing it here proves the plugin's schema source against reality.
const SNAPSHOT = readFileSync(join(__dirname, "../src/describe-snapshot.json"), "utf8");

describe("parseDescribeJson", () => {
  it("parses the real describe --json capture", () => {
    const schema = parseDescribeJson(SNAPSHOT);
    expect(schema.source).toBe("moss 0.14.1");
    expect(schema.fields.size).toBeGreaterThanOrEqual(40);

    const title = schema.fields.get("title");
    expect(title?.type).toBe("string");
    expect(title?.description).toMatch(/Title of the page/);
    expect(title?.group).toBe("This Page");

    expect(schema.fields.get("draft")?.type).toBe("boolean");
    expect(schema.fields.get("weight")?.type).toBe("integer");
    expect(schema.fields.get("tags")?.type).toBe("array");
    expect(schema.fields.get("cascade")?.type).toBe("object");
  });

  it("keeps enum values for closed one_of fields", () => {
    const schema = parseDescribeJson(SNAPSHOT);
    expect(schema.fields.get("sort")?.enumValues).toEqual(["date", "weight", "title"]);
    // Open unions carry no enum — must not be over-validated.
    expect(schema.fields.get("children")?.enumValues).toBeUndefined();
  });

  it("throws on output with no frontmatter array", () => {
    expect(() => parseDescribeJson("{}")).toThrow(/frontmatter/);
    expect(() => parseDescribeJson('{"frontmatter": []}')).toThrow(/no usable fields/);
  });

  it("throws on non-JSON (a crashed or ancient CLI)", () => {
    expect(() => parseDescribeJson("Unknown command: describe")).toThrow();
  });

  it("labels a custom source", () => {
    const schema = parseDescribeJson(SNAPSHOT, "bundled snapshot");
    expect(schema.source).toBe("bundled snapshot");
  });
});
