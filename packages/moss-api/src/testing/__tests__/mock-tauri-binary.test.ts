import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setupMockTauri, type MockTauriContext } from "../index.js";
import { writeFileBinary } from "../../utils/filesystem.js";
import { downloadAsset, httpGet } from "../../utils/http.js";

const JPEG_LIKE = new Uint8Array([0xff, 0xd8, 0x00, 0x80, 0x0a]);
const b64 = (b: Uint8Array) => btoa(String.fromCharCode(...b));

describe("mock Tauri: binary writes and downloads", () => {
  let ctx: MockTauriContext;
  beforeEach(() => {
    ctx = setupMockTauri({ projectPath: "/site", pluginName: "maps" });
  });
  afterEach(() => ctx.cleanup());

  it("records the exact bytes writeFileBinary wrote", async () => {
    await writeFileBinary("maps/a.jpg", JPEG_LIKE);
    expect(ctx.filesystem.getFile("/site/maps/a.jpg")?.bytes).toEqual(JPEG_LIKE);
  });

  it("refuses .moss/ and a path that leaves the project", async () => {
    await expect(writeFileBinary(".moss/config.toml", JPEG_LIKE)).rejects.toThrow(/\.moss/);
    await expect(writeFileBinary("../out.bin", JPEG_LIKE)).rejects.toThrow(/escapes/);
    await expect(writeFileBinary("/abs.bin", JPEG_LIKE)).rejects.toThrow(/escapes/);
    expect(ctx.filesystem.listFiles()).toEqual([]);
  });

  it("downloadAsset records the body under the requested filename", async () => {
    ctx.urlConfig.setResponse("https://iiif.example/a/default.jpg", {
      status: 200, ok: true, contentType: "image/jpeg", bodyBase64: b64(JPEG_LIKE),
    });
    const r = await downloadAsset("https://iiif.example/a/default.jpg", "maps", { filename: "a.jpg" });
    expect(r.actualPath).toBe("maps/a.jpg");
    expect(r.bytesWritten).toBe(JPEG_LIKE.length);
    expect(ctx.filesystem.getFile("/site/maps/a.jpg")?.bytes).toEqual(JPEG_LIKE);
  });

  it("downloadAsset keeps the URL-derived name by default and refuses a path as a name", async () => {
    ctx.urlConfig.setResponse("https://iiif.example/b/default.jpg", {
      status: 200, ok: true, contentType: "image/jpeg", bodyBase64: b64(JPEG_LIKE),
    });
    expect((await downloadAsset("https://iiif.example/b/default.jpg", "maps")).actualPath).toBe("maps/default.jpg");
    await expect(
      downloadAsset("https://iiif.example/b/default.jpg", "maps", { filename: "../x.jpg" })
    ).rejects.toThrow(/Invalid file name/);
  });

  it("does not record a file for a failed download", async () => {
    ctx.urlConfig.setResponse("https://iiif.example/missing.jpg", { status: 404, ok: false });
    await downloadAsset("https://iiif.example/missing.jpg", "maps");
    expect(ctx.filesystem.listFiles()).toEqual([]);
  });

  it("answers http_get from the configured responses", async () => {
    ctx.urlConfig.setResponse("https://api.example/info.json", {
      status: 200, ok: true, contentType: "application/json", bodyBase64: btoa('{"w":10}'),
    });
    const r = await httpGet("https://api.example/info.json", { headers: { Accept: "application/json" } });
    expect(r.ok).toBe(true);
    expect(JSON.parse(r.text())).toEqual({ w: 10 });
  });
});
