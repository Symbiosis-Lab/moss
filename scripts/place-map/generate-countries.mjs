#!/usr/bin/env node
// Country shapes for the country map: Natural Earth 1:50m admin-0 countries,
// disputed and breakaway land cut out of every country, Antarctica dropped,
// simplified for a map about 1,300 px wide in Equal Earth. Writes the
// brotli-compressed MOSSCTY1 asset and its size report, with a budget of its
// own so the landscape pack's budget is untouched. With `--admin1 <ISO3>` it
// writes one country's first-level subdivisions instead (Natural Earth 1:10m
// admin-1), simplified for that country drawn 1,300 px wide in its own conic
// equal-area projection, in the same format.
//
//   MOSS_PLACE_MAP_SOURCE_DIR=<cache> python3 generate.py --check --countries
//   MOSS_PLACE_MAP_SOURCE_DIR=<cache> node generate-countries.mjs
//   MOSS_PLACE_MAP_SOURCE_DIR=<cache> node generate-countries.mjs --admin1 DEU
//
// Boolean operations use polygon-clipping (pinned in package.json).

import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { brotliCompressSync, constants as zlib } from "node:zlib";
import { dirname, resolve } from "node:path";
import shp from "shpjs";
import polygonClipping from "polygon-clipping";

const ROOT = resolve(import.meta.dirname, "../..");
const DATA = resolve(ROOT, "crates/moss-build/data/place-map");
const MANIFEST = resolve(DATA, "countries-source-manifest.toml");
const OUT = resolve(DATA, "countries-v1.bin.br");
const REPORT = resolve(DATA, "countries-size.json");
const CACHE = resolve(process.env.MOSS_PLACE_MAP_SOURCE_DIR ?? "/private/tmp/moss-place-map-sources");

// The search index's cities: every populated place of scalerank 4 or larger
// (0 is the largest) and any national capital down to scalerank 6, about a
// thousand names.
const CITY_MAX_RANK = 4;
const CAPITAL_MAX_RANK = 6;

const MAGIC = Buffer.from("MOSSCTY1", "ascii");
const SCHEMA = 1;
// Hundredths of a degree: 0.04 px on the 1,300 px map.
const QUANT = 100;
// The frame: 84N to 57S, the whole longitude range.
const NORTH = 84;
const SOUTH = -57;
const MAP_WIDTH_PX = 1300;
// Douglas-Peucker tolerance, and the smallest ring kept unless it is a
// country's largest, both in pixels of the 1,300 px map.
const TOLERANCE_PX = 0.6;
const MIN_RING_AREA_PX2 = 0.5;
// A country that keeps less than this share of its area once disputed land is
// cut out is a disputed name with no shape of its own.
const MIN_KEPT_SHARE = 0.05;
// The first measured run (32,117 brotli bytes of countries-v1.bin.br) plus 5%; it only moves down.
const BUDGET_BYTES = 33723;
// The search index moss-build writes from the city list and the shapes' names: the first measured size (27,174 bytes) plus 5%. It only moves down.
const INDEX_BUDGET_BYTES = 28533;

// Equal Earth (Savric, Patterson and Jenny 2018).
const A1 = 1.340264, A2 = -0.081106, A3 = 0.000893, A4 = 0.003796, M = Math.sqrt(3) / 2;
function equalEarth(lonDeg, latDeg) {
  const lon = lonDeg * Math.PI / 180;
  const theta = Math.asin(M * Math.sin(latDeg * Math.PI / 180));
  const t2 = theta * theta, t6 = t2 * t2 * t2;
  const x = (2 * Math.sqrt(3) * lon * Math.cos(theta)) / (3 * (9 * A4 * t6 * t2 + 7 * A3 * t6 + 3 * A2 * t2 + A1));
  const y = theta * (A1 + A2 * t2 + t6 * (A3 + A4 * t2));
  return [x, y];
}
const HALF_WIDTH = equalEarth(180, 0)[0];
const PX_PER_UNIT = MAP_WIDTH_PX / (2 * HALF_WIDTH);
const toPx = ([lon, lat]) => { const [x, y] = equalEarth(lon, lat); return [x * PX_PER_UNIT, -y * PX_PER_UNIT]; };

