import { describe, expect, it } from "vitest";
import {
  binaryName,
  buildArgs,
  candidateBinaryPaths,
  findMossBinary,
  parseBuildFailure,
  parseCliError,
  parseProblemCount,
  parseServerUrl,
  splitFlags,
  type DetectEnv,
} from "../src/cli";

// Verbatim stderr from `moss build /tmp/moss-smoke-vault --serve --no-plugins`
// (moss 0.11.1, captured 2026-08-11). The parser contract is this text.
const REAL_STDERR = `Building website from: /tmp/moss-smoke-vault
[ 10%] build: Building site...
[done] Build complete
📁 'moss-smoke-vault': Site generated at /tmp/moss-smoke-vault/.moss/build.nosync/current
🌐 Preview server ready! Access at http://localhost:8080
🤖 Coding agent: read /tmp/moss-smoke-vault/.claude/skills/moss/SKILL.md for how to author and theme this site.
Press Ctrl+C to stop the server
`;

function env(overrides: Partial<DetectEnv> = {}): DetectEnv {
  return {
    pathVar: "/usr/bin:/usr/local/bin",
    pathDelimiter: ":",
    platform: "linux",
    homeDir: "/home/u",
    fileExists: () => false,
    ...overrides,
  };
}

describe("parseServerUrl", () => {
  it("finds the URL and port in real CLI output", () => {
    expect(parseServerUrl(REAL_STDERR)).toEqual({ url: "http://localhost:8080", port: 8080 });
  });

  it("handles a scanned-up port", () => {
    const out = "🌐 Preview server ready! Access at http://localhost:8083\n";
    expect(parseServerUrl(out)).toEqual({ url: "http://localhost:8083", port: 8083 });
  });

  it("returns null before the server line has arrived", () => {
    expect(parseServerUrl("Building website from: /x\n[ 10%] build: ...\n")).toBeNull();
  });

  it("survives chunked partial lines", () => {
    const partial = "🌐 Preview server ready! Access at http://local";
    expect(parseServerUrl(partial)).toBeNull();
    expect(parseServerUrl(partial + "host:8080")).not.toBeNull();
  });
});

describe("failure and problem parsing", () => {
  it("extracts Build failed lines", () => {
    expect(parseBuildFailure("stuff\nBuild failed: boom happened\n")).toBe("boom happened");
    expect(parseBuildFailure(REAL_STDERR)).toBeNull();
  });

  it("extracts Error: lines (bad folder, plugin gate)", () => {
    expect(parseCliError("Error: Folder does not exist: /nope\n")).toBe(
      "Folder does not exist: /nope",
    );
    expect(parseCliError(REAL_STDERR)).toBeNull();
  });

  it("reads the problem summary count with singular and plural", () => {
    expect(parseProblemCount("moss: 1 problem reported above — the site was still generated.\n")).toBe(1);
    expect(parseProblemCount("moss: 12 problems reported above — failing because --strict was requested.\n")).toBe(12);
    expect(parseProblemCount(REAL_STDERR)).toBe(0);
  });
});

describe("buildArgs", () => {
  it("builds a plain build", () => {
    expect(buildArgs("/v")).toEqual(["build", "/v"]);
  });

  it("builds serve+watch with extra flags", () => {
    expect(buildArgs("/v", { serve: true, watch: true, extraFlags: ["--no-plugins"] })).toEqual([
      "build",
      "/v",
      "--serve",
      "--watch",
      "--no-plugins",
    ]);
  });

  it("drops empty extra flags", () => {
    expect(buildArgs("/v", { extraFlags: ["", "--strict"] })).toEqual(["build", "/v", "--strict"]);
  });
});

describe("splitFlags", () => {
  it("splits on whitespace and drops empties", () => {
    expect(splitFlags("  --no-plugins   --strict ")).toEqual(["--no-plugins", "--strict"]);
    expect(splitFlags("")).toEqual([]);
  });
});

describe("binary detection", () => {
  it("uses the platform binary name", () => {
    expect(binaryName("win32")).toBe("moss.exe");
    expect(binaryName("darwin")).toBe("moss");
  });

  it("prefers the explicit settings path", () => {
    const e = env({ fileExists: (p) => p === "/custom/moss" });
    expect(findMossBinary("/custom/moss", e)).toBe("/custom/moss");
  });

  it("falls back to PATH entries in order", () => {
    const e = env({ fileExists: (p) => p === "/usr/local/bin/moss" });
    expect(findMossBinary("", e)).toBe("/usr/local/bin/moss");
  });

  it("probes Homebrew and npm-global dirs beyond PATH (Finder-launched apps)", () => {
    const e = env({ pathVar: "/usr/bin", platform: "darwin", fileExists: (p) => p === "/opt/homebrew/bin/moss" });
    expect(findMossBinary("", e)).toBe("/opt/homebrew/bin/moss");
    expect(candidateBinaryPaths("", e)).toContain("/home/u/.local/bin/moss");
  });

  it("returns null when nothing exists", () => {
    expect(findMossBinary("", env())).toBeNull();
  });

  it("de-duplicates candidates", () => {
    const e = env({ pathVar: "/usr/local/bin:/usr/local/bin" });
    const candidates = candidateBinaryPaths("", e);
    expect(new Set(candidates).size).toBe(candidates.length);
  });
});
