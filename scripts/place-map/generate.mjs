#!/usr/bin/env node
/* Offline place-map pack generator. The source cache is the only input. */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { deflateRawSync } from "node:zlib";
import { resolve, dirname, basename } from "node:path";
import shp from "shpjs";
import h5wasm from "h5wasm/node";
import { contours } from "d3-contour";

const ROOT = resolve(import.meta.dirname, "../..");
const MANIFEST = resolve(ROOT, "crates/moss-build/data/place-map/source-manifest.toml");
const CACHE = resolve(process.env.MOSS_PLACE_MAP_SOURCE_DIR ?? "/private/tmp/moss-place-map-sources");
const OUT = resolve(ROOT, "crates/moss-build/data/place-map/place-map-v1.bin");
const REPORT = resolve(ROOT, "crates/moss-build/data/place-map/place-map-size.json");
const QUANT = 10000;
const POLE_EPSILON = 1e-9;
const POLE_CAP = 89.999;
const MAGIC = Buffer.from("MOSSPLM1", "ascii");
const SCHEMA = 1;
const TILE_DEGREES = 10;
const LAYERS = [
  [1, "coast", 2], [2, "land", 1], [3, "lakes", 1], [4, "rivers", 2],
  [5, "ice", 1], [6, "reefs", 1], [7, "salt_flats", 1], [8, "built_up", 1],
  [9, "relief", 1], [10, "sea_floor", 1],
];
const LAYER_ID = new Map(LAYERS.map(([id, name]) => [name, id]));
const SOURCES = {
  coast: "ne_10m_coastline.zip", land: "ne_10m_land.zip", lakes: "ne_10m_lakes.zip",
  rivers: "ne_10m_rivers_lake_centerlines_scale_rank.zip", ice: ["ne_10m_glaciated_areas.zip", "ne_10m_antarctic_ice_shelves_polys.zip"],
  reefs: "ne_10m_reefs.zip", salt_flats: "ne_10m_playas.zip", built_up: "ne_10m_urban_areas.zip",
  bathymetry: "ne_10m_bathymetry_all.zip", relief: "earth_relief_06m_g.grd",
};
const TOLERANCE = { world: 0.98, fine: 0.3 };
const BUILT_UP_TOLERANCE = { world: 1.2, fine: 0.4 };
const SEA_FLOOR_THRESHOLDS = [-6000, -4000, -2000, -1000, -500, -250, -100, -10];
const RELIEF_THRESHOLDS = [100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000];
const SOURCE_MASKS = [1, 2, 4, 8, 48, 64, 128, 512, 1024, 1280];

function fail(message) { throw new Error(message); }
function u16(value) { const b = Buffer.alloc(2); b.writeUInt16LE(value); return b; }
function i16(value) { const b = Buffer.alloc(2); b.writeInt16LE(value); return b; }
function u32(value) { const b = Buffer.alloc(4); b.writeUInt32LE(value); return b; }
function i32(value) { const b = Buffer.alloc(4); b.writeInt32LE(value); return b; }
function hash(bytes) { return createHash("sha256").update(bytes).digest(); }

function parseToml(text) {
  const sources = [];
  const manifest = { sources };
  let current = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) continue;
    if (line === "[[sources]]") { current = {}; sources.push(current); continue; }
    const match = line.match(/^([A-Za-z0-9_]+)\s*=\s*(.*)$/);
    if (!match) continue;
    const [, key, value] = match;
    const quoted = value.match(/^"(.*)"$/);
    (current ?? manifest)[key] = quoted ? quoted[1] : Number(value);
  }
  return manifest;
}

async function verifySources() {
  const manifestBytes = await readFile(MANIFEST);
  const manifest = parseToml(manifestBytes.toString("utf8"));
  if (manifest.schema !== 1 || manifest.gmt_dataset !== "earth_relief_06m_g") fail("unsupported source manifest");
  if (manifest.sources.length !== 11) fail("source manifest has an unexpected source count");
  for (const source of manifest.sources) {
    const path = resolve(CACHE, source.artifact);
    if (dirname(path) !== CACHE || basename(path) !== source.artifact) fail(`unsafe source path ${source.artifact}`);
    const bytes = await readFile(path);
    if (bytes.length !== Number(source.size_bytes)) fail(`${source.name}: size mismatch`);
    if (hash(bytes).toString("hex") !== source.sha256) fail(`${source.name}: SHA-256 mismatch`);
  }
  return { manifest, manifestBytes, manifestDigest: hash(manifestBytes) };
}