const sha256 = (bytes) => createHash("sha256").update(bytes).digest();
const fail = (message) => { throw new Error(message); };

function parseManifest(text) {
  const sources = [];
  let current = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/#.*/, "").trim();
    if (line === "[[sources]]") { current = {}; sources.push(current); continue; }
    const match = line.match(/^([A-Za-z0-9_]+)\s*=\s*"(.*)"$/);
    if (match && current) current[match[1]] = match[2];
  }
  return sources;
}

async function readFeatures(artifact) {
  const bytes = await readFile(resolve(CACHE, artifact));
  const value = await shp(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  return Array.isArray(value) ? value.flatMap((layer) => layer.features ?? []) : value.features ?? [];
}

const quantRing = (ring, quant = QUANT) => {
  const q = (v) => Math.round(v * quant) / quant;
  const out = [];
  for (const [lon, lat] of ring) {
    const p = [q(lon), q(lat)];
    const last = out.at(-1);
    if (!last || last[0] !== p[0] || last[1] !== p[1]) out.push(p);
  }
  if (out.length > 1 && out[0][0] === out.at(-1)[0] && out[0][1] === out.at(-1)[1]) out.pop();
  return out;
};
// polygon-clipping geometry: array of polygons, each an array of closed rings.
const multiPolygon = (geometry) => geometry.type === "Polygon" ? [geometry.coordinates] : geometry.coordinates;
const signedArea = (ring) => { let a = 0; for (let i = 0; i < ring.length; i++) { const [x1, y1] = ring[i], [x2, y2] = ring[(i + 1) % ring.length]; a += x1 * y2 - x2 * y1; } return a / 2; };
const geometryArea = (mp) => mp.reduce((sum, poly) => sum + poly.reduce((s, ring, i) => s + (i === 0 ? 1 : -1) * Math.abs(signedArea(ring.slice(0, -1))), 0), 0);
const bbox = (mp) => { let b = [Infinity, Infinity, -Infinity, -Infinity]; for (const poly of mp) for (const [x, y] of poly[0]) b = [Math.min(b[0], x), Math.min(b[1], y), Math.max(b[2], x), Math.max(b[3], y)]; return b; };
const overlaps = (a, b) => a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3];

// ---- topology-preserving simplification ----------------------------------

const keyOf = (p) => `${p[0]},${p[1]}`;
function perpendicular(p, a, b) {
  const dx = b[0] - a[0], dy = b[1] - a[1], len2 = dx * dx + dy * dy;
  if (len2 === 0) return Math.hypot(p[0] - a[0], p[1] - a[1]);
  const t = Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}
function douglasPeucker(points, tolerance, project = toPx) {
  const px = points.map(project);
  const keep = new Uint8Array(points.length);
  keep[0] = keep[points.length - 1] = 1;
  const stack = [[0, points.length - 1]];
  while (stack.length) {
    const [lo, hi] = stack.pop();
    let max = tolerance, at = -1;
    for (let i = lo + 1; i < hi; i++) { const d = perpendicular(px[i], px[lo], px[hi]); if (d > max) { max = d; at = i; } }
    if (at >= 0) { keep[at] = 1; stack.push([lo, at], [at, hi]); }
  }
  return points.filter((_, i) => keep[i]);
}
const compare = (a, b) => a[0] - b[0] || a[1] - b[1];

/**
 * Simplify every ring so that an edge two rings share is simplified once and
 * stays identical in both: rings are cut into arcs at the vertices where
 * "shared with another ring" starts or stops, and each shared arc is
 * simplified in a canonical direction and cached.
 */
