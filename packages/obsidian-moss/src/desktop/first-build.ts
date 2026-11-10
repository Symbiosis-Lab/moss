// What a first moss build writes into a folder that is not yet a moss site,
// said before it happens. Pure, so the wording is tested without Obsidian.

export interface FirstBuildFacts {
  /** The folder's name, as the owner sees it in the file list. */
  folder: string;
  /** The folder sits in iCloud Drive, so every change reaches the owner's other devices. */
  inICloud: boolean;
}

export function firstBuildMessage(f: FirstBuildFacts): string[] {
  const lines = [`Building ${f.folder} will create a .moss folder in it, and change no note.`];
  lines.push(
    f.inICloud
      ? "iCloud syncs these changes to your other devices."
      : `Whatever syncs ${f.folder} copies these changes too.`,
  );
  return lines;
}

export function isInICloud(folder: string): boolean {
  return folder.includes("/Library/Mobile Documents/");
}
