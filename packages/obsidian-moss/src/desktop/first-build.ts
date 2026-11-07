// What a first moss build writes into a folder that is not yet a moss site,
// said before it happens. Pure, so the wording is tested without Obsidian.

export interface FirstBuildFacts {
  /** Notes that have frontmatter but no `uid:`; the build adds one to each. */
  notesGainingUid: number;
  /** The coding-agent folders the build will create (`.claude`, `.cursor`). */
  agentFolders: string[];
  /** The folder sits in iCloud Drive, so every change reaches the owner's other devices. */
  inICloud: boolean;
}

export function firstBuildMessage(f: FirstBuildFacts): string[] {
  const lines = ["moss has not built this vault before. Building it will:"];
  if (f.notesGainingUid > 0) {
    const notes = f.notesGainingUid === 1 ? "1 note" : `${f.notesGainingUid} notes`;
    lines.push(`• add a uid: line to the frontmatter of ${notes}: a permanent id moss uses to keep each page's identity when it moves`);
  }
  lines.push("• create a .moss folder in the vault, for moss's cache and the built site");
  if (f.agentFolders.length > 0) {
    lines.push(`• create ${f.agentFolders.join(" and ")} with moss's notes for coding agents`);
  }
  lines.push(
    f.inICloud
      ? "iCloud syncs these changes to your other devices."
      : "Whatever syncs this vault will copy these changes too.",
  );
  return lines;
}

/** The agent folders a build writes: those whose tool is set up in the vault or in the home folder. */
export function agentFoldersFor(exists: (p: string) => boolean, vault: string, home: string, sep = "/"): string[] {
  return [".claude", ".cursor"].filter((d) => !exists(vault + sep + d) && exists(home + sep + d));
}

export function isInICloud(vaultPath: string): boolean {
  return vaultPath.includes("/Library/Mobile Documents/");
}
