import type { TAbstractFile, Vault } from "obsidian";
import { ownsMoss } from "./site-context";

/** The vault paths of the moss folders ("/" is the vault itself). Hidden folders are outside Obsidian's index, so each folder is asked. */
export async function mossFolders(vault: Pick<Vault, "adapter" | "getAllLoadedFiles">): Promise<Set<string>> {
  const folders = vault.getAllLoadedFiles().filter((entry: TAbstractFile & { children?: unknown }) => entry.children !== undefined);
  const owned = await Promise.all(folders.map((folder) => ownsMoss(vault.adapter, folder.path)));
  return new Set(folders.filter((_, index) => owned[index]).map((folder) => folder.path));
}

/**
 * Puts the class that draws the moss mark (styles.css) on each moss folder's
 * row in the file tree, and takes it off every other. The root has no row; the
 * vault's name at the foot of the sidebar stands for it.
 */
export function markRows(root: ParentNode, folders: ReadonlySet<string>): void {
  root.querySelectorAll<HTMLElement>(".nav-folder-title[data-path]").forEach((row) => {
    row.classList.toggle("moss-folder", folders.has(row.dataset.path ?? ""));
  });
  root.querySelectorAll(".workspace-drawer-vault-name").forEach((name) => name.classList.toggle("moss-folder", folders.has("/")));
}