function verifyCanonicalSources() {
  const verifier = resolve(import.meta.dirname, "generate.py");
  const result = spawnSync("python3", [verifier, "--check", "--source-dir", CACHE], { stdio: "inherit" });
  if (result.error) fail(`canonical source verifier failed to start: ${result.error.message}`);
  if (result.status !== 0) fail(`canonical source verifier exited with status ${result.status}`);
}

function numberPoint(point) {
  if (!Array.isArray(point) || point.length < 2 || !Number.isFinite(point[0]) || !Number.isFinite(point[1])) fail(`invalid WGS84 coordinate: ${JSON.stringify(point)}`);
  let lon = point[0];
  while (lon < -180) lon += 360;
  while (lon >= 180) lon -= 360;
  if (point[1] < -90 || point[1] > 90) fail(`geometry latitude outside WGS84: ${point[1]}`);
  // A pole has no unique longitude. Keep a pole-touching ring finite and
  // deterministic by explicitly capping it just inside the pole; values
  // outside WGS84 are rejected above rather than silently clamped.
  const lat = point[1] >= 90 - POLE_EPSILON ? POLE_CAP : point[1] <= -90 + POLE_EPSILON ? -POLE_CAP : point[1];
  return [lon, lat];
}

function distance(point, a, b) {
  const [x, y] = point; const [ax, ay] = a; const [bx, by] = b;
  const dx = bx - ax; const dy = by - ay;
  if (dx === 0 && dy === 0) return Math.hypot(x - ax, y - ay);
  const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)));
  return Math.hypot(x - (ax + t * dx), y - (ay + t * dy));
}

function rdp(points, tolerance) {
  if (points.length <= 2) return points;
  let max = tolerance; let index = -1;
  for (let i = 1; i < points.length - 1; i++) {
    const d = distance(points[i], points[0], points[points.length - 1]);
    if (d > max) { max = d; index = i; }
  }
  if (index < 0) return [points[0], points.at(-1)];
  return [...rdp(points.slice(0, index + 1), tolerance).slice(0, -1), ...rdp(points.slice(index), tolerance)];
}

function splitDateline(points) {
  const output = []; let part = [];
  const add = (point) => { if (!part.length || point[0] !== part.at(-1)[0] || point[1] !== part.at(-1)[1]) part.push(point); };
  for (let i = 0; i < points.length; i++) {
    const point = numberPoint(points[i]);
    if (part.length) {
      const previous = part.at(-1); const delta = point[0] - previous[0];
      if (Math.abs(delta) > 180) {
        const wrapped = delta > 0 ? point[0] - 360 : point[0] + 360;
        const crossing = delta > 0 ? -180 : 180;
        const denominator = wrapped - previous[0];
        const fraction = Math.abs(denominator) < 1e-9 ? 0 : Math.max(0, Math.min(1, (crossing - previous[0]) / denominator));
        const y = previous[1] + (point[1] - previous[1]) * fraction;
        add([crossing, y]); output.push(part); part = [[crossing === 180 ? -180 : 180, y]];
      }
    }
    add(point);
  }
  if (part.length > 1) output.push(part);
  return output;
}

function ringArea(points) {
  let area = 0;
  for (let i = 0; i < points.length - 1; i++) {
    area += points[i][0] * points[i + 1][1] - points[i + 1][0] * points[i][1];
  }
  return area / 2;
}

function clipRing(points, boundary, keepGreater) {
  const output = [];
  const inside = (point) => keepGreater ? point[0] >= boundary - 1e-9 : point[0] <= boundary + 1e-9;
  const intersection = (a, b) => {
    const denominator = b[0] - a[0];
    const fraction = Math.abs(denominator) < 1e-12 ? 0 : (boundary - a[0]) / denominator;
    return [boundary, a[1] + (b[1] - a[1]) * fraction];
  };
  let previous = points.at(-1);
  for (const point of points) {
    const previousInside = inside(previous); const pointInside = inside(point);
    if (pointInside !== previousInside) output.push(intersection(previous, point));
    if (pointInside) output.push(point);
    previous = point;
  }
  return output;
}

