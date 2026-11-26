// Menu entries that put a folder's settings and a page's cover one click from where they are chosen.
import { Notice, TFile, TFolder, type App, type Plugin } from "obsidian";

const IMAGE = /^(png|jpe?g|gif|webp|avif|svg)$/i;
export const NEW_FOLDER_HOME = "---\ncascade:\n---\n";

/** `target` as written from the folder of the page at `from`. */
export function relativePath(from: string, target: string): string {
  const base = from.split("/").slice(0, -1);
  const to = target.split("/");
  while (base.length && to.length > 1 && base[0] === to[0]) { base.shift(); to.shift(); }
  return [...base.map(() => ".."), ...to].join("/");
}

export function folderHome(folder: TFolder): string {
  return folder.isRoot() ? "index.md" : `${folder.path}/index.md`;
}

async function openFolderSettings(app: App, folder: TFolder): Promise<void> {
  const path = folderHome(folder);
  const home = app.vault.getAbstractFileByPath(path) ?? await app.vault.create(path, NEW_FOLDER_HOME);
  if (home instanceof TFile) await app.workspace.getLeaf(false).openFile(home);
}

async function useAsCover(app: App, image: TFile): Promise<void> {
  const page = app.workspace.getActiveFile();
  if (!page || page.extension !== "md") return void new Notice("Open a page first.");
  await app.fileManager.processFrontMatter(page, (data) => { data.cover = relativePath(page.path, image.path); });
}

export function registerEditorActions(plugin: Plugin): void {
  const { app } = plugin;
  plugin.registerEvent(app.workspace.on("file-menu", (menu, file) => {
    if (file instanceof TFolder) {
      menu.addItem((item) => item.setTitle("Folder settings").setIcon("settings").onClick(() => void openFolderSettings(app, file)));
    } else if (file instanceof TFile && IMAGE.test(file.extension)) {
      menu.addItem((item) => item.setTitle("Use as cover").setIcon("image").onClick(() => void useAsCover(app, file)));
    }
  }));
}
