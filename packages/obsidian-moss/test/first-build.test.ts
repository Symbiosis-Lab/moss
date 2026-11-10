import { describe, expect, it } from "vitest";
import { firstBuildMessage, isInICloud } from "../src/desktop/first-build";

describe("firstBuildMessage", () => {
  it("says a first build adds only .moss, and that iCloud copies it", () => {
    const text = firstBuildMessage({ folder: "Garden", inICloud: true }).join("\n");
    expect(text).toContain("Building Garden will create a .moss folder in it, and change no note.");
    expect(text).toContain("iCloud syncs these changes");
    expect(text).not.toContain("uid");
  });

  it("names the folder's own sync outside iCloud", () => {
    expect(firstBuildMessage({ folder: "Garden", inICloud: false }).join("\n")).toContain("Whatever syncs Garden copies");
  });
});

describe("isInICloud", () => {
  it("knows an iCloud Drive vault", () => {
    expect(isInICloud("/Users/me/Library/Mobile Documents/iCloud~md~obsidian/Documents/Vault")).toBe(true);
    expect(isInICloud("/Users/me/Notes")).toBe(false);
  });
});
