import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { brotliDecompressSync } from "node:zlib";
import test from "node:test";
import {
  buildCityLabels,
  buildPeakLabels,
  buildRangeLabels,
  buildRiverLabels,
  CITY_SCALERANK_MAX,
  cleanCityNameZht,
  COAST_TOLERANCE,
  compareLabels,
  encodeFeature,
  encodeLabels,
  QUANT,
  featuresFor,
  FINE_PX,
  geometryParts,
  PEAK_SCALERANK_MAX,
  RELIEF_THRESHOLDS,
  reliefBands,
  rdp,
  riverSimplification,
  RIVER_SCALERANK_MAX,
  SCHEMA,
  SEA_FLOOR_THRESHOLDS,
  splitDateline,
  tierPayload,
  TOLERANCE,
  writePack,
} from "./generate.mjs";

test("relief and sea-floor ladders cover the approved visual ranges", () => {
  assert.deepEqual(RELIEF_THRESHOLDS, [
    100, 200, 400, 700, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000,
  ]);
  assert.deepEqual(SEA_FLOOR_THRESHOLDS, [-6000, -5000, -4000, -3000, -2000, -1000, -200, -100, -50, -30, -20, -10]);
  assert.equal(SEA_FLOOR_THRESHOLDS.at(0), -6000);
  assert.equal(SEA_FLOOR_THRESHOLDS.at(-1), -10);
  // Shelf-sea steps: without them a shallow shelf sea (the Gulf of Thailand,
  // the Persian Gulf) has no band shallower than -50m and renders as one
  // flat deep-water tone instead of shoaling toward the coast.
  for (const shelf of [-10, -20, -30]) assert.ok(SEA_FLOOR_THRESHOLDS.includes(shelf), `missing shelf-sea band ${shelf}`);
});

test("locator-tier tolerances hold the approved ~1px coast / ~2px band resolution, not the old ~22px pass", () => {
  // Locator frames render ~10 degrees across ~720px, so 1px is 10/720deg.
  const px = 10 / 720;
  assert.equal(FINE_PX, px);
  assert.ok(COAST_TOLERANCE.fine <= px + 1e-9, `coast tolerance ${COAST_TOLERANCE.fine} exceeds 1px (${px})`);
  assert.ok(TOLERANCE.fine <= 2 * px + 1e-9, `band tolerance ${TOLERANCE.fine} exceeds 2px (${2 * px})`);
  // The bug this guards: a 0.3deg pass is about 21.6px at this scale, coarse
  // enough to draw a small island as a handful of straight segments.
  assert.ok(COAST_TOLERANCE.fine < 0.3 / 10, "coast tolerance regressed toward the old 0.3deg pass");
});

test("a jagged coastline keeps materially more vertices at the approved tolerance than the old 0.3deg pass", () => {
  // Stand-in for a real small, convoluted island coastline (e.g. Cyprus):
  // a ring with many capes and bays at a scale (~0.4deg across) typical of
  // a Mediterranean island, well above the ~2px sub-pixel-drop floor.
  const points = [];
  const n = 400;
  for (let i = 0; i < n; i++) {
    const angle = (2 * Math.PI * i) / n;
    const radius = 0.2 + 0.05 * Math.sin(angle * 17) + 0.02 * Math.sin(angle * 41 + 1);
    points.push([Math.cos(angle) * radius, Math.sin(angle) * radius]);
  }
  points.push(points[0]);
  const oldTolerance = 0.3; // the bug: applied uniformly to coast and bands alike
  const before = rdp(points.slice(0, -1), oldTolerance);
  const after = rdp(points.slice(0, -1), COAST_TOLERANCE.fine);
  assert.ok(after.length > before.length * 3, `expected the approved tolerance (${after.length} pts) to keep at least 3x the old pass's vertices (${before.length} pts)`);
  assert.ok(before.length < 15, "the old 0.3deg pass should reduce this coastline to only a handful of points");
});

