import { describe, expect, it } from "vitest";
import { agentFoldersFor, firstBuildMessage, isInICloud } from "../src/first-build";

describe("firstBuildMessage", () => {
  it("names every change a first build makes, and iCloud", () => {
    const text = firstBuildMessage({ notesGainingUid: 12, agentFolders: [".claude", ".cursor"], inICloud: true }).join("\n");
    expect(text).toContain("uid: line to the frontmatter of 12 notes");
    expect(text).toContain("create a .moss folder");
    expect(text).toContain("create .claude and .cursor");
    expect(text).toContain("iCloud syncs these changes");
  });

  it("leaves out what will not happen", () => {
    const text = firstBuildMessage({ notesGainingUid: 0, agentFolders: [], inICloud: false }).join("\n");
    expect(text).not.toContain("uid:");
    expect(text).not.toContain(".claude");
    expect(text).toContain("Whatever syncs this vault");
  });

  it("says 1 note, not 1 notes", () => {
    expect(firstBuildMessage({ notesGainingUid: 1, agentFolders: [], inICloud: false }).join("\n")).toContain("of 1 note:");
  });
});

describe("agentFoldersFor", () => {
  it("lists a tool's folder when it is set up at home and missing in the vault", () => {
    const have = new Set(["/h/.claude", "/h/.cursor", "/v/.cursor"]);
    expect(agentFoldersFor((p) => have.has(p), "/v", "/h")).toEqual([".claude"]);
  });
});

describe("isInICloud", () => {
  it("knows an iCloud Drive vault", () => {
    expect(isInICloud("/Users/me/Library/Mobile Documents/iCloud~md~obsidian/Documents/Vault")).toBe(true);
    expect(isInICloud("/Users/me/Notes")).toBe(false);
  });
});
