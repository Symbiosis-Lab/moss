import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setupMockTauri, type MockTauriContext } from "../index.js";
import { listSiteFilesWithSizes, readSiteFile } from "../../utils/filesystem.js";
import { getPluginEnvVar } from "../../utils/env.js";
import { getKey, listKeys } from "../../utils/keystore.js";
import { returnToEditor } from "../../utils/browser.js";

const SITE = "/site/.moss/build.nosync/current";

describe("mock Tauri: host commands", () => {
  let ctx: MockTauriContext;
  beforeEach(() => {
    ctx = setupMockTauri({
      projectPath: "/site",
      pluginName: "deployer",
      envVars: { MOSS_MATTERS_TEST_PROFILE: "staging", UNLISTED_VAR: "secret" },
    });
  });
  afterEach(() => ctx.cleanup());

  describe("compiled site files", () => {
    it("readSiteFile returns the built bytes as base64", async () => {
      ctx.filesystem.setFileBytes(`${SITE}/img/a.png`, new Uint8Array([0, 1, 2, 255]));
      expect(await readSiteFile("img/a.png")).toBe(btoa("\x00\x01\x02\xff"));
    });

    it("readSiteFile encodes text pages and refuses a missing file or a path leaving the site", async () => {
      ctx.filesystem.setFile(`${SITE}/index.html`, "<h1>hi</h1>");
      expect(await readSiteFile("index.html")).toBe(btoa("<h1>hi</h1>"));
      await expect(readSiteFile("nope.html")).rejects.toThrow(/Failed to read site file/);
      await expect(readSiteFile("../index.md")).rejects.toThrow(/escapes/);
    });

    it("listSiteFilesWithSizes lists only the built site, relative, with byte sizes", async () => {
      ctx.filesystem.setFile(`${SITE}/index.html`, "<h1>hi</h1>");
      ctx.filesystem.setFileBytes(`${SITE}/img/a.png`, new Uint8Array(5));
      ctx.filesystem.setFile("/site/index.md", "source, not built");
      const files = await listSiteFilesWithSizes();
      expect(files.sort((a, b) => a.path.localeCompare(b.path))).toEqual([
        { path: "img/a.png", size: 5 },
        { path: "index.html", size: 11 },
      ]);
    });
  });

  describe("getPluginEnvVar", () => {
    it("serves an allow-listed variable that is set", async () => {
      expect(await getPluginEnvVar("MOSS_MATTERS_TEST_PROFILE")).toBe("staging");
    });

    it("is undefined for an allow-listed variable that is unset and for any other name", async () => {
      expect(await getPluginEnvVar("MOSS_MATTERS_DOMAIN")).toBeUndefined();
      expect(await getPluginEnvVar("UNLISTED_VAR")).toBeUndefined();
    });
  });

  describe("keystore", () => {
    it("getKey is idempotent and returns a 32-byte public key", async () => {
      const a = await getKey("ipns", "ed25519");
      const b = await getKey("ipns", "ed25519");
      expect(a.publicKey).toHaveLength(32);
      expect(b.publicKey).toEqual(a.publicKey);
      expect(a).toMatchObject({ name: "ipns", algorithm: "ed25519" });
    });

    it("getKey refuses a different algorithm for an existing key, an unknown one, and a path-like name", async () => {
      await getKey("ipns", "ed25519");
      await expect(getKey("ipns", "secp256k1-schnorr")).rejects.toThrow(/AlgorithmMismatch/);
      await expect(getKey("x", "rsa" as never)).rejects.toThrow(/unknown key algorithm/);
      await expect(getKey("../x", "ed25519")).rejects.toThrow(/InvalidName/);
    });

    it("listKeys lists the caller's keys", async () => {
      expect(await listKeys()).toEqual([]);
      await getKey("a", "ed25519");
      await getKey("b", "secp256k1-schnorr");
      const keys = await listKeys();
      expect(keys.map((k) => [k.name, k.algorithm]).sort()).toEqual([
        ["a", "ed25519"],
        ["b", "secp256k1-schnorr"],
      ]);
    });
  });

  it("returnToEditor is recorded on the browser tracker", async () => {
    await returnToEditor();
    await returnToEditor();
    expect(ctx.browserTracker.returnToEditorCount).toBe(2);
  });
});
