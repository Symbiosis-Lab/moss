import { describe, expect, it, vi } from "vitest";
import { mergeTypes, propertyType, showMossFields } from "../src/property-types";
import { initializeCore, MossSite } from "../src/core";
import { siteSchema } from "../src/schema";

const schema = (fields: Record<string, string>) => ({
  fields: new Map(Object.entries(fields).map(([name, widget]) => [name, { name, widget, type: "", description: "", group: "" }])),
});

describe("propertyType", () => {
  it("maps widgets to Obsidian types", () => {
    expect(propertyType({ name: "draft", widget: "checkbox" })).toBe("checkbox");
    expect(propertyType({ name: "date", widget: "date-picker" })).toBe("date");
    expect(propertyType({ name: "weight", widget: "number-input" })).toBe("number");
    expect(propertyType({ name: "syndicate", widget: "tag-input" })).toBe("multitext");
    expect(propertyType({ name: "tags", widget: "tag-input" })).toBe("tags");
    expect(propertyType({ name: "title", widget: "text-input" })).toBe("text");
    expect(propertyType({ name: "x" })).toBe("text");
  });
});

describe("mergeTypes", () => {
  it("never overwrites a type the person set, and leaves text fields out", () => {
    const { types, added } = mergeTypes({ draft: "text" }, schema({ draft: "checkbox", date: "date-picker", title: "text-input" }));
    expect(types).toEqual({ draft: "text", date: "date" });
    expect(added).toEqual(["date"]);
  });

  it("types the real moss schema", () => {
    initializeCore();
    const { types } = mergeTypes({}, siteSchema(new MossSite(JSON.stringify({ files: [], lang: "en" }))));
    expect(types).toMatchObject({ draft: "checkbox", syndicate: "multitext", weight: "number" });
    expect(types.title).toBeUndefined();
  });
});

describe("showMossFields", () => {
  it("leaves a types.json it cannot read untouched", async () => {
    for (const text of ["{ not json", "null", "[]"]) {
      const write = vi.fn();
      const app = { vault: { configDir: ".obsidian", adapter: { exists: async () => true, read: async () => text, write } } };
      expect(await showMossFields(app as never, schema({ draft: "checkbox" }))).toBeNull();
      expect(write).not.toHaveBeenCalled();
    }
  });
});