function unwrapRing(points) {
  const result = [points[0]];
  for (let index = 1; index < points.length; index++) {
    const previous = result.at(-1); let lon = points[index][0];
    while (lon - previous[0] > 180) lon -= 360;
    while (lon - previous[0] < -180) lon += 360;
    if (Math.abs(lon - previous[0]) > 180) fail("polygon edge spans more than half the globe");
    result.push([lon, points[index][1]]);
  }
  return result;
}

function cleanRing(points) {
  const unique = points.filter((point, index) => index === 0 || point[0] !== points[index - 1][0] || point[1] !== points[index - 1][1]);
  if (unique.length > 1 && unique[0][0] === unique.at(-1)[0] && unique[0][1] === unique.at(-1)[1]) unique.pop();
  return unique;
}

function normalizeRing(coords, tolerance, outer) {
  const points = coords.map(numberPoint);
  if (points.length < 3) return [];
  const source = points[0][0] === points.at(-1)[0] && points[0][1] === points.at(-1)[1] ? points.slice(0, -1) : points;
  const unwrapped = unwrapRing(source);
  const latitudes = unwrapped.map(([, lat]) => lat);
  if (Math.max(...latitudes) - Math.min(...latitudes) >= 179.998) fail("polygon spans both poles");
  const minimum = Math.min(...unwrapped.map(([lon]) => lon));
  const maximum = Math.max(...unwrapped.map(([lon]) => lon));
  const firstWindow = Math.floor((minimum + 180) / 360);
  const lastWindow = Math.floor((maximum + 180) / 360);
  const output = [];
  for (let window = firstWindow; window <= lastWindow; window++) {
    const west = -180 + window * 360; const east = 180 + window * 360;
    let clipped = clipRing(unwrapped, west, true);
    clipped = clipRing(clipped, east, false);
    clipped = cleanRing(clipped);
    if (clipped.length < 3) continue;
    const simplified = cleanRing(rdp(clipped, tolerance));
    if (simplified.length < 3) continue;
    const ring = simplified.map(([lon, lat]) => [lon - window * 360, lat]);
    if (ring[0][0] !== ring.at(-1)[0] || ring[0][1] !== ring.at(-1)[1]) ring.push(ring[0]);
    const area = ringArea(ring);
    if (Math.abs(area) < 1e-9) continue;
    if ((area > 0) !== outer) ring.reverse();
    output.push(ring);
  }
  return output;
}

function geometryParts(geometry, tolerance, polygon = false) {
  if (!geometry) return [];
  if (geometry.type === "Polygon") return geometry.coordinates.flatMap((ring, index) => normalizeRing(ring, tolerance, index === 0));
  if (geometry.type === "MultiPolygon") return geometry.coordinates.flatMap((polygonRings) => polygonRings.flatMap((ring, index) => normalizeRing(ring, tolerance, index === 0)));
  const collect = (coords, isPolygon) => {
    if (!coords?.length) return [];
    if (typeof coords[0][0] === "number") {
      const split = splitDateline(coords);
      return split.map((part) => {
        const closed = isPolygon && part.length > 2 && part[0][0] === part.at(-1)[0] && part[0][1] === part.at(-1)[1];
        const simplified = rdp(closed ? part.slice(0, -1) : part, tolerance);
        if (simplified.length < 2) return null;
        if (closed && simplified.length > 2) simplified.push(simplified[0]);
        return simplified;
      }).filter(Boolean);
    }
    return coords.flatMap((item) => collect(item, isPolygon));
  };
  return collect(geometry.coordinates, geometry.type.includes("Polygon") || polygon);
}

