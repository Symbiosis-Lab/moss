import { describe, expect, test } from "vitest";
import { aspectBlend, aspectX, openingMaxZoom, screenScale, screenToWorld, worldToScreen } from "../camera";
import { creditInView, degreesPerPxAtZoom, layerCeilingZoom, parseLayers, type DetailLayer } from "../layers";
import { latitudeSpanDegrees, project, trueAspect, unproject, WORLD_WIDTH } from "../projection";

const R = 6371008.8;
const rad = (deg: number) => (deg * Math.PI) / 180;

/** Ground metres per world unit east and north at `lat`, by differencing the projection itself. */
function groundPerWorldUnit(lat: number): { east: number; north: number } {
  const d = 1e-4;
  const a = project(lat, 0);
  const east = project(lat, d);
  const north = project(lat + d, 0);
  return {
    east: (R * Math.cos(rad(lat)) * rad(d)) / (east.x - a.x),
    north: (R * rad(d)) / (a.y - north.y),
  };
}

describe("trueAspect", () => {
  test.each([0, 40.7, 60])("at %s degrees an east metre and a north metre draw the same length", (lat) => {
    const { east, north } = groundPerWorldUnit(lat);
    // Drawn length of one ground metre east is k / east world units; north is 1 / north.
    const k = trueAspect(lat);
    expect(k / east).toBeCloseTo(1 / north, 8);
  });

  test("known values", () => {
    expect(trueAspect(0)).toBeCloseTo(1.0148, 6);
    expect(trueAspect(40.7)).toBeCloseTo(0.9048, 3);
    expect(trueAspect(60)).toBeCloseTo(0.6918, 3);
  });

  test("the uncorrected east-west stretch at 40.7 N is 10.5%, not the 13% the proposal estimated", () => {
    const { east, north } = groundPerWorldUnit(40.7);
    expect(north / east).toBeCloseTo(1 / trueAspect(40.7), 4);
    expect(north / east).toBeCloseTo(1.105, 3);
  });
});

describe("aspectBlend", () => {
  test.each([
    [3, 0],
    [2, 0],
    [1.25, 0.5],
    [0.5, 1],
    [0.4, 1],
  ])("a %s degree span applies %s of the correction", (span, share) => {
    expect(aspectBlend(span)).toBeCloseTo(share, 10);
  });
});

describe("aspectX", () => {
  const viewport = { width: 800, height: 500 };
  const at = (lat: number, spanDegrees: number) => {
    const y = project(lat, 0).y;
    // Find the zoom whose latitude span is `spanDegrees` by bisection on screenScale.
    let lo = 1;
    let hi = 1e6;
    for (let i = 0; i < 80; i++) {
      const mid = Math.sqrt(lo * hi);
      const span = latitudeSpanDegrees(viewport.height / screenScale({ x: 0, y, zoom: mid }, viewport), lat);
      if (span > spanDegrees) lo = mid;
      else hi = mid;
    }
    return { x: project(lat, -73.98).x, y, zoom: hi };
  };

  test("is 1 for a view of 3 degrees, partial at 1.25, full at 0.4", () => {
    const lat = 40.7;
    expect(aspectX(at(lat, 3), viewport)).toBeCloseTo(1, 6);
    const full = trueAspect(unproject(0, at(lat, 0.4).y).lat);
    expect(aspectX(at(lat, 0.4), viewport)).toBeCloseTo(full, 6);
    expect(aspectX(at(lat, 1.25), viewport)).toBeCloseTo(1 + (full - 1) * 0.5, 4);
  });

  test("worldToScreen and screenToWorld are inverses under the correction", () => {
    const camera = at(40.75, 0.2);
    const point = { x: camera.x + 0.01, y: camera.y - 0.004 };
    const screen = worldToScreen(point, camera, viewport);
    const back = screenToWorld(screen, camera, viewport);
    expect(back.x).toBeCloseTo(point.x, 9);
    expect(back.y).toBeCloseTo(point.y, 9);
  });

  test("a ground east-metre and north-metre draw the same length at city scale", () => {
    const lat = 40.75;
    const camera = at(lat, 0.2);
    const o = worldToScreen(project(lat, -73.98), camera, viewport);
    const dLng = 0.001;
    const e = worldToScreen(project(lat, -73.98 + dLng), camera, viewport);
    const dLat = dLng * Math.cos(rad(lat));
    const n = worldToScreen(project(lat + dLat, -73.98), camera, viewport);
    // Equal ground distances (dLng * cos(lat) degrees of ground each way).
    expect(Math.abs(e.x - o.x)).toBeCloseTo(Math.abs(o.y - n.y), 3);
  });
});