function simplifyRings(ringsByOwner, tolerance, project = toPx) {
  const owners = new Map();
  for (const [owner, rings] of ringsByOwner) for (const ring of rings) for (const p of ring) { const k = keyOf(p); (owners.get(k) ?? owners.set(k, new Set()).get(k)).add(owner); }
  const shared = (p) => owners.get(keyOf(p)).size > 1;
  const cache = new Map();
  const simplifyArc = (arc) => {
    if (arc.length <= 2) return arc;
    const flip = arc.length > 2 && (compare(arc[0], arc.at(-1)) || compare(arc[1], arc.at(-2))) > 0;
    const canonical = flip ? [...arc].reverse() : arc;
    const k = canonical.map(keyOf).join(";");
    let result = cache.get(k);
    if (!result) { result = douglasPeucker(canonical, tolerance, project); cache.set(k, result); }
    return flip ? [...result].reverse() : result;
  };
  const simplifyRing = (ring) => {
    const n = ring.length;
    const flags = ring.map(shared);
    let start = -1;
    for (let i = 0; i < n; i++) if (flags[i] !== flags[(i + n - 1) % n]) { start = i; break; }
    if (start < 0) {
      if (!flags[0]) return douglasPeucker([...ring, ring[0]], tolerance, project).slice(0, -1);
      let low = 0; for (let i = 1; i < n; i++) if (compare(ring[i], ring[low]) < 0) low = i;
      const rotated = [...ring.slice(low), ...ring.slice(0, low)];
      return simplifyArc([...rotated, rotated[0]]).slice(0, -1);
    }
    const rotated = [...ring.slice(start), ...ring.slice(0, start)];
    const rf = [...flags.slice(start), ...flags.slice(0, start)];
    const out = [];
    let from = 0;
    for (let i = 1; i <= n; i++) {
      if (i === n || rf[i] !== rf[i - 1]) {
        const arc = rotated.slice(from, Math.min(i + 1, n)).concat(i === n ? [rotated[0]] : []);
        const simplified = rf[from] ? simplifyArc(arc) : douglasPeucker(arc, tolerance, project);
        out.push(...simplified.slice(0, -1));
        from = i;
      }
    }
    return out;
  };
  const result = new Map();
  for (const [owner, rings] of ringsByOwner) {
    const kept = rings.map((ring) => { const reduced = simplifyRing(ring); const simple = reduced.length >= 3 ? reduced : ring; return { ring: simple, area: Math.abs(signedArea(simple.map(project))) }; });
    const largest = Math.max(...kept.map((r) => r.area), 0);
    result.set(owner, kept.filter((r) => r.ring.length >= 3 && (r.area === largest || r.area >= MIN_RING_AREA_PX2)).map((r) => r.ring));
  }
  return result;
}

// ---- encoding -------------------------------------------------------------

function varint(value) { const out = []; let x = value; while (x >= 128) { out.push((x & 127) | 128); x = Math.floor(x / 128); } out.push(x); return Buffer.from(out); }
const zigzag = (v) => (v < 0 ? -v * 2 - 1 : v * 2);
const text = (s) => { const b = Buffer.from(s ?? "", "utf8"); return Buffer.concat([varint(b.length), b]); };
function encodeRings(rings, quant = QUANT) {
  const chunks = [varint(rings.length)];
  let px = 0, py = 0;
  for (const ring of rings) {
    chunks.push(varint(ring.length));
    for (const [lon, lat] of ring) {
      const x = Math.round(lon * quant), y = Math.round(lat * quant);
      chunks.push(varint(zigzag(x - px)), varint(zigzag(y - py)));
      px = x; py = y;
    }
  }
  return Buffer.concat(chunks);
}
function encodeAsset(countries, disputedRings, manifestDigest, quant = QUANT) {
  const chunks = [MAGIC, Buffer.from([SCHEMA & 255, SCHEMA >> 8, quant & 255, quant >> 8]), manifestDigest, varint(countries.length)];
  for (const c of countries) {
    chunks.push(text(c.code), text(c.adm0), text(c.name), text(c.nameLong), text(c.admin), text(c.formal), Buffer.from([c.disputedName ? 1 : 0]), encodeRings(c.rings, quant));
  }
  chunks.push(encodeRings(disputedRings, quant));
  return Buffer.concat(chunks);
}

function compress(bytes) {
  return brotliCompressSync(bytes, { params: { [zlib.BROTLI_PARAM_QUALITY]: 11, [zlib.BROTLI_PARAM_SIZE_HINT]: bytes.length, [zlib.BROTLI_PARAM_LGWIN]: 24 } });
}

