import type { DataAdapter, Vault } from "obsidian";
import type { SitePlugin } from "./schema";

export interface SiteContext {
  files: string[];
  lang: string;
  theme_css: string;
  plugins: SitePlugin[];
  listing_styles: string[];
}

// Site configuration parsing belongs in moss-core.
export function siteLanguage(toml: string): string {
  let inSite = false;
  for (const line of toml.split(/\r?\n/)) {
    const section = /^\s*\[([^\]]+)\]\s*(?:#.*)?$/.exec(line);
    if (section) inSite = section[1].trim() === "site";
    if (!inSite) continue;
    const lang = /^\s*lang\s*=\s*(?:"([^"\\]+)"|'([^']+)')\s*(?:#.*)?$/.exec(line);
    if (lang) return lang[1] ?? lang[2];
  }
  return "en";
}

async function list(adapter: DataAdapter, path: string) {
  return await adapter.exists(path) ? adapter.list(path) : { files: [], folders: [] };
}

export async function readSiteContext(vault: Pick<Vault, "adapter" | "getFiles">): Promise<SiteContext> {
  const adapter = vault.adapter;
  const config = await adapter.exists(".moss/config.toml") ? await adapter.read(".moss/config.toml") : "";
  const theme = await list(adapter, ".moss/theme");
  const css = await Promise.all(theme.files.filter((p) => p.endsWith(".css")).sort().map((p) => adapter.read(p)));
  const plugins: SitePlugin[] = [];
  const listingStyles = new Set<string>();
  const directory = await list(adapter, ".moss/plugins");
  for (const folder of directory.folders.sort()) {
    const path = `${folder}/manifest.json`;
    if (!await adapter.exists(path)) continue;
    try {
      const manifest = JSON.parse(await adapter.read(path));
      plugins.push({ name: manifest.name ?? folder.split("/").pop(), fields: manifest.contributes?.frontmatter?.fields ?? {} });
      for (const style of Object.keys(manifest.contributes?.listing_styles ?? {})) listingStyles.add(style);
    } catch (error) {
      console.warn(`[moss] Cannot read ${path}`, error);
    }
  }
  return {
    files: vault.getFiles().map((file) => file.path).sort(),
    lang: siteLanguage(config), theme_css: css.join("\n"), plugins, listing_styles: [...listingStyles],
  };
}

export function vaultBaseHref(adapter: Pick<DataAdapter, "getResourcePath">, filePath: string): string {
  const resource = adapter.getResourcePath(filePath).split("?")[0];
  const parts = resource.split("/");
  parts.splice(-filePath.split("/").length);
  return `${parts.join("/")}/`;
}
