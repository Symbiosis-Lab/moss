#!/usr/bin/env node
/* Offline place-map pack generator. The source cache is the only input. */

import { readFile, writeFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { brotliCompressSync, constants as zlibConstants } from "node:zlib";
import { resolve, dirname, basename, join } from "node:path";
import shp from "shpjs";
import h5wasm from "h5wasm/node";
import { contours } from "d3-contour";

const ROOT = resolve(import.meta.dirname, "../..");
const MANIFEST = resolve(ROOT, "crates/moss-build/data/place-map/source-manifest.toml");
const CACHE = resolve(process.env.MOSS_PLACE_MAP_SOURCE_DIR ?? "/private/tmp/moss-place-map-sources");
// The checked-in artifact is the brotli-compressed pack, not the raw
// MOSSPLM1 bytes: the moss binary embeds this file verbatim and decodes it
// once at runtime (place_map.rs's `embedded()`), so what's on disk here is
// exactly what the app carries.
const OUT = resolve(ROOT, "crates/moss-build/data/place-map/place-map-v1.bin.br");
const REPORT = resolve(ROOT, "crates/moss-build/data/place-map/place-map-size.json");
// Coordinates are stored in thousandths of a degree: 0.07 px on a
// 10-degree locator, far below anything visible, and a vertex a couple of
// pixels from the last one encodes as one varint byte per axis instead of
// two. Ten-thousandths bought nothing on screen and cost about a quarter of
// the pack.
const QUANT = 1000;
const POLE_EPSILON = 1e-9;
const POLE_CAP = 89.999;
const MAGIC = Buffer.from("MOSSPLM1", "ascii");
// Schema 2 (2026-09): the pack no longer stores a coast (layer 1) record.
// Natural Earth's coastline and land datasets trace the same digitized
// shoreline, so storing both independently duplicated the pack's single
// largest chunk of geometry; land is now simplified at coast's own finer
// tolerance (see featureTolerance below) and the Rust decoder derives every
// coast stroke from land's ring boundary instead — place_map.rs's
// decode_tier and TileSelection::features (geometry.rs).
const SCHEMA = 2;
const TILE_DEGREES = 10;
const LAYERS = [
  [2, "land", 1], [3, "lakes", 1], [4, "rivers", 2],
  [5, "ice", 1], [6, "reefs", 1], [7, "salt_flats", 1], [8, "built_up", 1],
  [9, "relief", 1], [10, "sea_floor", 1],
];
const LAYER_ID = new Map(LAYERS.map(([id, name]) => [name, id]));
const SOURCES = {
  land: "ne_10m_land.zip", lakes: "ne_10m_lakes.zip",
  rivers: "ne_10m_rivers_lake_centerlines_scale_rank.zip", ice: ["ne_10m_glaciated_areas.zip", "ne_10m_antarctic_ice_shelves_polys.zip"],
  reefs: "ne_10m_reefs.zip", salt_flats: "ne_10m_playas.zip", built_up: "ne_10m_urban_areas.zip",
  bathymetry: "ne_10m_bathymetry_all.zip", relief: "earth_relief_06m_g.grd",
};
// Eight 90x90-degree gridline-registered tiles making up the 3-arcminute
// GMT relief grid (source-manifest.toml earth_relief_03m_g_*); the locator
// tier contours this at native resolution instead of the 06m grid's stride.
// Filenames name the tile's southwest corner: N00 spans lat 0..90, S90
// spans lat -90..0; each longitude tag spans 90 degrees starting there.
const RELIEF_FINE_TILES = [
  { artifact: "N00W180.earth_relief_03m_g.jp2", latMin: 0, lonMin: -180 },
  { artifact: "N00W090.earth_relief_03m_g.jp2", latMin: 0, lonMin: -90 },
  { artifact: "N00E000.earth_relief_03m_g.jp2", latMin: 0, lonMin: 0 },
  { artifact: "N00E090.earth_relief_03m_g.jp2", latMin: 0, lonMin: 90 },
  { artifact: "S90W180.earth_relief_03m_g.jp2", latMin: -90, lonMin: -180 },
  { artifact: "S90W090.earth_relief_03m_g.jp2", latMin: -90, lonMin: -90 },
  { artifact: "S90E000.earth_relief_03m_g.jp2", latMin: -90, lonMin: 0 },
  { artifact: "S90E090.earth_relief_03m_g.jp2", latMin: -90, lonMin: 90 },
];
const RELIEF_FINE_TILE_POINTS = 1801; // 90deg at 0.05deg (3 arcmin) spacing, gridline-registered.
const RELIEF_FINE_SPACING = 0.05;
const RELIEF_WORLD_SPACING = 0.1; // 06m native spacing.
const RELIEF_FINE_SIGMA = 1.2; // grid cells; suppresses marching-squares noise before tracing native-res contours.
// The world tier traces the 6-arcminute grid at native resolution, blurred
// by 3 cells (0.3 degrees, under a pixel of the world map). Sampling every
// fifth cell unblurred instead aliased the terrain into star-shaped shards
// and hundreds of pixel-sized islands, drawn as slivers on the world map.
const RELIEF_WORLD_STRIDE = 1;
// Wrapped columns added on each side of a global grid before contouring.
const SEAM_PAD = 2;
const RELIEF_WORLD_SIGMA = 3;
// Locator ("fine") frames render about 10 degrees across 720 device px.
const FINE_PX = 10 / 720;
const TOLERANCE = { world: 0.98, fine: 2 * FINE_PX };
// World-tier relief and sea-floor bands are simplified by area at this
// tolerance squared: about 2 px^2 on the world map, which draws some 2.3 px
// per degree. The 0.98-degree tolerance the other world layers use means
// about 5 px^2 there, and left 1,300 relief edges longer than 12 px.
const WORLD_BAND_TOLERANCE = 0.6;
// The world tier keeps only the trunk rivers the world map draws
// (scalerank 0-3; the renderer's WORLD_MAX_RIVER_RANK); the globe inset
// draws no rivers.
const WORLD_MAX_RIVER_RANK = 3;
const COAST_TOLERANCE = { world: 0.98, fine: FINE_PX };
const BUILT_UP_TOLERANCE = { world: 1.2, fine: 2 * FINE_PX };
const SEA_FLOOR_THRESHOLDS = [-6000, -5000, -4000, -3000, -2000, -1000, -200, -100, -50, -30, -20, -10];
const RELIEF_THRESHOLDS = [100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000];
// Per-layer bitmask of the logical source datasets behind it (opaque to the
// Rust decoder). Bit 10 is the 06m world-tier GMT grid; bit 11 is the
// 03m locator-tier GMT tile set added for the approved-resolution fix.
// Index 0 (coast) mirrors index 1 (land)'s mask (bit 2) rather than the old
// standalone coastline-dataset bit: schema 2 derives every coast stroke
// from land's own rings, so land is what actually feeds it now.
const SOURCE_MASKS = [2, 2, 4, 8, 48, 64, 128, 512, 1024 + 2048, 256 + 1024 + 2048];

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
  if (manifest.schema !== 1 || manifest.gmt_dataset !== "earth_relief_06m_g" || manifest.gmt_dataset_fine !== "earth_relief_03m_g") fail("unsupported source manifest");
  if (manifest.sources.length !== 18) fail("source manifest has an unexpected source count");
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

function numberPoint(point, wrapLongitude = true) {
  if (!Array.isArray(point) || point.length < 2 || !Number.isFinite(point[0]) || !Number.isFinite(point[1])) fail(`invalid WGS84 coordinate: ${JSON.stringify(point)}`);
  let lon = point[0];
  while (wrapLongitude && lon < -180) lon += 360;
  while (wrapLongitude && lon >= 180) lon -= 360;
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

const triangleArea = (a, b, c) => Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2;

// Visvalingam-Whyatt simplification of a closed ring (no repeated end
// point), the way topojson-simplify does it: repeatedly drop the vertex
// whose triangle with its neighbours has the smallest area, never letting
// an effective area fall below one already dropped, until every vertex left
// spans at least `minArea`. Douglas-Peucker instead keeps only the vertices
// farthest from a chord, so a contour that meanders within the tolerance of
// a long chord (a band edge across a flat plain) collapses into one long
// straight edge; an area criterion keeps a vertex whose small deviation
// spans a long base, so curves stay curves.
function visvalingam(ring, minArea) {
  const n = ring.length;
  if (n <= 3) return ring.slice();
  const previous = new Int32Array(n); const next = new Int32Array(n);
  const area = new Float64Array(n); const alive = new Uint8Array(n).fill(1);
  for (let i = 0; i < n; i++) { previous[i] = (i - 1 + n) % n; next[i] = (i + 1) % n; }
  const heap = [];
  const push = (value, index) => {
    heap.push([value, index]);
    for (let k = heap.length - 1; k > 0;) { const parent = (k - 1) >> 1; if (heap[parent][0] <= heap[k][0]) break; [heap[parent], heap[k]] = [heap[k], heap[parent]]; k = parent; }
  };
  const pop = () => {
    const top = heap[0]; const last = heap.pop();
    if (heap.length) {
      heap[0] = last;
      for (let k = 0; ;) {
        const left = 2 * k + 1; const right = left + 1; let smallest = k;
        if (left < heap.length && heap[left][0] < heap[smallest][0]) smallest = left;
        if (right < heap.length && heap[right][0] < heap[smallest][0]) smallest = right;
        if (smallest === k) break;
        [heap[smallest], heap[k]] = [heap[k], heap[smallest]]; k = smallest;
      }
    }
    return top;
  };
  for (let i = 0; i < n; i++) { area[i] = triangleArea(ring[previous[i]], ring[i], ring[next[i]]); push(area[i], i); }
  let remaining = n; let floor = 0;
  while (heap.length && remaining > 3) {
    const [value, index] = pop();
    if (!alive[index] || value !== area[index]) continue;
    if (Math.max(value, floor) >= minArea) break;
    floor = Math.max(value, floor);
    alive[index] = 0; remaining--;
    const before = previous[index]; const after = next[index];
    next[before] = after; previous[after] = before;
    for (const neighbour of [before, after]) {
      area[neighbour] = Math.max(triangleArea(ring[previous[neighbour]], ring[neighbour], ring[next[neighbour]]), floor);
      push(area[neighbour], neighbour);
    }
  }
  return ring.filter((_, index) => alive[index]);
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

function normalizeRing(coords, tolerance, outer, minArea = 0, byArea = false, primaryOnly = false) {
  // A ring traced from a wrapped grid runs continuously past the
  // antimeridian into the padding, so its longitudes stay as traced and
  // only the primary window is kept.
  const points = coords.map((point) => numberPoint(point, !primaryOnly));
  if (points.length < 3) return [];
  const source = points[0][0] === points.at(-1)[0] && points[0][1] === points.at(-1)[1] ? points.slice(0, -1) : points;
  const unwrapped = unwrapRing(source);
  // Loops, not Math.min/max(...spread): a native-resolution relief ring can
  // have far more points than the engine's argument-list stack allows.
  let minLat = Infinity, maxLat = -Infinity, minimum = Infinity, maximum = -Infinity;
  for (const [lon, lat] of unwrapped) { if (lat < minLat) minLat = lat; if (lat > maxLat) maxLat = lat; if (lon < minimum) minimum = lon; if (lon > maximum) maximum = lon; }
  if (maxLat - minLat >= 179.998) fail("polygon spans both poles");
  const firstWindow = Math.floor((minimum + 180) / 360);
  const lastWindow = Math.floor((maximum + 180) / 360);
  const output = [];
  for (let window = firstWindow; window <= lastWindow; window++) {
    // The padding beyond the antimeridian duplicates the other side of the
    // grid, which has its own contours.
    if (primaryOnly && window !== 0) continue;
    const west = -180 + window * 360; const east = 180 + window * 360;
    let clipped = clipRing(unwrapped, west, true);
    clipped = clipRing(clipped, east, false);
    clipped = cleanRing(clipped);
    if (clipped.length < 3) continue;
    // Relief and sea-floor bands are simplified by area at the tolerance
    // squared. The approved design used half that, which leaves no long
    // chords at all but carries about 10% more bytes than the pack budget
    // allows; at the full square a 10-degree frame around Bangkok keeps 2
    // edges over 30 px (against 69 for Douglas-Peucker). Other layers keep
    // Douglas-Peucker: a coast or a lake is not traced from a grid, so it
    // has no meander to collapse.
    const simplified = cleanRing(byArea ? visvalingam(clipped, tolerance * tolerance) : rdp(clipped, tolerance));
    if (simplified.length < 3) continue;
    const ring = simplified.map(([lon, lat]) => [lon - window * 360, lat]);
    if (ring[0][0] !== ring.at(-1)[0] || ring[0][1] !== ring.at(-1)[1]) ring.push(ring[0]);
    const area = ringArea(ring);
    if (Math.abs(area) < Math.max(1e-9, minArea)) continue;
    if ((area > 0) !== outer) ring.reverse();
    output.push(ring);
  }
  return output;
}

// minArea drops a whole ring below that footprint (deg^2) rather than just
// thinning its points: a Douglas-Peucker pass alone keeps every feature
// regardless of size, so without this a locator-scale pack ends up carrying
// thousands of sub-pixel islands, lakes and urban patches at full encoding
// cost. This mirrors the approved design's ring-weight filtering step.
function geometryParts(geometry, tolerance, polygon = false, minArea = 0, byArea = false, primaryOnly = false) {
  if (!geometry) return [];
  if (geometry.type === "Polygon") return geometry.coordinates.flatMap((ring, index) => normalizeRing(ring, tolerance, index === 0, minArea, byArea, primaryOnly));
  if (geometry.type === "MultiPolygon") return geometry.coordinates.flatMap((polygonRings) => polygonRings.flatMap((ring, index) => normalizeRing(ring, tolerance, index === 0, minArea, byArea, primaryOnly)));
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

// The signed 16-bit band field is the one per-feature attribute the pack
// carries. Contour bands store their threshold and the retained Natural
// Earth bathymetry stores its (negated) depth; rivers store Natural Earth's
// scalerank, 0 for the largest trunk rivers, so the renderer can taper
// strokes by rank instead of guessing importance from a river's extent.
function featureBand(layer, properties) {
  if (layer === "rivers") return properties?.scalerank ?? 0;
  return properties?.band ?? (properties?.depth === undefined ? 0 : -properties.depth);
}

function featuresFor(features, tolerance, minArea = 0, layer = null) {
  const output = [];
  for (const feature of features) {
    const parts = geometryParts(feature.geometry, tolerance, false, minArea, BAND_LAYERS.has(layer), feature.properties?.wrapped === true);
    if (!parts.length) continue;
    // A loop, not Math.min/max(...spread): a native-resolution relief ring
    // can carry tens of thousands of points before simplification, which
    // overflows the engine's call-stack-sized argument list.
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const part of parts) for (const [x, y] of part) { if (x < minX) minX = x; if (y < minY) minY = y; if (x > maxX) maxX = x; if (y > maxY) maxY = y; }
    const bounds = [minX, minY, maxX, maxY];
    const rawBand = featureBand(layer, feature.properties);
    const band = Number.isFinite(rawBand) ? Math.max(-32768, Math.min(32767, Math.round(rawBand))) : 0;
    output.push({ parts, bounds, band, bytes: encodeFeature(parts) });
  }
  return output;
}

// z is a flat row-major grid (row index = latitude index, ascending from
// lat[0]) sampled every `stride` native cells, each `spacing` degrees apart.
// Separable Gaussian blur, wrapping across the antimeridian (longitude is
// periodic) and clamping at the poles (latitude is not). Contouring raw
// relief/bathymetry with marching squares turns per-cell noise into a huge
// number of tiny spurious rings; a mild pre-blur is what keeps the traced
// bands close to the real terrain shape instead of quantisation static.
function blurGrid(z, nx, ny, sigma) {
  const radius = Math.max(1, Math.ceil(sigma * 3));
  const kernel = new Float64Array(2 * radius + 1);
  let sum = 0;
  for (let i = -radius; i <= radius; i++) { const v = Math.exp(-(i * i) / (2 * sigma * sigma)); kernel[i + radius] = v; sum += v; }
  for (let i = 0; i < kernel.length; i++) kernel[i] /= sum;
  const tmp = new Float32Array(nx * ny);
  for (let y = 0; y < ny; y++) {
    const rowBase = y * nx;
    for (let x = 0; x < nx; x++) {
      let acc = 0;
      for (let k = -radius; k <= radius; k++) acc += z[rowBase + (((x + k) % nx) + nx) % nx] * kernel[k + radius];
      tmp[rowBase + x] = acc;
    }
  }
  const out = new Float32Array(nx * ny);
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      let acc = 0;
      for (let k = -radius; k <= radius; k++) acc += tmp[Math.max(0, Math.min(ny - 1, y + k)) * nx + x] * kernel[k + radius];
      out[y * nx + x] = acc;
    }
  }
  return out;
}

function reliefBands(z, lon, lat, stride, spacing, smoothSigma = 0) {
  const nx = Math.floor((lon.length - 1) / stride) + 1; const ny = Math.floor((lat.length - 1) / stride) + 1;
  let values = new Float32Array(nx * ny);
  for (let y = 0; y < ny; y++) for (let x = 0; x < nx; x++) values[y * nx + x] = z[Math.min(y * stride, lat.length - 1) * lon.length + Math.min(x * stride, lon.length - 1)];
  // A global grid's first and last columns are the same meridian, so it
  // repeats every nx - 1 columns. The last column is rebuilt from the first
  // before blurring: in the 3-arcminute GMT tiles it does not hold 180E at
  // all (each eastern tile's last column repeats its own first column, 90E,
  // which put a ridge thousands of metres high down the antimeridian), and
  // the blur spread that ridge into a gap in every band either side of it.
  const global = lon[lon.length - 1] - lon[0] >= 360 - 1e-6;
  if (global) for (let y = 0; y < ny; y++) values[y * nx + nx - 1] = values[y * nx];
  if (smoothSigma > 0) values = blurGrid(values, nx, ny, smoothSigma);
  // d3-contour treats everything outside the grid as below every level, so
  // contours on a global grid close along its edges. The grid is padded
  // with wrapped columns so contours run through the antimeridian;
  // normalizeRing then cuts them exactly there and discards the padding's
  // duplicate copies.
  const pad = global ? SEAM_PAD : 0;
  const width = nx + 2 * pad;
  if (pad) {
    const padded = new Float32Array(width * ny);
    for (let y = 0; y < ny; y++) for (let x = 0; x < width; x++) padded[y * width + x] = values[y * nx + (((x - pad) % (nx - 1)) + (nx - 1)) % (nx - 1)];
    values = padded;
  }
  // d3-contour centres the value at index i on coordinate i + 0.5.
  // Contours reaching a pole close along a virtual row just past it, which
  // is clamped back onto the pole instead of leaving WGS84.
  const lastLat = lat[lat.length - 1];
  const project = (geometry) => geometry.map((polygon) => polygon.map((ring) => ring.map(([x, y]) => [lon[0] + (x - 0.5 - pad) * stride * spacing, Math.max(lat[0], Math.min(lastLat, lat[0] + (y - 0.5) * stride * spacing))])));
  const make = (thresholds, grid, level) => thresholds.flatMap((threshold) => contours().size([width, ny]).thresholds([level(threshold)])(grid).flatMap((contour) => [{ properties: { band: threshold, wrapped: global }, geometry: { type: contour.type, coordinates: project(contour.coordinates) } }]));
  // d3-contour traces the region at or above a level. A relief band wants
  // exactly that (ground at least 100 m up), but a sea-floor band means
  // "at least this deep": contouring the raw grid at -200 would instead
  // trace every cell shallower than 200 m, land included, so the bands grow
  // toward the deep ocean and the last one painted covers the whole sea.
  // Contouring the negated grid at +200 traces the water deeper than 200 m,
  // which shrinks with depth like the Natural Earth bathymetry polygons
  // this layer also carries.
  const negated = values.map((value) => -value);
  return { relief: make(RELIEF_THRESHOLDS, values, (threshold) => threshold), sea_floor: make(SEA_FLOOR_THRESHOLDS, negated, (threshold) => -threshold) };
}

function worldReliefBands({ z, lon, lat }) {
  return reliefBands(z, lon, lat, RELIEF_WORLD_STRIDE, RELIEF_WORLD_SPACING, RELIEF_WORLD_SIGMA);
}

async function readWorldReliefGrid() {
  const h5 = await import("h5wasm/node"); await h5.default.ready;
  const file = new h5.default.File(resolve(CACHE, SOURCES.relief), "r");
  const lon = file.get("lon").value; const lat = file.get("lat").value; const z = file.get("z").value;
  file.close();
  return { lon, lat, z };
}

// Decodes one lossless signed-16-bit gridline-registered GMT relief tile
// (JPEG2000) to a flat row-major Int16Array via the `opj_decompress` CLI
// (from the openjpeg project). Node has no JP2 decoder of its own; this
// mirrors the existing dependency on an external `python3` verifier.
async function decodeReliefTile(artifact, workDir) {
  const input = resolve(CACHE, artifact);
  const output = join(workDir, `${basename(artifact)}.rawl`);
  const result = spawnSync("opj_decompress", ["-i", input, "-o", output, "-quiet"], { encoding: "utf8" });
  if (result.error) fail(`opj_decompress is required to decode ${artifact} (openjpeg not found): ${result.error.message}`);
  if (result.status !== 0) fail(`opj_decompress failed on ${artifact}: ${result.stderr || result.stdout}`);
  const raw = await readFile(output);
  const expected = RELIEF_FINE_TILE_POINTS * RELIEF_FINE_TILE_POINTS * 2;
  if (raw.length !== expected) fail(`${artifact}: decoded ${raw.length} bytes, expected ${expected}`);
  const values = new Int16Array(RELIEF_FINE_TILE_POINTS * RELIEF_FINE_TILE_POINTS);
  for (let i = 0; i < values.length; i++) values[i] = raw.readInt16LE(i * 2);
  return values;
}

// Assembles the eight 90x90-degree tiles into one native-resolution global
// grid (0.05deg spacing), row 0 = south (lat -90), matching the 06m grid's
// ascending-latitude convention so reliefBands needs no orientation case.
async function readFineReliefGrid() {
  const workDir = await mkdtemp(join(tmpdir(), "moss-place-map-relief-"));
  try {
    const nx = Math.round(360 / RELIEF_FINE_SPACING) + 1;
    const ny = Math.round(180 / RELIEF_FINE_SPACING) + 1;
    const z = new Float32Array(nx * ny);
    const T = RELIEF_FINE_TILE_POINTS;
    for (const tile of RELIEF_FINE_TILES) {
      const values = await decodeReliefTile(tile.artifact, workDir);
      const latTop = tile.latMin + 90; // tile-local row 0 is the tile's north edge
      for (let r = 0; r < T; r++) {
        const lat = latTop - r * RELIEF_FINE_SPACING;
        const gy = Math.round((lat + 90) / RELIEF_FINE_SPACING);
        const rowBase = r * T; const gRowBase = gy * nx;
        for (let c = 0; c < T; c++) {
          const gx = Math.round((tile.lonMin + c * RELIEF_FINE_SPACING + 180) / RELIEF_FINE_SPACING);
          z[gRowBase + gx] = values[rowBase + c];
        }
      }
    }
    const lon = new Float64Array(nx); for (let x = 0; x < nx; x++) lon[x] = -180 + x * RELIEF_FINE_SPACING;
    const lat = new Float64Array(ny); for (let y = 0; y < ny; y++) lat[y] = -90 + y * RELIEF_FINE_SPACING;
    return { lon, lat, z };
  } finally {
    await rm(workDir, { recursive: true, force: true });
  }
}

async function makeLayers() {
  const raw = {};
  for (const [name, source] of Object.entries(SOURCES)) {
    if (name === "relief") continue;
    if (name === "ice") raw[name] = [...await readGeoJson(source[0]), ...await readGeoJson(source[1])];
    else raw[name] = await readGeoJson(source);
  }
  const worldGrid = await readWorldReliefGrid();
  const worldBands = worldReliefBands(worldGrid);
  const fineGrid = await readFineReliefGrid();
  const fineBands = reliefBands(fineGrid.z, fineGrid.lon, fineGrid.lat, 1, RELIEF_FINE_SPACING, RELIEF_FINE_SIGMA);

  const sourceLayers = { land: "land", lakes: "lakes", rivers: "rivers", ice: "ice", reefs: "reefs", salt_flats: "salt_flats", built_up: "built_up" };
  const bathymetry = raw.bathymetry.filter((feature) => feature.properties?.depth === 6000);
  const buildLayerMap = (bands) => {
    const layers = new Map();
    for (const [name, sourceName] of Object.entries(sourceLayers)) layers.set(name, raw[sourceName]);
    layers.set("relief", bands.relief);
    layers.set("sea_floor", [...bands.sea_floor, ...bathymetry]);
    return layers;
  };
  return { worldLayers: buildLayerMap(worldBands), fineLayers: buildLayerMap(fineBands) };
}

const LINE_LAYERS = new Set(["rivers"]);
const BAND_LAYERS = new Set(["relief", "sea_floor"]);

function tierPayload(layers, tolerance) {
  const pieces = []; const refs = []; const layerBytes = {}; let featureRef = 0;
  for (const [name, features] of layers) {
    const tight = name === "built_up"; // small real urban patches must not be eaten by the same threshold as ocean-scale bands.
    // Land carries coast's own ~1px tolerance, not the ~2px "wide" bands
    // get: its rings are now the only source of the coast stroke (schema 2),
    // so they need to stay as crisp as the old standalone coastline layer.
    const worldBand = tolerance === "world" && BAND_LAYERS.has(name);
    const featureTolerance = worldBand ? WORLD_BAND_TOLERANCE : tight ? BUILT_UP_TOLERANCE[tolerance] : name === "land" ? COAST_TOLERANCE[tolerance] : TOLERANCE[tolerance];
    // A ring below this footprint is dropped outright, not just thinned: a
    // point-simplification pass alone keeps every feature no matter how
    // small. The "tight" built-up layer drops only sub-pixel rings; "wide"
    // bands (land included) drop anything under a 2x2-pixel footprint.
    // World-tier tolerances are calibrated to a whole-globe frame, where
    // the same 2x2-pixel footprint would drop real islands and lakes, so
    // the world tier drops only relief and sea-floor rings, and only under
    // twice their tolerance squared (about 4 px^2 on the world map): at
    // that scale a band ring that small is contour noise, drawn as a sliver.
    const worldBandFloor = worldBand ? 2 * featureTolerance * featureTolerance : 0;
    const minArea = LINE_LAYERS.has(name) ? 0 : tolerance !== "fine" ? worldBandFloor : tight ? featureTolerance * featureTolerance : (2 * featureTolerance) * (2 * featureTolerance);
    const kept = tolerance === "world" && name === "rivers" ? features.filter((feature) => (feature.properties?.scalerank ?? 0) <= WORLD_MAX_RIVER_RANK) : features;
    const normalized = featuresFor(kept, featureTolerance, minArea, name);
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

const BUDGET_BYTES = 3_565_158;

// Compresses the pack and writes it with its size report, but only once it
// fits: the budget is checked before anything touches disk, so a run that
// blows the budget fails without replacing the checked-in pack.
async function writePack(bytes, fields, { out = OUT, reportPath = REPORT, budgetBytes = BUDGET_BYTES } = {}) {
  // The budget measures what the app actually ships: this container,
  // brotli -q11, not the raw MOSSPLM1 bytes.
  const compressed = brotliCompressSync(bytes, {
    params: {
      [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
      [zlibConstants.BROTLI_PARAM_SIZE_HINT]: bytes.length,
    },
  });
  const marginBytes = budgetBytes - compressed.length;
  const marginPercent = (marginBytes / budgetBytes) * 100;
  const report = { schema: fields.schema, manifest_sha256: fields.manifest_sha256, raw_bytes: bytes.length, brotli_bytes: compressed.length, budget_bytes: budgetBytes, margin_bytes: marginBytes, margin_percent: Number(marginPercent.toFixed(3)), ...fields };
  if (compressed.length > budgetBytes) fail(`place-map pack is ${compressed.length} brotli bytes, over ${budgetBytes}; nothing written\n${JSON.stringify(report, null, 2)}`);
  if (marginPercent < 5) fail(`place-map pack has only ${marginPercent.toFixed(3)}% brotli budget margin; need at least 5%; nothing written\n${JSON.stringify(report, null, 2)}`);
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, compressed);
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  return report;
}

async function main() {
  const { manifestBytes, manifestDigest } = await verifySources();
  const { worldLayers, fineLayers } = await makeLayers();
  const world = tierPayload(worldLayers, "world"); const fine = tierPayload(fineLayers, "fine");
  const index = tileIndex(fine.refs);
  const sourceMasks = SOURCE_MASKS.map(u16);
  const header = Buffer.concat([MAGIC, u16(SCHEMA), u16(0), u32(QUANT), i32(-180 * QUANT), i32(180 * QUANT), i32(-90 * QUANT), i32(90 * QUANT), u16(LAYERS.length), u16(2), u16(TILE_DEGREES), u16(0), manifestDigest, ...sourceMasks]);
  const tiers = Buffer.concat([Buffer.from([0, 0]), u32(world.refs.length), u32(world.bytes.length), world.bytes, Buffer.from([1, 0]), u32(fine.refs.length), u32(fine.bytes.length), fine.bytes, index]);
  const bytes = Buffer.concat([header, tiers]);
  bytes.writeUInt16LE(header.length, 10);
  const report = await writePack(bytes, { schema: SCHEMA, manifest_sha256: manifestDigest.toString("hex"), tiers: { world: world.bytes.length, fine: fine.bytes.length }, index: index.length, layers: { world_counts: Object.fromEntries([...worldLayers].map(([name, features]) => [name, features.length])), fine_counts: Object.fromEntries([...fineLayers].map(([name, features]) => [name, features.length])), world_bytes: world.layerBytes, fine_bytes: fine.layerBytes } });
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
  COAST_TOLERANCE,
  encodeFeature,
  QUANT,
  featuresFor,
  FINE_PX,
  geometryParts,
  reliefBands,
  rdp,
  RELIEF_THRESHOLDS,
  SCHEMA,
  SEA_FLOOR_THRESHOLDS,
  splitDateline,
  tierPayload,
  tileIndex,
  TOLERANCE,
  worldReliefBands,
  writePack,
};