async function readGeoJson(source) {
  const bytes = await readFile(resolve(CACHE, source));
  const value = await shp(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  return Array.isArray(value) ? value.flatMap((x) => x.features ?? []) : value.features ?? [];
}

function encodeVarint(value) {
  const result = []; let x = BigInt(value >>> 0);
  while (x >= 128n) { result.push(Number(x & 127n) | 128); x >>= 7n; }
  result.push(Number(x)); return Buffer.from(result);
}
function zigzag(value) { return value < 0 ? (-value * 2) - 1 : value * 2; }

function encodeFeature(parts) {
  const chunks = [u16(parts.length)]; let previousX = 0; let previousY = 0;
  for (const part of parts) {
    chunks.push(u32(part.length));
    for (const [lon, lat] of part) {
      const x = Math.round(lon * QUANT); const y = Math.round(lat * QUANT);
      chunks.push(encodeVarint(zigzag(x - previousX)), encodeVarint(zigzag(y - previousY)));
      previousX = x; previousY = y;
    }
  }
  return Buffer.concat(chunks);
}

function featuresFor(features, tolerance) {
  const output = [];
  for (const feature of features) {
    const parts = geometryParts(feature.geometry, tolerance);
    if (!parts.length) continue;
    const flat = parts.flat();
    const bounds = [Math.min(...flat.map((x) => x[0])), Math.min(...flat.map((x) => x[1])), Math.max(...flat.map((x) => x[0])), Math.max(...flat.map((x) => x[1]))];
    const rawBand = feature.properties?.band ?? (feature.properties?.depth === undefined ? 0 : -feature.properties.depth);
    const band = Number.isFinite(rawBand) ? Math.max(-32768, Math.min(32767, Math.round(rawBand))) : 0;
    output.push({ parts, bounds, band, bytes: encodeFeature(parts) });
  }
  return output;
}

function reliefBands(z, lon, lat) {
  const stride = 5; // 0.5 degree samples from the 0.1 degree source grid.
  const nx = Math.floor((lon.length - 1) / stride) + 1; const ny = Math.floor((lat.length - 1) / stride) + 1;
  const values = new Float32Array(nx * ny);
  for (let y = 0; y < ny; y++) for (let x = 0; x < nx; x++) values[y * nx + x] = z[Math.min(y * stride, lat.length - 1) * lon.length + Math.min(x * stride, lon.length - 1)];
  const project = (geometry) => geometry.map((polygon) => polygon.map((ring) => ring.map(([x, y]) => [lon[0] + x * stride * 0.1, lat[0] + y * stride * 0.1])));
  const make = (name, thresholds) => thresholds.flatMap((threshold) => contours().size([nx, ny]).thresholds([threshold])(values).flatMap((contour) => [{ properties: { band: threshold }, geometry: { type: contour.type, coordinates: project(contour.coordinates) } }]));
  return [{ name: "relief", features: make("relief", RELIEF_THRESHOLDS) }, { name: "sea_floor", features: make("sea_floor", SEA_FLOOR_THRESHOLDS) }];
}

async function makeLayers() {
  const raw = {};
  for (const [name, source] of Object.entries(SOURCES)) {
    if (name === "ice") raw[name] = [...await readGeoJson(source[0]), ...await readGeoJson(source[1])];
    else if (name !== "relief") raw[name] = await readGeoJson(source);
  }
  const h5 = await import("h5wasm/node"); await h5.default.ready;
  const file = new h5.default.File(resolve(CACHE, SOURCES.relief), "r");
  const lon = file.get("lon").value; const lat = file.get("lat").value; const z = file.get("z").value;
  const bands = reliefBands(z, lon, lat);
  file.close();
  const layers = new Map();
  const sourceLayers = { coast: "coast", land: "land", lakes: "lakes", rivers: "rivers", ice: "ice", reefs: "reefs", salt_flats: "salt_flats", built_up: "built_up" };
  for (const [name, sourceName] of Object.entries(sourceLayers)) layers.set(name, raw[sourceName]);
  layers.set("relief", bands.find((band) => band.name === "relief").features);
  const bathymetry = raw.bathymetry.filter((feature) => feature.properties?.depth === 6000);
  layers.set("sea_floor", [...bands.find((band) => band.name === "sea_floor").features, ...bathymetry]);
  return layers;
}

function tierPayload(layers, tolerance) {
  const pieces = []; const refs = []; const layerBytes = {}; let featureRef = 0;
  for (const [name, features] of layers) {
    const featureTolerance = name === "built_up" ? BUILT_UP_TOLERANCE[tolerance] : TOLERANCE[tolerance];
    const normalized = featuresFor(features, featureTolerance);
    const body = [];
    for (const feature of normalized) {
      body.push(u32(feature.bytes.length), ...feature.bounds.map((x) => i32(Math.round(x * QUANT))), i16(feature.band), feature.bytes);
      refs.push({ feature: featureRef++, name, bounds: feature.bounds });
    }
    const content = Buffer.concat(body);
    const layerId = LAYER_ID.get(name); if (!layerId) fail(`unknown layer ${name}`);
    pieces.push(Buffer.concat([Buffer.from([layerId, 1]), u32(normalized.length), u32(content.length), content]));
    layerBytes[name] = content.length;
  }
  return { bytes: Buffer.concat(pieces), refs, layerBytes };
}

function tileIndex(refs) {
  const tiles = new Map();
  for (const ref of refs) {
    const minX = Math.floor((ref.bounds[0] + 180) / TILE_DEGREES); const maxX = Math.floor((ref.bounds[2] + 180) / TILE_DEGREES);
    const minY = Math.floor((ref.bounds[1] + 90) / TILE_DEGREES); const maxY = Math.floor((ref.bounds[3] + 90) / TILE_DEGREES);
    for (let y = minY; y <= maxY; y++) for (let x = minX; x <= maxX; x++) {
      const key = `${Math.max(0, Math.min(35, x))},${Math.max(0, Math.min(17, y))}`;
      if (!tiles.has(key)) tiles.set(key, []); tiles.get(key).push(ref.feature);
    }
  }
  const entries = [...tiles.entries()].sort(([a], [b]) => { const [ax, ay] = a.split(",").map(Number); const [bx, by] = b.split(",").map(Number); return ax - bx || ay - by; });
  const chunks = [u32(entries.length)];
  for (const [key, values] of entries) { const [x, y] = key.split(",").map(Number); const unique = [...new Set(values)].sort((a, b) => a - b); chunks.push(i16(x), i16(y), u32(unique.length), ...unique.map(u32)); }
  return Buffer.concat(chunks);
}

async function main() {
  const { manifestBytes, manifestDigest } = await verifySources();
  const layers = await makeLayers();
  const world = tierPayload(layers, "world"); const fine = tierPayload(layers, "fine");
  const index = tileIndex(fine.refs);
  const sourceMasks = SOURCE_MASKS.map(u16);
  const header = Buffer.concat([MAGIC, u16(SCHEMA), u16(0), u32(QUANT), i32(-180 * QUANT), i32(180 * QUANT), i32(-90 * QUANT), i32(90 * QUANT), u16(LAYERS.length), u16(2), u16(TILE_DEGREES), u16(0), manifestDigest, ...sourceMasks]);
  const tiers = Buffer.concat([Buffer.from([0, 0]), u32(world.refs.length), u32(world.bytes.length), world.bytes, Buffer.from([1, 0]), u32(fine.refs.length), u32(fine.bytes.length), fine.bytes, index]);
  const bytes = Buffer.concat([header, tiers]);
  bytes.writeUInt16LE(header.length, 10);
  await mkdir(dirname(OUT), { recursive: true }); await writeFile(OUT, bytes);
  const budgetBytes = 2_883_584;
  const marginBytes = budgetBytes - bytes.length;
  const marginPercent = (marginBytes / budgetBytes) * 100;
  const report = { schema: SCHEMA, manifest_sha256: manifestDigest.toString("hex"), bytes: bytes.length, compressed_bytes: deflateRawSync(bytes, { level: 9 }).length, budget_bytes: budgetBytes, margin_bytes: marginBytes, margin_percent: Number(marginPercent.toFixed(3)), tiers: { world: world.bytes.length, fine: fine.bytes.length }, index: index.length, layers: { counts: Object.fromEntries([...layers].map(([name, features]) => [name, features.length])), world_bytes: world.layerBytes, fine_bytes: fine.layerBytes } };
  await writeFile(REPORT, `${JSON.stringify(report, null, 2)}\n`);
  if (bytes.length > report.budget_bytes) fail(`place-map pack is ${bytes.length} bytes, over ${report.budget_bytes}`);
  if (marginPercent < 5) fail(`place-map pack has only ${marginPercent.toFixed(3)}% budget margin; need at least 5%`);
  console.log(JSON.stringify(report, null, 2));
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    verifyCanonicalSources();
    await main();
  } catch (error) {
    console.error(error.stack ?? `place-map generation failed: ${error.message}`);
    process.exitCode = 1;
  }
}

export {
  encodeFeature,
  geometryParts,
  reliefBands,
  rdp,
  RELIEF_THRESHOLDS,
  SEA_FLOOR_THRESHOLDS,
  splitDateline,
  tileIndex,
};