// ---- cities ---------------------------------------------------------------

// [name, admin code] pairs, largest first. A place belongs to the country whose
// shape carries its admin code, else to its sovereign state's; one on land
// that has no shape of its own (a disputed name) is dropped.
function cutCities(features, shaped) {
  const seen = new Set();
  const cities = [];
  const kept = features.map((f) => f.properties).filter((p) => p.scalerank <= CITY_MAX_RANK || (p.adm0cap && p.scalerank <= CAPITAL_MAX_RANK));
  for (const p of kept.sort((a, b) => a.scalerank - b.scalerank || b.pop_max - a.pop_max || a.name.localeCompare(b.name))) {
    const code = [p.adm0_a3, p.sov_a3].find((c) => shaped.has(c));
    const key = `${p.name}|${code}`;
    if (!code || seen.has(key)) continue;
    seen.add(key);
    cities.push([p.name, code]);
  }
  return cities;
}

// ---- build ----------------------------------------------------------------

// Disputed and breakaway land is cut out of every shape and kept apart, to be
// drawn as plain land; a shape that keeps under MIN_KEPT_SHARE of its area is a
// disputed name with no rings.
function disputedCutter(disputedFeatures, quant = QUANT) {
  const disputed = polygonClipping.union(...disputedFeatures.map((f) => multiPolygon(f.geometry)));
  const disputedBox = bbox(disputed);
  const pieces = [];
  const cut = (raw) => {
    const geometry = multiPolygon(raw);
    const touches = overlaps(bbox(geometry), disputedBox);
    const kept = touches ? polygonClipping.difference(geometry, disputed) : geometry;
    if (touches) { const piece = polygonClipping.intersection(geometry, disputed); if (piece.length) pieces.push(...piece); }
    const share = geometryArea(kept) / geometryArea(geometry);
    return {
      disputedName: share < MIN_KEPT_SHARE,
      rings: share < MIN_KEPT_SHARE ? [] : kept.flatMap((poly) => poly.map((ring) => quantRing(ring.slice(0, -1), quant))).filter((r) => r.length >= 3),
    };
  };
  return { cut, pieces };
}

async function build() {
  const manifestBytes = await readFile(MANIFEST);
  const sources = parseManifest(manifestBytes.toString("utf8"));
  const artifact = (name) => sources.find((s) => s.name === name)?.artifact ?? fail(`manifest has no ${name}`);
  const countryFeatures = await readFeatures(artifact("countries"));
  const disputedFeatures = await readFeatures(artifact("breakaway_disputed_areas"));

  const cutter = disputedCutter(disputedFeatures);
  const raw = [];
  for (const feature of countryFeatures) {
    const p = feature.properties;
    if (p.ADM0_A3 === "ATA") continue;
    raw.push({
      code: p.ISO_A3_EH && p.ISO_A3_EH !== "-99" ? p.ISO_A3_EH : p.ADM0_A3,
      adm0: p.ADM0_A3, name: p.NAME, nameLong: p.NAME_LONG, admin: p.ADMIN, formal: p.FORMAL_EN ?? "",
      ...cutter.cut(feature.geometry),
    });
  }
  const disputedPieces = cutter.pieces;
  const ringsByOwner = new Map(raw.map((c) => [c.code, c.rings]));
  ringsByOwner.set("\u0000disputed", disputedPieces.flatMap((poly) => poly.map((ring) => quantRing(ring.slice(0, -1)))).filter((r) => r.length >= 3));
  const simplified = simplifyRings(ringsByOwner, TOLERANCE_PX);
  const countries = raw.map((c) => ({ ...c, rings: simplified.get(c.code) }));
  for (const c of countries) for (const ring of c.rings) for (const [, lat] of ring) if (lat > NORTH + 1e-6 || lat < SOUTH - 1e-6) fail(`${c.name} leaves the frame`);
  const bytes = encodeAsset(countries, simplified.get("\u0000disputed"), sha256(manifestBytes));
  const places = await readFeatures(artifact("populated_places"));
  const cities = cutCities(places, new Set(countries.filter((c) => c.rings.length).map((c) => c.adm0)));
  return { bytes, countries, cities, disputedRings: simplified.get("\u0000disputed"), manifestDigest: sha256(manifestBytes) };
}