describe("layers", () => {
  const viewport = { width: 800, height: 500 };
  const layer: DetailLayer = { name: "atlas", extent: [-74.03, 40.68, -73.9, 40.88], maxZoom: 17, credit: "Atlas of the City, 1930", tiles: null };
  const inside = project(40.75, -73.98);
  const outside = project(35, 139);

  test("the ceiling inside an extent is the layer's max_zoom, and absent outside", () => {
    const zoom = layerCeilingZoom([layer], viewport, inside);
    expect(zoom).toBeCloseTo(openingMaxZoom(viewport, degreesPerPxAtZoom(17)), 6);
    expect(degreesPerPxAtZoom(17)).toBeCloseTo(360 / (256 * 2 ** 17), 12);
    expect(layerCeilingZoom([layer], viewport, outside)).toBeNull();
  });

  test("credit shows for a layer in view, joined across layers, and is empty out of view", () => {
    const other: DetailLayer = { ...layer, name: "streets", credit: "Street data" };
    const near = { x: inside.x - 5, y: inside.y - 5, width: 10, height: 10 };
    expect(creditInView([layer, other], near)).toBe("Atlas of the City, 1930 · Street data");
    expect(creditInView([layer], { x: outside.x - 5, y: outside.y - 5, width: 10, height: 10 })).toBe("");
    expect(creditInView([{ ...layer, credit: "" }], near)).toBe("");
  });

  test("parseLayers drops malformed entries and unknown versions", () => {
    expect(parseLayers({ version: 2, layers: [] })).toEqual([]);
    expect(parseLayers(null)).toEqual([]);
    const parsed = parseLayers({ version: 1, layers: [{ name: "a", kind: "historical", extent: [1, 2, 3, 4], max_zoom: 16, credit: " c ", tiles: null }, { extent: [1], max_zoom: 3 }] });
    expect(parsed).toEqual([{ name: "a", extent: [1, 2, 3, 4], frame: [1, 2, 3, 4], maxZoom: 16, credit: "c", tiles: null }]);
    const framed = parseLayers({ version: 1, layers: [{ name: "a", extent: [1, 2, 3, 4], frame: [0, 1, 5, 6], max_zoom: 16 }] });
    expect(framed[0].frame).toEqual([0, 1, 5, 6]);
    const withTiles = parseLayers({ version: 1, layers: [{ name: "a", extent: [1, 2, 3, 4], max_zoom: 16, tiles: "layers/a/index.json" }] });
    expect(withTiles[0].tiles).toBe("layers/a/index.json");
  });

  test("the ceiling reaches max_zoom anywhere in the frame, even off the plates, and nowhere outside it", () => {
    const layer = { name: "a", extent: [-74.02, 40.70, -74.01, 40.71], frame: [-74.05, 40.65, -73.80, 40.90], maxZoom: 17, credit: "", tiles: null } as DetailLayer;
    const offPlate = project(40.80, -73.90);
    const outside = project(41.50, -73.00);
    expect(layerCeilingZoom([layer], { width: 800, height: 500 }, offPlate)).not.toBeNull();
    expect(layerCeilingZoom([layer], { width: 800, height: 500 }, outside)).toBeNull();
  });

  test("WORLD_WIDTH is unchanged by layers (sanity)", () => {
    expect(WORLD_WIDTH).toBeGreaterThan(800);
  });
});
