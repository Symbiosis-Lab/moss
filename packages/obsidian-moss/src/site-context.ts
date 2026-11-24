import { StateEffect, type EditorState } from "@codemirror/state";
import type { DataAdapter, Vault } from "obsidian";
import type { MossSchema, SitePlugin } from "./schema";
import { initializeCore, siteBundle, siteTextPaths, type MossSite } from "./core";

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

/**
 * moss's own rule: a folder is a site when it owns a `.moss/` folder, except
 * the person's home folder, whose `.moss/` is moss's own store. `folder` is a
 * vault path; "/" is the vault.
 */
export async function ownsMoss(adapter: Pick<DataAdapter, "stat"> & { getBasePath?(): string }, folder: string): Promise<boolean> {
  const inVault = folder === "/" ? "" : `${folder}/`;
  // Only the desktop app has a process and a path on disk.
  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env;
  const home = env?.HOME ?? env?.USERPROFILE;
  if (home && `${adapter.getBasePath?.()}/${inVault}` === `${home}/`) return false;
  return (await adapter.stat(`${inVault}.moss`))?.type === "folder";
}

/**
 * The site of the moss folder `root` ("/" is the vault), with every path as
 * that site sees it, or null when `root` is not a moss folder. `roots` are the
 * vault's moss folders: a note below another one is that folder's, as in
 * moss's build, which leaves a nested site out of the site around it.
 */
export async function readSiteContext(vault: Pick<Vault, "adapter" | "getFiles">, root = "/", roots: readonly string[] = [root]): Promise<SiteContext | null> {
  const adapter = vault.adapter;
  if (!await ownsMoss(adapter, root)) return null;
  const at = root === "/" ? "" : `${root}/`;
  const config = await adapter.exists(`${at}.moss/config.toml`) ? await adapter.read(`${at}.moss/config.toml`) : "";
  // The build serves only style.css; other sheets reach a page through its @import.
  const css = await themeCss(adapter, `${at}.moss/theme/style.css`);
  const owners = roots.map((owner) => ({ root: owner }));
  const files = vault.getFiles().filter((file) => siteOf(owners, file.path)?.root === root).map((file) => file.path.slice(at.length));
  initializeCore();
  const texts: Record<string, string> = {};
  for (const path of siteTextPaths(files)) texts[path] = await adapter.read(at + path);
  const plugins = [];
  const directory = await list(adapter, `${at}.moss/plugins`);
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

/** A moss folder the plugin has read ("/" is the vault). */
export interface OpenSite {
  root: string;
  site: MossSite;
  schema: MossSchema;
}

/** A note as its site sees it: `path` is relative to the site's folder. */
export interface SiteNote extends OpenSite {
  path: string;
}

/** The site of an editor's note, or null when no moss folder holds it. */
export type NoteOf = (state: EditorState) => SiteNote | null;

/** Dispatched to every editor when the plugin has read the moss folders again. */
export const contextChanged = StateEffect.define<void>();

/** The nearest moss folder above the note at vault path `path`, or null when no moss folder holds it. */
export function siteOf<Site extends { root: string }>(sites: readonly Site[], path: string): (Site & { path: string }) | null {
  const depth = (root: string) => (root === "/" ? 0 : root.length + 1);
  let owner: Site | undefined;
  for (const site of sites) {
    if (depth(site.root) && !path.startsWith(`${site.root}/`)) continue;
    if (!owner || depth(site.root) > depth(owner.root)) owner = site;
  }
  return owner ? { ...owner, path: path.slice(depth(owner.root)) } : null;
}

/** The address of the folder `sitePath` is relative to, from the resource URL of the file at vault path `filePath`. */
export function vaultBaseHref(adapter: Pick<DataAdapter, "getResourcePath">, filePath: string, sitePath = filePath): string {
  const resource = adapter.getResourcePath(filePath).split("?")[0];
  const parts = resource.split("/");
  parts.splice(-sitePath.split("/").length);
  return `${parts.join("/")}/`;
}
