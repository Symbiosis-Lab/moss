import * as path from "node:path";
import { describe, expect, it } from "vitest";
import {
  binaryName,
  candidateBinaryPaths,
  findMossBinary,
  parseBuildFailure,
  parseCliError,
  parseServerUrls,
  parseRootJson,
  publishInAppUrl,
  publishNotice,
  siteRoot,
  type DetectEnv,
  type ServerAddress,
} from "../src/desktop/cli";

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

describe("parseServerUrls finds the URL", () => {
  it("finds the URL in real CLI output", () => {
    expect(firstUrl(REAL_STDERR)).toEqual({ url: "http://localhost:8080" });
  });

  it("handles a scanned-up port", () => {
    const out = "🌐 Preview server ready! Access at http://localhost:8083\n";
    expect(firstUrl(out)).toEqual({ url: "http://localhost:8083" });
  });

  it("returns null before the server line has arrived", () => {
    expect(firstUrl("Building website from: /x\n[ 10%] build: ...\n")).toBeNull();
  });

  it("survives chunked partial lines", () => {
    const partial = "🌐 Preview server ready! Access at http://local";
    expect(firstUrl(partial)).toBeNull();
    // A chunk boundary can also land mid-port ("…:8080" with nothing after
    // it yet is indistinguishable from "…:80801" still arriving), so
    // completion needs the line terminator too.
    expect(firstUrl(partial + "host:8080")).toBeNull();
    expect(firstUrl(partial + "host:8080\n")).not.toBeNull();
  });
});

describe("parseServerUrls", () => {
  it("yields each url in order", () => {
    const stream =
      "🌐 Preview server ready! Access at http://localhost:8081\n" +
      "🌐 Preview server ready! Access at http://localhost:8090\n";
    expect(parseServerUrls(stream)).toEqual([{ url: "http://localhost:8081" }, { url: "http://localhost:8090" }]);
  });

  it("defers a url until a chunk boundary mid-port is resolved", () => {
    const cut = "🌐 Preview server ready! Access at http://localhost:8";
    expect(parseServerUrls(cut)).toEqual([]);
    expect(parseServerUrls(cut + "080\n")).toEqual([{ url: "http://localhost:8080" }]);
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

  it("finds the program inside the moss app on macOS, which a writer who has the app already has", () => {
    const app = "/Applications/moss.app/Contents/MacOS/moss";
    expect(findMossBinary("", env({ pathVar: "/usr/bin", platform: "darwin", fileExists: (p) => p === app }))).toBe(app);
    expect(candidateBinaryPaths("", env({ platform: "linux" }))).not.toContain(app);
  });

  it("finds a per-user macOS install, and looks only on PATH on Windows", () => {
    expect(candidateBinaryPaths("", env({ platform: "darwin" }))).toContain("/home/u/Applications/moss.app/Contents/MacOS/moss");
    expect(candidateBinaryPaths("", env({ platform: "win32", pathVar: "", homeDir: "C:\\Users\\u" }))).toEqual([]);
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

describe("publishInAppUrl", () => {
  it("percent-encodes the whole folder path into the moss app's deep link", () => {
    expect(publishInAppUrl("/Users/me/My Vault/潮汐")).toBe(
      "moss://deploy?path=%2FUsers%2Fme%2FMy%20Vault%2F%E6%BD%AE%E6%B1%90",
    );
  });

  it("keeps characters that would end the query intact", () => {
    expect(publishInAppUrl("/a/b&c?d#e")).toBe("moss://deploy?path=%2Fa%2Fb%26c%3Fd%23e");
  });
});

describe("publishNotice", () => {
  it("names the address a live site publishes to", () => {
    expect(publishNotice("Garden", "garden.example")).toBe("Opening Garden in moss to publish to garden.example.");
  });

  it("names mosspub.com before the first publish", () => {
    expect(publishNotice("Garden", null)).toBe("Opening Garden in moss to publish to mosspub.com.");
  });

  it("names only the folder when the CLI cannot say where it goes", () => {
    expect(publishNotice("Garden", undefined)).toBe("Opening Garden in moss to publish.");
  });
});

describe("parseRootJson", () => {
  it("reads the site and its address", () => {
    expect(parseRootJson('{"address":"garden.example","root":"/v/Garden"}')).toEqual({ root: "/v/Garden", address: "garden.example" });
  });

  it("keeps a null address apart from one an older CLI does not print", () => {
    expect(parseRootJson('{"address":null,"root":null}')).toEqual({ root: null, address: null });
    expect(parseRootJson('{"root":"/v/Garden"}')).toEqual({ root: "/v/Garden", address: undefined });
  });

  it("reads a failed run as no site and an unknown address", () => {
    expect(parseRootJson("")).toEqual({ root: null, address: undefined });
    expect(parseRootJson('{"root":3}')).toEqual({ root: null, address: undefined });
  });
});

describe("siteRoot", () => {
  it("prefers the site moss found", () => {
    expect(siteRoot(path.posix, "/v", "notes", "/v/blog/")).toBe("/v/blog");
  });

  it("makes a clicked folder the site when none holds it, and the vault root's menu names the vault", () => {
    expect(siteRoot(path.posix, "/v", "notes/drafts", null)).toBe("/v/notes/drafts");
    expect(siteRoot(path.posix, "/v", "/", null)).toBe("/v");
    expect(siteRoot(path.posix, "/v/", undefined, null)).toBe("/v");
  });

  it("names a Windows folder the way moss root prints it", () => {
    expect(siteRoot(path.win32, "C:\\v", "notes/drafts", null)).toBe("C:\\v\\notes\\drafts");
    expect(siteRoot(path.win32, "C:\\v", "/", null)).toBe("C:\\v");
  });
});

function firstUrl(text: string): ServerAddress | null {
  return parseServerUrls(text)[0] ?? null;
}
