import type { ContentSchema } from "@symbiosis-lab/moss-ui";
import { initializeCore, MossSite } from "../src/core";
import type { DataAdapter, Vault } from "obsidian";
import { describe, expect, it } from "vitest";
import { readSiteContext, siteLanguage, vaultBaseHref } from "../src/site-context";

initializeCore();

function vault(files: Record<string, string>) {
  const adapter = {
    exists: async (path: string) => path in files || Object.keys(files).some((key) => key.startsWith(`${path}/`)),
    read: async (path: string) => files[path],
    list: async (path: string) => {
      const children = Object.keys(files).filter((key) => key.startsWith(`${path}/`));
      return {
        files: children.filter((key) => !key.slice(path.length + 1).includes("/")),
        folders: [...new Set(children.filter((key) => key.slice(path.length + 1).includes("/"))
          .map((key) => `${path}/${key.slice(path.length + 1).split("/")[0]}`))],
      };
    },
  } as DataAdapter;
  return { adapter, getFiles: () => [{ path: "note.md" }, { path: "images/pic.png" }] } as Pick<Vault, "adapter" | "getFiles">;
}

describe("site context through the mobile adapter", () => {
  it("reads lang, sorted hidden CSS and plugin contributions", async () => {
    const context = await readSiteContext(vault({
      ".moss/config.toml": 'lang = "wrong"\n[site]\nlang = "fr" # language\n[other]\nlang = "de"',
      ".moss/theme/z.css": "last", ".moss/theme/a.css": "first", ".moss/theme/ignored.txt": "ignored",
      ".moss/plugins/b/manifest.json": JSON.stringify({ name: "b", contributes: { listing_styles: { board: {} } } }),
      ".moss/plugins/a/manifest.json": JSON.stringify({ name: "a", contributes: { frontmatter: { fields: { review_score: { type: "integer" } } }, listing_styles: { cards: {} } } }),
    }));
    const site = new MossSite(JSON.stringify(context));
    const schema: ContentSchema = JSON.parse(site.schemaJson());
    expect(schema.frontmatter.fields.review_score).toMatchObject({ type: "integer", source: "a" });
    expect(schema.frontmatter.fields.children_style.enum_values).toEqual(expect.arrayContaining(["cards", "board"]));
    const preview = JSON.parse(site.preview("note.md", "# Test", ""));
    expect(preview.html).toContain('lang="fr"');
    expect(preview.html).toContain("first\nlast");
    site.free();
    expect(context).toEqual({
      files: ["images/pic.png", "note.md"], lang: "fr", theme_css: "first\nlast",
      plugins: [{ name: "a", fields: { review_score: { type: "integer" } } }, { name: "b", fields: {} }],
      listing_styles: ["cards", "board"],
    });
  });
  it("defaults missing configuration to English with no contributions", async () => {
    expect(await readSiteContext(vault({}))).toMatchObject({ lang: "en", theme_css: "", plugins: [], listing_styles: [] });
    expect(siteLanguage("[site]\nlang = 'zh-Hant'\n")).toBe("zh-Hant");
    expect(siteLanguage('[other]\nlang = "fr"')).toBe("en");
  });
  it("removes the entire nested file path and cache query from a resource URL", () => {
    expect(vaultBaseHref({ getResourcePath: () => "capacitor://localhost/vault/folder/my%20note.md?mtime=123" }, "folder/my note.md"))
      .toBe("capacitor://localhost/vault/");
  });
});