test("quantised feature encoding is deterministic", () => {
  const first = encodeFeature([[[0, 0], [1, 1], [2, 1]]]);
  const second = encodeFeature([[[0, 0], [1, 1], [2, 1]]]);
  assert.deepEqual(first, second);
  assert.equal(first.toString("hex"), "0100030000000000d00fd00fd00f00");
});

test("the coordinate quantum is invisible on a locator and a 2 px step costs a byte per axis", () => {
  // 1 px of a 10-degree, 720 px locator is FINE_PX degrees.
  assert.ok(1 / QUANT < FINE_PX / 10, `a quantum of ${1 / QUANT} degrees is a tenth of a pixel or more`);
  const step = 2 * FINE_PX;
  const bytes = encodeFeature([[[0, 0], [step, step]]]);
  // part count u16 + point count u32 + the first point (1 byte per axis) + the 2 px step.
  assert.equal(bytes.length, 2 + 4 + 2 + 2);
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

test("checked-in pack remains within its brotli budget and decodes to the versioned header", async () => {
  // The embedded artifact is brotli-compressed on disk (moss-build's
  // `place_map::embedded()` decompresses it once at runtime): the budget
  // this gates is the compressed size, not the raw MOSSPLM1 bytes
  // underneath.
  const compressed = await readFile(new URL("../../crates/moss-build/data/place-map/place-map-v1.bin.br", import.meta.url));
  const report = JSON.parse(await readFile(new URL("../../crates/moss-build/data/place-map/place-map-size.json", import.meta.url)));
  const bytes = brotliDecompressSync(compressed);
  assert.equal(bytes.subarray(0, 8).toString("ascii"), "MOSSPLM1");
  assert.equal(bytes.readUInt16LE(8), SCHEMA);
  assert.equal(report.raw_bytes, bytes.length);
  assert.equal(report.brotli_bytes, compressed.length);
  assert.ok(report.margin_percent >= 5);
  assert.ok(compressed.length <= report.budget_bytes);
});

test("a sea-floor band covers the water deeper than its threshold, never the land or the shallows", () => {
  // Ten columns west to east: land (+50 m), a shelf (-50 m), deep water (-500 m).
  const size = 10;
  const axis = Array.from({ length: size }, (_, index) => index);
  const z = new Float32Array(size * size);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) z[y * size + x] = x < 3 ? 50 : x < 6 ? -50 : -500;
  const { sea_floor: seaFloor } = reliefBands(z, axis, axis, 1);
  const band = seaFloor.find((feature) => feature.properties.band === -100);
  assert.ok(band, "the -100 m band must exist over 500 m deep water");
  const inside = ([px, py]) => {
    let hit = false;
    for (const polygon of band.geometry.coordinates) for (const ring of polygon) {
      for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
        const [xi, yi] = ring[i]; const [xj, yj] = ring[j];
        if ((yi > py) !== (yj > py) && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) hit = !hit;
      }
    }
    return hit;
  };
  assert.equal(inside([8, 5]), true, "deep water belongs to the -100 m band");
  assert.equal(inside([4, 5]), false, "a 50 m shelf is not deeper than 100 m");
  assert.equal(inside([1, 5]), false, "land is never part of a sea-floor band");
});

test("the shallowest sea band reaches the coastline the land layer draws", () => {
  // Deep water west of x = 8, a 5 m shelf from there to the coast at x = 12,
  // and land beyond. Natural Earth's coastline sits at x = 12.
  const size = 20;
  // Typed, like the real grid's axes.
  const axis = Float64Array.from({ length: size }, (_, index) => index);
  const z = new Float32Array(size * size);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) z[y * size + x] = x < 8 ? -500 : x < 12 ? -5 : 40;
  const land = [{ geometry: { type: "Polygon", coordinates: [[[11.6, -1], [30, -1], [30, 30], [11.6, 30], [11.6, -1]]] } }];
  const covers = (bands, [px, py]) => bands.filter((feature) => feature.properties.band === -10).some((band) => {
    let hit = false;
    for (const polygon of band.geometry.coordinates) for (const ring of polygon) {
      for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
        const [xi, yi] = ring[i]; const [xj, yj] = ring[j];
        if ((yi > py) !== (yj > py) && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) hit = !hit;
      }
    }
    return hit;
  });
  const { sea_floor: seaFloor } = reliefBands(z, axis, axis, 1, 0, land);
  assert.equal(covers(seaFloor, [10, 10]), true, "the 5 m shelf off the coast belongs to the shallowest band");
  assert.equal(covers(seaFloor, [15, 10]), false, "land is never part of a sea-floor band");
  const deeper = seaFloor.find((feature) => feature.properties.band === -20);
  assert.ok(deeper, "the deeper bands still come from the grid alone");
  assert.equal(covers([{ ...deeper, properties: { band: -10 } }], [10, 10]), false, "a 5 m shelf is not 20 m deep");
});