// ---- regions --------------------------------------------------------------

const ADMIN1_MANIFEST = resolve(DATA, "admin1-source-manifest.toml");
const ADMIN1_REPORT = resolve(DATA, "admin1-size.json");
const admin1Out = (iso3) => resolve(DATA, `admin1-${iso3}-v1.bin.br`);
// A thousandth of a degree: a country drawn 1,300 px wide is far larger than
// on the world map, so hundredths would show as steps.
const ADMIN1_QUANT = 1000;
// Each pack's first measured size plus 5% (DEU: 9,852 bytes); they only move down.
const ADMIN1_BUDGET_BYTES = { DEU: 10344 };

// Conic equal-area (Albers) fitted to a box [west, south, east, north] in
// degrees, standard parallels a sixth of the latitude range in from each edge.
// The same function as `conic_equal_area` in moss-build's projection.rs, which
// draws the packs this simplifies; both tests pin the same fixture.
function conicFor([west, south, east, north]) {
  const rad = Math.PI / 180, span = north - south;
  const p1 = (south + span / 6) * rad, p2 = (north - span / 6) * rad, p0 = ((south + north) / 2) * rad, l0 = (west + east) / 2;
  const n = (Math.sin(p1) + Math.sin(p2)) / 2;
  const c = Math.cos(p1) ** 2 + 2 * n * Math.sin(p1);
  const rho = (p) => Math.sqrt(c - 2 * n * Math.sin(p)) / n;
  const rho0 = rho(p0);
  return (lon, lat) => { const r = rho(lat * rad), t = n * (lon - l0) * rad; return [r * Math.sin(t), rho0 - r * Math.cos(t)]; };
}

// The country's first-level subdivisions: ISO 3166-2 code, the code without
// its country prefix, the local name (also what the map shows), the English
// name and the GeoNames name, simplified to TOLERANCE_PX of the country drawn
// MAP_WIDTH_PX wide in its own conic projection.
async function buildRegions(iso3) {
  const manifestBytes = await readFile(ADMIN1_MANIFEST);
  const sources = parseManifest(manifestBytes.toString("utf8"));
  const artifact = (name) => sources.find((s) => s.name === name)?.artifact ?? fail(`manifest has no ${name}`);
  const features = (await readFeatures(artifact("admin_1_states_provinces"))).filter((f) => f.properties.adm0_a3 === iso3);
  if (!features.length) fail(`Natural Earth has no admin-1 regions for ${iso3}`);
  const cutter = disputedCutter(await readFeatures(artifact("breakaway_disputed_areas")), ADMIN1_QUANT);
  const raw = features.map((f) => {
    const p = f.properties;
    return { code: p.iso_3166_2, adm0: p.iso_3166_2.split("-")[1] ?? "", name: p.name, nameLong: p.name, admin: p.name_en ?? "", formal: p.gn_name ?? "", ...cutter.cut(f.geometry) };
  });
  const box = [Infinity, Infinity, -Infinity, -Infinity];
  for (const r of raw) for (const ring of r.rings) for (const [x, y] of ring) [box[0], box[1], box[2], box[3]] = [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)];
  const conic = conicFor(box);
  const corners = raw.flatMap((r) => r.rings.flat()).map(([lon, lat]) => conic(lon, lat));
  const width = Math.max(...corners.map((p) => p[0])) - Math.min(...corners.map((p) => p[0]));
  const toRegionPx = ([lon, lat]) => { const [x, y] = conic(lon, lat); return [(x * MAP_WIDTH_PX) / width, (-y * MAP_WIDTH_PX) / width]; };
  const ringsByOwner = new Map(raw.map((r) => [r.code, r.rings]));
  ringsByOwner.set("\u0000disputed", cutter.pieces.flatMap((poly) => poly.map((ring) => quantRing(ring.slice(0, -1), ADMIN1_QUANT))).filter((r) => r.length >= 3));
  const simplified = simplifyRings(ringsByOwner, TOLERANCE_PX, toRegionPx);
  const regions = raw.map((r) => ({ ...r, rings: simplified.get(r.code) }));
  const disputedRings = simplified.get("\u0000disputed");
  const manifestDigest = sha256(manifestBytes);
  return { bytes: encodeAsset(regions, disputedRings, manifestDigest, ADMIN1_QUANT), regions, disputedRings, manifestDigest };
}

