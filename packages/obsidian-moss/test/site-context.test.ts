import type { ContentSchema } from "@symbiosis-lab/moss-ui";
import { initializeCore, MossSite } from "../src/core";
import type { DataAdapter, Vault } from "obsidian";
import { describe, expect, it } from "vitest";
import { ownsMoss, readSiteContext, siteOf, vaultBaseHref, type OpenSite } from "../src/site-context";

initializeCore();

/** `moss` is what sits at the vault's `.moss`: moss's folder by default. */
function vault(files: Record<string, string>, moss: "folder" | "file" | null = "folder") {
  const adapter = {
    stat: async (path: string) => path === ".moss" && moss ? { type: moss } : null,
    exists: async (path: string) => path in files || Object.keys(files).some((key) => key.startsWith(`${path}/`)),
    read: async (path: string) => files[path] ?? "",
    list: async (path: string) => {
      const children = Object.keys(files).filter((key) => key.startsWith(`${path}/`));
      return {
        files: children.filter((key) => !key.slice(path.length + 1).includes("/")),
        folders: [...new Set(children.filter((key) => key.slice(path.length + 1).includes("/"))
          .map((key) => `${path}/${key.slice(path.length + 1).split("/")[0]}`))],
      };
    },
  } as DataAdapter;
  return { adapter, getFiles: () => [...new Set(["note.md", "images/pic.png", ...Object.keys(files).filter((p) => !p.startsWith(".moss/"))])].map((path) => ({ path })) } as Pick<Vault, "adapter" | "getFiles">;
}

async function contextOf(site: Pick<Vault, "adapter" | "getFiles">) {
  const context = await readSiteContext(site);
  if (!context) throw new Error("not read as a moss folder");
  return context;
}

describe("site context through the mobile adapter", () => {
  it("reads nothing from a vault that is not a moss folder", async () => {
    for (const moss of [null, "file"] as const) {
      const site = vault({ "2026-11-17.md": "First line of a plain note." }, moss);
      site.adapter.read = async (path: string) => { throw new Error(`read ${path}`); };
      expect(await readSiteContext(site)).toBeNull();
    }
  });

  it("inlines the sheets style.css imports, where it imports them, once each", async () => {
    const { theme_css } = await contextOf(vault({
      ".moss/theme/style.css": "/* was: @import \"parts/a.css\"; */\n@import \"parts/a.css\";\n@import url('print.css') print;\n@import url(https://fonts.example/x.css);\n@import \"l.css\" layer(base);\nbody{}",
      ".moss/theme/parts/a.css": "@import '../b.css';\n@import \"a.css\";\n.a{}",
      ".moss/theme/b.css": ".b{}",
      ".moss/theme/print.css": ".p{}",
      ".moss/theme/unused.css": ".u{}",
      ".moss/theme/l.css": ".l{}",
    }));
    expect(theme_css).toBe("/* was: @import \"parts/a.css\"; */\n.b{}\n\n.a{}\n@media print{.p{}}\n@import url(https://fonts.example/x.css);\n@import \"l.css\" layer(base);\nbody{}");
  });

  it("reads lang, the theme's style.css alone and plugin contributions", async () => {
    const context = await contextOf(vault({
      ".moss/config.toml": 'lang = "wrong"\n[site]\nlang = "fr" # language\n[other]\nlang = "de"',
      ".moss/theme/style.css": "/* served-sheet */", ".moss/theme/a.css": "/* unserved-sheet */", ".moss/theme/ignored.txt": "ignored",
      ".moss/plugins/a/main.js": "", ".moss/plugins/b/main.js": "",
      ".moss/plugins/b/manifest.json": JSON.stringify({ name: "b", entry: "main.js", contributes: { listing_styles: { board: {} } } }),
      ".moss/plugins/a/manifest.json": JSON.stringify({ name: "a", entry: "main.js", contributes: { frontmatter: { fields: { review_score: { type: "integer" } } }, listing_styles: { cards: {} } } }),
    }));
    const site = new MossSite(JSON.stringify(context));
    const schema: ContentSchema = JSON.parse(site.schemaJson());
    expect(schema.frontmatter.fields.review_score).toMatchObject({ type: "integer", source: "a" });
    expect(schema.frontmatter.fields.children_style.enum_values).toEqual(expect.arrayContaining(["cards", "board"]));
    const preview = JSON.parse(site.preview("note.md", "# Test", ""));
    expect(preview.html).toContain('lang="fr"');
    expect(preview.html).toContain("/* served-sheet */");
    expect(preview.html).not.toContain("unserved-sheet");
    site.free();
    expect(context).toMatchObject({
      files: ["images/pic.png", "note.md"], lang: "fr", theme_css: "/* served-sheet */",
      declared_name_lists: ["author", "tags"],
      plugins: [{ name: "a", fields: { review_score: { type: "integer" } } }, { name: "b", fields: {} }],
      listing_styles: ["board", "cards"],
    });
  });
  it("defaults missing configuration to English with no contributions", async () => {
    expect(await contextOf(vault({}))).toMatchObject({ lang: "en", theme_css: "", plugins: [], listing_styles: [] });
  });
  it("lets core elect the marked home and read terms and normalized language", async () => {
    const context = await contextOf(vault({
      "index.md": "---\nlang: de\n---",
      "welcome.md": "---\nhome: true\nlang: ' ZH-Hant '\n---",
      ".moss/config.toml": "[site]\nlang = 'invalid'\n[terms.people]\nfields = ['editor', 'title']",
    }));
    expect(context.lang).toBe("zh-hant");
    expect(context.declared_name_lists).toEqual(["author", "tags", "editor"]);
    const site = new MossSite(JSON.stringify(context));
    const fields = JSON.parse(site.schemaJson()).frontmatter.fields;
    expect(fields.editor.type).toBe(fields.author.type);
    site.free();
  });
  it("reads the texts of only the files core names", async () => {
    const files = { "index.md": "Home", "Notes.MD": "Notes", "child/page.md": "Child", "data.json": "{}" };
    const site = vault(files);
    const reads: string[] = [];
    const read = site.adapter.read;
    site.adapter.read = async (path: string) => { reads.push(path); return read(path); };
    await contextOf(site);
    expect(reads.filter((path) => !path.startsWith(".moss/"))).toEqual(["Notes.MD", "index.md", "note.md"]);
  });
  it("passes malformed manifests and missing entries to core to skip", async () => {
    const context = await contextOf(vault({
      ".moss/config.toml": "[broken",
      ".moss/plugins/broken/manifest.json": "{",
      ".moss/plugins/missing/manifest.json": JSON.stringify({ entry: "missing.js", contributes: { listing_styles: { board: {} } } }),
      ".moss/plugins/fallback/manifest.json": "{}",
    }));
    expect(context.lang).toBe("en");
    expect(context.plugins).toEqual([{ name: "fallback", fields: {}, blocks: [] }]);
    expect(context.listing_styles).toEqual([]);
  });
  it("removes the entire nested file path and cache query from a resource URL", () => {
    expect(vaultBaseHref({ getResourcePath: () => "capacitor://localhost/vault/folder/my%20note.md?mtime=123" }, "folder/my note.md"))
      .toBe("capacitor://localhost/vault/");
  });
});

