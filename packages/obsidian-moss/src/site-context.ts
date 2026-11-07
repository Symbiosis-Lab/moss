import type { DataAdapter, Vault } from "obsidian";
import type { SitePlugin } from "./schema";
import { initializeCore, siteBundle, siteTextPaths } from "./core";

export interface SiteContext {
  files: string[];
  lang: string;
  declared_name_lists: string[];
  theme_css: string;
  plugins: SitePlugin[];
  listing_styles: string[];
}

// A comment is matched first so an @import written inside one is never followed.
const IMPORT = /\/\*[\s\S]*?\*\/|@import\s+(?:url\(\s*)?["']?([^"')\s;]+)["']?\s*\)?([^;]*);/g;

/**
 * The theme's style.css with each relative `@import` replaced by the sheet it
 * names, to any depth, so the one inline `<style>` of the preview carries what
 * the build's page gets by following the imports. A sheet is inlined once; an
 * import's media condition becomes an `@media` block around it; an import in a
 * layer or behind `supports()` stays as written.
 */
async function themeCss(adapter: DataAdapter, path: string, seen = new Set<string>()): Promise<string> {
  if (seen.has(path) || !await adapter.exists(path)) return "";
  seen.add(path);
  const css = await adapter.read(path);
  const dir = path.slice(0, path.lastIndexOf("/"));
  let out = "";
  let last = 0;
  for (const match of css.matchAll(IMPORT)) {
    const [, target, condition = ""] = match;
    const media = condition.trim();
    if (!target || target.includes("://") || target.startsWith("/") || /^(layer|supports)\b/.test(media)) continue;
    const parts: string[] = [];
    for (const part of `${dir}/${target}`.split("/")) {
      if (part === "..") parts.pop();
      else if (part && part !== ".") parts.push(part);
    }
    const sheet = await themeCss(adapter, parts.join("/"), seen);
    out += css.slice(last, match.index) + (media && sheet ? `@media ${media}{${sheet}}` : sheet);
    last = match.index + match[0].length;
  }
  return out + css.slice(last);
}

async function list(adapter: DataAdapter, path: string) {
  return await adapter.exists(path) ? adapter.list(path) : { files: [], folders: [] };
}

export async function readSiteContext(vault: Pick<Vault, "adapter" | "getFiles">): Promise<SiteContext> {
  const adapter = vault.adapter;
  const config = await adapter.exists(".moss/config.toml") ? await adapter.read(".moss/config.toml") : "";
  // The build serves only style.css; other sheets reach a page through its @import.
  const css = await themeCss(adapter, ".moss/theme/style.css");
  const files = vault.getFiles().map((file) => file.path);
  initializeCore();
  const texts: Record<string, string> = {};
  for (const path of siteTextPaths(files)) texts[path] = await adapter.read(path);
  const plugins = [];
  const directory = await list(adapter, ".moss/plugins");
  for (const folder of directory.folders) {
    const path = `${folder}/manifest.json`;
    if (!await adapter.exists(path)) continue;
    const manifest_json = await adapter.read(path);
    // Only locate the entry's bytes; core reads and validates contributions.
    let entry: unknown;
    try {
      entry = JSON.parse(manifest_json).entry;
    } catch { /* Core reports malformed manifests along with the other warnings. */ }
    const entry_present = typeof entry === "string" && await adapter.exists(`${folder}/${entry}`);
    plugins.push({ folder: folder.split("/").pop() ?? folder, manifest_json, entry_present, admission: { status: "admitted" } });
  }
  const result: { bundle: SiteContext; warnings: string[] } = JSON.parse(siteBundle(JSON.stringify({
    files, texts, config_toml: config, plugins, theme_style_css: css,
  })));
  for (const warning of result.warnings) console.warn(`[moss] ${warning}`);
  return result.bundle;
}

export function vaultBaseHref(adapter: Pick<DataAdapter, "getResourcePath">, filePath: string): string {
  const resource = adapter.getResourcePath(filePath).split("?")[0];
  const parts = resource.split("/");
  parts.splice(-filePath.split("/").length);
  return `${parts.join("/")}/`;
}