async function writeRegions(iso3, { bytes, regions, disputedRings, manifestDigest }) {
  const compressed = compress(bytes);
  const budget = ADMIN1_BUDGET_BYTES[iso3];
  if (budget === undefined) fail(`${iso3} has no budget in ADMIN1_BUDGET_BYTES; measure it and add one`);
  if (compressed.length > budget) fail(`${iso3} regions are ${compressed.length} brotli bytes, over the ${budget} budget; nothing written`);
  const reports = JSON.parse(await readFile(ADMIN1_REPORT, "utf8").catch(() => "{}"));
  reports[iso3] = {
    schema: SCHEMA,
    manifest_sha256: manifestDigest.toString("hex"),
    raw_bytes: bytes.length,
    brotli_bytes: compressed.length,
    budget_bytes: budget,
    regions: regions.length,
    vertices: regions.reduce((n, r) => n + r.rings.reduce((m, ring) => m + ring.length, 0), 0) + disputedRings.reduce((m, r) => m + r.length, 0),
  };
  await writeFile(admin1Out(iso3), compressed);
  await writeFile(ADMIN1_REPORT, `${JSON.stringify(reports, null, 2)}\n`);
  return reports[iso3];
}

async function writeAsset({ bytes, countries, cities, disputedRings, manifestDigest }, { out = OUT, reportPath = REPORT, budget = BUDGET_BYTES } = {}) {
  const compressed = compress(bytes);
  const report = {
    schema: SCHEMA,
    manifest_sha256: manifestDigest.toString("hex"),
    raw_bytes: bytes.length,
    brotli_bytes: compressed.length,
    budget_bytes: budget,
    index_budget_bytes: INDEX_BUDGET_BYTES,
    countries: countries.length,
    disputed_names: countries.filter((c) => c.disputedName).map((c) => c.name),
    cities: cities.length,
    vertices: countries.reduce((n, c) => n + c.rings.reduce((m, r) => m + r.length, 0), 0) + disputedRings.reduce((m, r) => m + r.length, 0),
  };
  if (budget && compressed.length > budget) fail(`country shapes are ${compressed.length} brotli bytes, over the ${budget} budget; nothing written`);
  await writeFile(out, compressed);
  await writeFile(resolve(dirname(out), "countries-cities-v1.json"), `${JSON.stringify({ schema: SCHEMA, manifest_sha256: manifestDigest.toString("hex"), cities })}\n`);
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  return report;
}

function verifyCanonicalSources(set) {
  const result = spawnSync("python3", ["-I", resolve(import.meta.dirname, "generate.py"), "--check", set, "--source-dir", CACHE], { stdio: "inherit" });
  if (result.status !== 0) fail("country-shape source verification failed");
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    const at = process.argv.indexOf("--admin1");
    if (at >= 0) {
      const iso3 = process.argv[at + 1] ?? fail("--admin1 needs a country's ISO 3166 alpha-3 code");
      verifyCanonicalSources("--admin1");
      console.log(JSON.stringify(await writeRegions(iso3, await buildRegions(iso3)), null, 2));
    } else {
      verifyCanonicalSources("--countries");
      console.log(JSON.stringify(await writeAsset(await build()), null, 2));
    }
  } catch (error) {
    console.error(error.stack ?? error.message);
    process.exitCode = 1;
  }
}

export { ADMIN1_BUDGET_BYTES, ADMIN1_QUANT, BUDGET_BYTES, buildRegions, conicFor, INDEX_BUDGET_BYTES, build, compress, cutCities, encodeAsset, encodeRings, equalEarth, simplifyRings, toPx, writeAsset, QUANT, NORTH, SOUTH, HALF_WIDTH };