describe("ownsMoss", () => {
  const adapter = (base: string) => ({ getBasePath: () => base, stat: async (path: string) => path.endsWith(".moss") ? { type: "folder" } : null }) as never;

  it("reads a folder with .moss as a moss folder, but not the home folder, whose .moss is moss's own store", async () => {
    const home = process.env.HOME;
    process.env.HOME = "/home/ada";
    try {
      expect(await ownsMoss(adapter("/home/ada/vault"), "/")).toBe(true);
      expect(await ownsMoss(adapter("/home/ada/vault"), "garden")).toBe(true);
      expect(await ownsMoss(adapter("/home/ada"), "/")).toBe(false);
      expect(await ownsMoss(adapter("/home"), "ada")).toBe(false);
    } finally {
      process.env.HOME = home;
    }
  });
});

describe("a moss folder inside the vault", () => {
  it("is read as a site of its own, by the paths inside it", async () => {
    const files: Record<string, string> = {
      "garden/.moss/config.toml": '[site]\nlang = "de"',
      "garden/.moss/theme/style.css": "body { color: teal }",
      "garden/index.md": "# Home",
      "garden/posts/frost.md": "[[index]]",
      "Daily notes/2026-11-24.md": "A plain note.",
    };
    const adapter = {
      stat: async (path: string) => Object.keys(files).some((key) => key.startsWith(`${path}/`)) ? { type: "folder" } : null,
      exists: async (path: string) => path in files || Object.keys(files).some((key) => key.startsWith(`${path}/`)),
      read: async (path: string) => files[path] ?? "",
      list: async () => ({ files: [], folders: [] }),
    } as unknown as DataAdapter;
    const garden = { adapter, getFiles: () => Object.keys(files).filter((path) => !path.includes(".moss/")).map((path) => ({ path })) } as Pick<Vault, "adapter" | "getFiles">;
    expect(await readSiteContext(garden)).toBeNull();
    const context = (await readSiteContext(garden, "garden"))!;
    expect([...context.files].sort()).toEqual(["index.md", "posts/frost.md"]);
    expect(context.lang).toBe("de");
    expect(context.theme_css).toContain("teal");
  });

  it("holds the notes below it, the nearest moss folder first", () => {
    const sites = ["/", "garden", "garden/old"].map((root) => ({ root }) as OpenSite);
    expect(siteOf(sites, "garden/old/frost.md")).toMatchObject({ root: "garden/old", path: "frost.md" });
    expect(siteOf([...sites].reverse(), "garden/frost.md")).toMatchObject({ root: "garden", path: "frost.md" });
    expect(siteOf(sites, "gardening/frost.md")).toMatchObject({ root: "/", path: "gardening/frost.md" });
    expect(siteOf(sites.slice(1), "Daily notes/2026-11-24.md")).toBeNull();
  });

  it("gives the address of the site's folder for a note inside it", () => {
    const adapter = { getResourcePath: () => "app://local/vault/my%20garden/posts/frost.md?1" };
    expect(vaultBaseHref(adapter, "my garden/posts/frost.md", "posts/frost.md")).toBe("app://local/vault/my%20garden/");
  });
});

it("leaves a moss folder inside a site out of that site, as moss's build does", async () => {
  const files = ["index.md", "about.md", "blog/post.md", "blog/drafts/idea.md", "blogroll.md"];
  const adapter = {
    stat: async (path: string) => (path === ".moss" || path === "blog/.moss" ? { type: "folder" } : null),
    exists: async () => false,
    read: async () => "",
    list: async () => ({ files: [], folders: [] }),
  } as unknown as DataAdapter;
  const vault = { adapter, getFiles: () => files.map((path) => ({ path })) } as Pick<Vault, "adapter" | "getFiles">;
  const roots = ["/", "blog"];
  expect([...(await readSiteContext(vault, "/", roots))!.files].sort()).toEqual(["about.md", "blogroll.md", "index.md"]);
  expect([...(await readSiteContext(vault, "blog", roots))!.files].sort()).toEqual(["drafts/idea.md", "post.md"]);
});
