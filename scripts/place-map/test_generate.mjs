import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { encodeFeature, geometryParts, rdp, splitDateline } from "./generate.mjs";

test("quantised feature encoding is deterministic", () => {
  const first = encodeFeature([[[0, 0], [1, 1], [2, 1]]]);
  const second = encodeFeature([[[0, 0], [1, 1], [2, 1]]]);
  assert.deepEqual(first, second);
  assert.equal(first.toString("hex"), "0100030000000000a09c01a09c01a09c0100");
});

test("simplification retains endpoints and dateline splitting never makes a world-spanning segment", () => {
  assert.deepEqual(rdp([[0, 0], [0.01, 0.01], [1, 1]], 0.1), [[0, 0], [1, 1]]);
  const pieces = splitDateline([[179, 0], [-179, 1]]);
  assert.equal(pieces.length, 2);
  assert.ok(Math.abs(pieces[0].at(-1)[0]) === 180);
  assert.ok(Math.abs(pieces[1][0][0]) === 180);
});

test("polygon dateline pieces are closed, bounded, and consistently wound", () => {
  const parts = geometryParts({ type: "Polygon", coordinates: [[
    [179, -10], [-179, -10], [-179, 10], [179, 10], [179, -10],
  ]] }, 0.01);
  assert.equal(parts.length, 2);
  for (const ring of parts) {
    assert.deepEqual(ring[0], ring.at(-1));
    assert.ok(ring.every(([lon, lat]) => lon >= -180 && lon <= 180 && lat >= -90 && lat <= 90));
    for (let i = 1; i < ring.length; i++) assert.ok(Math.abs(ring[i][0] - ring[i - 1][0]) <= 180);
    let area = 0;
    for (let i = 1; i < ring.length; i++) area += ring[i - 1][0] * ring[i][1] - ring[i][0] * ring[i - 1][1];
    assert.ok(area > 0);
  }
});

test("polygon holes use the opposite winding and malformed or pole-spanning coordinates fail", () => {
  const [outer, hole] = geometryParts({ type: "Polygon", coordinates: [
    [[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]],
    [[1, 1], [1, 2], [2, 2], [2, 1], [1, 1]],
  ] }, 0.01);
  const signedArea = (ring) => ring.slice(1).reduce((sum, point, index) => sum + ring[index][0] * point[1] - point[0] * ring[index][1], 0);
  assert.ok(signedArea(outer) > 0);
  assert.ok(signedArea(hole) < 0);
  assert.throws(() => geometryParts({ type: "Polygon", coordinates: [[[0, 0], [1, 100], [0, 0]]] }, 0.01), /latitude outside WGS84/);
  assert.throws(() => geometryParts({ type: "Polygon", coordinates: [[[-180, -90], [180, 90], [0, 0], [-180, -90]]] }, 0.01), /spans both poles/);
});

test("near-pole floating point noise is capped before encoding", () => {
  const parts = geometryParts({ type: "Polygon", coordinates: [[
    [0, 89.99999999999994], [1, 89], [0, 88], [0, 89.99999999999994],
  ]] }, 0.000001);
  assert.equal(Math.max(...parts.flat().map(([, lat]) => lat)), 89.999);
});

test("the executable generator invokes the canonical source verifier", async () => {
  const packageJson = JSON.parse(await readFile(new URL("./package.json", import.meta.url)));
  assert.equal(packageJson.scripts.generate, "node generate.mjs");
  const result = spawnSync(process.execPath, ["generate.mjs"], {
    cwd: fileURLToPath(new URL(".", import.meta.url)),
    env: { ...process.env, MOSS_PLACE_MAP_SOURCE_DIR: "/private/tmp/moss-place-map-no-such-cache" },
    encoding: "utf8",
  });
  assert.notEqual(result.status, 0);
  assert.match(`${result.stdout}\n${result.stderr}`, /missing cached source|canonical source verifier exited/);
});

test("checked-in pack remains within its raw-size budget and has the versioned header", async () => {
  const bytes = await readFile(new URL("../../crates/moss-build/data/place-map/place-map-v1.bin", import.meta.url));
  const report = JSON.parse(await readFile(new URL("../../crates/moss-build/data/place-map/place-map-size.json", import.meta.url)));
  assert.equal(bytes.subarray(0, 8).toString("ascii"), "MOSSPLM1");
  assert.equal(bytes.readUInt16LE(8), 1);
  assert.equal(report.bytes, bytes.length);
  assert.ok(report.margin_percent >= 5);
  assert.ok(bytes.length <= report.budget_bytes);
});