test("land cut at the antimeridian stays land in the samples on it", () => {
  // A global 5-degree grid: shallow shelf everywhere, and a polar landmass
  // whose Natural Earth ring is cut at 180 degrees, as Antarctica's is.
  const lon = Float64Array.from({ length: 73 }, (_, i) => -180 + i * 5); const lat = Float64Array.from({ length: 37 }, (_, i) => -90 + i * 5);
  const z = new Float32Array(lon.length * lat.length);
  for (let y = 0; y < lat.length; y++) for (let x = 0; x < lon.length; x++) z[y * lon.length + x] = lat[y] <= -70 ? 100 : -5;
  const land = [{ geometry: { type: "Polygon", coordinates: [[[-180, -90], [180, -90], [180, -70], [-180, -70], [-180, -90]]] } }];
  const { sea_floor: seaFloor } = reliefBands(z, lon, lat, 5, 0, land);
  // Read as sea, the antimeridian column carried the shallowest band from
  // the Arctic down to the south pole.
  const [feature] = featuresFor(seaFloor.filter((f) => f.properties.band === -10), TOLERANCE.fine, 0, "sea_floor");
  assert.ok(Math.min(...feature.parts.flat().map(([, y]) => y)) >= -72.5);
});

test("the world tier refuses any layer but land", () => {
  const lake = { properties: {}, geometry: { type: "Polygon", coordinates: [[[0, 0], [2, 0], [2, 2], [0, 2], [0, 0]]] } };
  assert.throws(() => tierPayload(new Map([["lakes", [lake]]]), "world"), /the world tier stores land alone, not lakes/);
  assert.equal(tierPayload(new Map([["land", [lake]]]), "world").refs.length, 1);
});

test("a river's band carries its Natural Earth scalerank so strokes can taper by rank", () => {
  const river = { properties: { scalerank: 3 }, geometry: { type: "LineString", coordinates: [[35.6, 32.7], [35.6, 32.9], [35.7, 33.4]] } };
  const { bytes } = tierPayload(new Map([["rivers", [river]]]), "fine");
  // Layer record: id u8, kind u8, count u32, byte length u32; then the
  // feature: byte length u32, four i32 bounds, and the i16 band.
  assert.equal(bytes.readUInt8(0), 4, "rivers are layer 4");
  assert.equal(bytes.readInt16LE(10 + 4 + 16), 3);
});

test("an over-budget pack fails before anything is written", async () => {
  const dir = await mkdtemp(join(tmpdir(), "moss-place-map-budget-"));
  try {
    const out = join(dir, "pack.bin.br"); const reportPath = join(dir, "size.json");
    const bytes = Buffer.from("MOSSPLM1 a pack that cannot fit a ten-byte budget");
    await assert.rejects(writePack(bytes, { schema: 2 }, { out, reportPath, budgetBytes: 10 }), /over 10; nothing written/);
    await assert.rejects(stat(out), { code: "ENOENT" }, "an over-budget run must not replace the checked-in pack");
    await assert.rejects(stat(reportPath), { code: "ENOENT" });
    const report = await writePack(bytes, { schema: 2 }, { out, reportPath, budgetBytes: 10_000 });
    assert.equal((await stat(out)).size, report.brotli_bytes);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("a band that reaches a pole is clamped onto it instead of leaving WGS84", () => {
  // Five rows from pole to pole, all deep water: every sea-floor band
  // touches both grid edges, so d3-contour closes it along rows past them.
  const lon = [-180, -90, 0, 90, 180]; const lat = [-90, -45, 0, 45, 90];
  const z = new Float32Array(lon.length * lat.length).fill(-500);
  const { sea_floor: seaFloor } = reliefBands(z, lon, lat, 45);
  const band = seaFloor.find((feature) => feature.properties.band === -100);
  const latitudes = band.geometry.coordinates.flat(2).map(([, y]) => y);
  assert.ok(Math.max(...latitudes) <= 90 && Math.min(...latitudes) >= -90, `latitudes ${Math.min(...latitudes)}..${Math.max(...latitudes)}`);
});

test("a gently curving band edge keeps its curve instead of long straight chords", () => {
  // A 5-degree-radius circle sampled every 0.05 degrees, like a contour
  // traced from the 3-arcminute grid. At the locator tolerance a chord only
  // 0.028 degrees from the arc may span a whole degree (about 70 px), so a
  // chord-distance simplifier draws it as a polygon of long straight edges.
  const circle = [];
  for (let i = 0; i <= 628; i++) circle.push([100 + 5 * Math.cos(i / 100), 15 + 5 * Math.sin(i / 100)]);
  circle.push(circle[0]);
  const longestEdge = (layer) => {
    const [feature] = featuresFor([{ properties: { band: 100 }, geometry: { type: "Polygon", coordinates: [circle] } }], TOLERANCE.fine, 0, layer);
    const ring = feature.parts[0];
    let longest = 0;
    for (let i = 1; i < ring.length; i++) longest = Math.max(longest, Math.hypot(ring[i][0] - ring[i - 1][0], ring[i][1] - ring[i - 1][1]));
    return longest;
  };
  assert.ok(longestEdge("relief") < 0.5, `a relief band's longest edge is ${longestEdge("relief").toFixed(3)} degrees`);
});

test("a meandering river keeps its bends instead of one straight chord", () => {
  // Six degrees of river swinging 0.02 degrees either side of its course,
  // under the locator tolerance: Douglas-Peucker keeps only the two ends,
  // one 430 px chord on a 10-degree map.
  const course = [];
  for (let i = 0; i <= 600; i++) course.push([30 + i / 100, 35 + 0.02 * Math.sin((i / 100) * (2 * Math.PI / 0.6))]);
  const [feature] = featuresFor([{ properties: { scalerank: 2 }, geometry: { type: "LineString", coordinates: course } }], TOLERANCE.fine, 0, "rivers");
  const line = feature.parts[0];
  let longest = 0;
  for (let i = 1; i < line.length; i++) longest = Math.max(longest, Math.hypot(line[i][0] - line[i - 1][0], line[i][1] - line[i - 1][1]));
  assert.ok(longest < 0.5, `the river's longest edge is ${longest.toFixed(3)} degrees`);
  assert.deepEqual([line[0], line.at(-1)], [course[0], course.at(-1)].map(([x, y]) => [Math.round(x * QUANT) / QUANT, Math.round(y * QUANT) / QUANT]));
});

test("a band that crosses the antimeridian reaches it from both sides, whatever the source's 180E column holds", () => {
  // A global 5-degree grid: land around both poles, deep water between.
  const lon = Array.from({ length: 73 }, (_, i) => -180 + i * 5); const lat = Array.from({ length: 37 }, (_, i) => -90 + i * 5);
  const z = new Float32Array(lon.length * lat.length);
  for (let y = 0; y < lat.length; y++) for (let x = 0; x < lon.length; x++) z[y * lon.length + x] = Math.abs(lat[y]) >= 70 ? 100 : -500;
  // The source tiles' 180E column repeats 90E; here it is high ground.
  for (let y = 0; y < lat.length; y++) z[y * lon.length + lon.length - 1] = 6000;
  const { sea_floor: seaFloor } = reliefBands(z, lon, lat, 5, 1.2);
  const [feature] = featuresFor(seaFloor.filter((f) => f.properties.band === -100), TOLERANCE.fine, 0, "sea_floor");
  const lons = feature.parts.flat().map(([x]) => x);
  assert.equal(Math.min(...lons), -180);
  assert.equal(Math.max(...lons), 180);
  // Covered right up to the antimeridian on both sides, once.
  const covering = (x, y) => feature.parts.filter((ring) => {
    let inside = false;
    for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
      const [xi, yi] = ring[i]; const [xj, yj] = ring[j];
      if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
    return inside;
  }).length;
  assert.equal(covering(179.99, 0), 1);
  assert.equal(covering(-179.99, 0), 1);
});

// --- Place labels ----------------------------------------------------------

test("a city's display name strips the trailing administrative 市 and the override table wins over it", () => {
  assert.equal(cleanCityNameZht("Beijing", "北京市"), "北京");
  // No trailing 市: left alone.
  assert.equal(cleanCityNameZht("Hong Kong", "香港"), "香港");
  // The override table fires regardless of what the raw name_zht says.
  assert.equal(cleanCityNameZht("Washington", "華盛頓哥倫比亞特區"), "華盛頓");
});

test("buildCityLabels reads both Natural Earth field-name conventions, filters by scalerank, and cleans the display name", () => {
  const feature = (nameEn, scalerank, nameZht) => ({
    properties: { NAME_EN: nameEn, NAME_ZHT: nameZht, LATITUDE: 1, LONGITUDE: 2, SCALERANK: scalerank },
    geometry: { type: "Point", coordinates: [2, 1] },
  });
  const features = [
    feature("Beijing", 0, "北京市"),
    feature("Nowheresville", CITY_SCALERANK_MAX + 1, "無名市"), // below the cutoff: dropped
  ];
  const labels = buildCityLabels(features);
  assert.deepEqual(labels, [{ nameEn: "Beijing", nameZht: "北京", lng: 2, lat: 1, rank: 0 }]);
});

test("buildRangeLabels keeps only Range/mtn features and labels each at its largest ring's centroid", () => {
  const square = [[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]];
  const notARange = {
    properties: { FEATURECLA: "Desert", NAME_EN: "Sahara", NAME_ZHT: "撒哈拉", SCALERANK: 1 },
    geometry: { type: "Polygon", coordinates: [square] },
  };
  const range = {
    properties: { FEATURECLA: "Range/mtn", NAME_EN: "Testrange", NAME_ZHT: "測試山脈", SCALERANK: 4 },
    geometry: { type: "Polygon", coordinates: [square] },
  };
  const labels = buildRangeLabels([notARange, range]);
  assert.deepEqual(labels, [{ nameEn: "Testrange", nameZht: "測試山脈", lng: 5, lat: 5, rank: 4 }]);
});

test("buildPeakLabels keeps only mountain features at or under the peak rank cutoff", () => {
  const feature = (nameEn, scalerank) => ({
    properties: { featurecla: "mountain", name_en: nameEn, name_zht: `${nameEn}-zht`, lat_y: 27.98, long_x: 86.88, scalerank },
    geometry: { type: "Point", coordinates: [86.88, 27.98] },
  });
  const notAMountain = { properties: { featurecla: "pass", name_en: "Some Pass", scalerank: 1, lat_y: 0, long_x: 0 }, geometry: { type: "Point", coordinates: [0, 0] } };
  const labels = buildPeakLabels([feature("Everest", 1), feature("TooMinor", PEAK_SCALERANK_MAX + 1), notAMountain]);
  assert.deepEqual(labels, [{ nameEn: "Everest", nameZht: "Everest-zht", lng: 86.88, lat: 27.98, rank: 1 }]);
});

test("buildRiverLabels keeps named rivers at or under the river rank cutoff and simplifies the SAME way the pack's own rivers layer does", () => {
  // A meandering course within one simplification pass's tolerance of a
  // straight chord -- `featuresFor`'s own river call simplifies this away
  // to its endpoints, so the label line should too.
  const course = Array.from({ length: 30 }, (_, i) => {
    const t = i / 29;
    return [t * 2, t * 2 + 0.3 * Math.sin(t * Math.PI * 6) * TOLERANCE.fine];
  });
  const named = { properties: { name_en: "Testriver", name_zht: "測試河", scalerank: RIVER_SCALERANK_MAX }, geometry: { type: "LineString", coordinates: course } };
  const unnamed = { properties: { name_en: "", scalerank: 0 }, geometry: { type: "LineString", coordinates: course } };
  const tooMinor = { properties: { name_en: "Minor Creek", name_zht: "小溪", scalerank: RIVER_SCALERANK_MAX + 1 }, geometry: { type: "LineString", coordinates: course } };
  const [label] = buildRiverLabels([named, unnamed, tooMinor]);
  assert.equal(label.nameEn, "Testriver");
  assert.equal(label.nameZht, "測試河");
  assert.equal(label.rank, RIVER_SCALERANK_MAX);
  assert.ok(label.line.length >= 2 && label.line.length < course.length, `expected the meander simplified away, got ${label.line.length} of ${course.length} points`);
  assert.deepEqual(buildRiverLabels([unnamed, tooMinor]), [], "an unnamed or below-cutoff river contributes no label");
});

test("river label line equals the pack's river geometry when simplified via riverSimplification", () => {
  // Create a synthetic river with a meandering course and verify that
  // riverSimplification ensures the label line matches the pack's geometry.
  const course = Array.from({ length: 50 }, (_, i) => {
    const t = i / 49;
    return [10 + t * 5, 20 + 0.15 * Math.sin(t * Math.PI * 8) * TOLERANCE.fine];
  });
  const river = {
    properties: { scalerank: 2 },
    geometry: { type: "LineString", coordinates: course },
  };
  // Get the pack's river geometry via featuresFor.
  const [packFeature] = featuresFor([river], TOLERANCE.fine, 0, "rivers");
  const packRiver = packFeature.parts[0];
  // Get the label line geometry via riverSimplification.
  const simplification = riverSimplification(false);
  const labelParts = geometryParts(river.geometry, simplification.tolerance, false, simplification.minArea, simplification.byArea, simplification.primaryOnly);
  const labelLine = labelParts.reduce((a, b) => (b.length > a.length ? b : a));
  // Both must produce the same simplified geometry.
  assert.deepEqual(labelLine, packRiver, "label line must match pack's river geometry exactly");
});

test("compareLabels sorts by rank then name regardless of input order, so encodeLabels never depends on shapefile feature order", () => {
  const point = (nameEn, rank) => ({ nameEn, nameZht: "", lng: 0, lat: 0, rank });
  const items = [point("Charlie", 2), point("Alpha", 1), point("Bravo", 1), point("Delta", 0)];
  const sorted = [...items].sort(compareLabels);
  assert.deepEqual(sorted.map((i) => i.nameEn), ["Delta", "Alpha", "Bravo", "Charlie"]);
  // Reversing the input must not change the result: the sort, not
  // insertion order, decides it.
  const reversedInputSorted = [...items].reverse().sort(compareLabels);
  assert.deepEqual(reversedInputSorted.map((i) => i.nameEn), ["Delta", "Alpha", "Bravo", "Charlie"]);
});

test("encodeLabels is deterministic and an empty label set encodes to just the four zero counts", () => {
  const empty = { cities: [], ranges: [], peaks: [], rivers: [] };
  const bytes = encodeLabels(empty);
  // 1 schema byte + 4 groups x u16(0) count.
  assert.equal(bytes.length, 1 + 4 * 2);
  assert.deepEqual(bytes, encodeLabels(empty));

  const point = { nameEn: "Alpha", nameZht: "甲", lng: 1.5, lat: 2.5, rank: 3 };
  const withACity = { cities: [point], ranges: [], peaks: [], rivers: [] };
  const first = encodeLabels(withACity);
  const second = encodeLabels(withACity);
  assert.deepEqual(first, second, "encoding the same labels twice must produce identical bytes");
  assert.ok(first.length > bytes.length, "a non-empty group must add bytes over the empty baseline");
});
