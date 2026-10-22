/**
 * The historical layer's raster: which level and tiles a view wants, what
 * stands in while they load, what leaves the view, and what a bad or distant
 * layer costs.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { screenScale } from "../camera";
import { LayerRaster, levelForView, parseLayerIndex, polygonPoints, standIns, tilesForRect, type TileRef } from "../layer-raster";
import { project, WORLD_WIDTH } from "../projection";
import type { DetailLayer } from "../layers";

const VIEWPORT = { width: 800, height: 500 };
const BASE = "/_moss/map.abc/";
const span = (level: number) => WORLD_WIDTH / 2 ** level;
const CENTER = project(40.75, -73.98);
const UNIT = 1.5;
const LAYER: DetailLayer = { name: "atlas", extent: [-74.03, 40.68, -73.9, 40.88], maxZoom: 17, credit: "", tiles: "layers/atlas/index.json" };

/** The camera at `level`'s own density (one tile pixel per device pixel), centred on `at`. */
function cameraAt(level: number, at = CENTER) {
  const cover = screenScale({ x: 0, y: 0, zoom: 1 }, VIEWPORT);
  return { x: at.x, y: at.y, zoom: 256 / (span(level) * cover) };
}

const cellAt = (level: number, p = CENTER): [number, number] => [Math.floor(p.x / span(level)), Math.floor(p.y / span(level))];

/** An index with a 7x7 block of tiles around the centre at every level from `min` to `max`. */
function wireIndex(min: number, max: number, extra: Partial<Record<string, unknown>> = {}) {
  const levels: Record<string, Array<[number, number]>> = {};
  for (let level = min; level <= max; level++) {
    const [ci, cj] = cellAt(level);
    levels[level] = [];
    for (let j = cj - 3; j <= cj + 3; j++) for (let i = ci - 3; i <= ci + 3; i++) levels[level].push([i, j]);
  }
  const a = { x: CENTER.x - 0.05, y: CENTER.y - 0.05 };
  const b = { x: CENTER.x + 0.05, y: CENTER.y - 0.05 };
  const c = { x: CENTER.x, y: CENTER.y + 0.05 };
  return {
    min_level: min, max_level: max, tile_size: 256, levels,
    footprints: [{ plate: "p1", polygon: [[a.x, a.y], [b.x, b.y], [c.x, c.y], [a.x, a.y]] }],
    ...extra,
  };
}

describe("parseLayerIndex", () => {
  test("reads levels, tiles and footprints", () => {
    const index = parseLayerIndex(wireIndex(12, 14))!;
    expect(index.minLevel).toBe(12);
    expect(index.maxLevel).toBe(14);
    expect(index.levels.get(13)!.has(cellAt(13).join(","))).toBe(true);
    expect(index.footprints).toHaveLength(1);
    expect(index.footprints[0]).toHaveLength(4);
  });

  test.each([
    ["null", null],
    ["a string", "tiles"],
    ["no footprints", { ...wireIndex(12, 13), footprints: undefined }],
    ["a tile size other than 256", wireIndex(12, 13, { tile_size: 512 })],
    ["min above max", wireIndex(12, 13, { min_level: 14 })],
    ["a malformed cell", wireIndex(12, 13, { levels: { "12": [["a", 1]] } })],
    ["a two-point polygon", wireIndex(12, 13, { footprints: [{ plate: "x", polygon: [[0, 0], [1, 1]] }] })],
  ])("rejects %s", (_name, body) => {
    expect(parseLayerIndex(body)).toBeNull();
  });
});

describe("levelForView", () => {
  const index = { minLevel: 12, maxLevel: 16 };
  const upp = (level: number) => span(level) / 256;

  test("picks the level whose world units per pixel match the view's", () => {
    for (const level of [12, 14, 16]) expect(levelForView(index, upp(level))).toBe(level);
  });

  test("picks the nearer level between two", () => {
    expect(levelForView(index, upp(14) * 1.3)).toBe(14);
    expect(levelForView(index, upp(14) * 1.6)).toBe(13);
  });

  test("a view finer than the deepest level uses the deepest", () => {
    expect(levelForView(index, upp(19))).toBe(16);
  });

  test("a view up to a factor of 2 coarser than the shallowest level still uses it, beyond that none", () => {
    expect(levelForView(index, upp(12) * 1.99)).toBe(12);
    expect(levelForView(index, upp(12) * 2.1)).toBeNull();
    expect(levelForView(index, upp(5))).toBeNull();
  });
});

describe("tilesForRect and standIns", () => {
  const index = parseLayerIndex(wireIndex(12, 14))!;
  const [ci, cj] = cellAt(14);
  const around = { x: (ci + 0.5) * span(14), y: (cj + 0.5) * span(14), width: 0.1 * span(14), height: 0.1 * span(14) };

  test("the tile under a rect at the chosen level", () => {
    expect(tilesForRect(index, 14, around)).toEqual([{ level: 14, i: ci, j: cj }]);
  });

  test("a cell the index does not list is covered by its finest listed ancestor", () => {
    const sparse = parseLayerIndex(wireIndex(12, 14, { levels: { "12": [cellAt(12)], "13": [cellAt(13)] } }))!;
    expect(tilesForRect(sparse, 14, around)).toEqual([{ level: 13, i: cellAt(13)[0], j: cellAt(13)[1] }]);
  });

  test("a loaded parent stands in for an unloaded child, and nothing stands in once the child is loaded", () => {
    const child: TileRef = { level: 14, i: ci, j: cj };
    const parent: TileRef = { level: 13, i: ci >> 1, j: cj >> 1 };
    expect(standIns([child], new Map([["13/" + parent.i + "/" + parent.j, parent]]))).toEqual([parent]);
    expect(standIns([child], new Map([["13/" + parent.i + "/" + parent.j, parent], ["14/" + ci + "/" + cj, child]]))).toEqual([]);
  });

  test("loaded children stand in for a wanted parent after zooming out", () => {
    const parent: TileRef = { level: 13, i: ci >> 1, j: cj >> 1 };
    const kid: TileRef = { level: 14, i: ci, j: cj };
    const elsewhere: TileRef = { level: 14, i: ci + 40, j: cj };
    const loaded = new Map([["14/" + ci + "/" + cj, kid], ["14/" + (ci + 40) + "/" + cj, elsewhere]]);
    expect(standIns([parent], loaded)).toEqual([kid]);
  });
});

describe("LayerRaster", () => {
  type Pending = { url: string; signal: AbortSignal | undefined; resolve: () => void; reject: (e: unknown) => void };
  let pending: Pending[];
  let calls: string[];
  let indexResponse: () => Response | Promise<Response>;

  const ok = (body: unknown) => ({ ok: true, json: async () => body, blob: async () => new Blob(["jpeg"]) }) as unknown as Response;

  beforeEach(() => {
    pending = [];
    calls = [];
    indexResponse = () => ok(wireIndex(12, 15));
    let n = 0;
    URL.createObjectURL = vi.fn(() => `blob:tile-${n++}`);
    URL.revokeObjectURL = vi.fn();
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      calls.push(url);
      if (url.endsWith("index.json")) return Promise.resolve(indexResponse());
      return new Promise<Response>((resolve, reject) => {
        const entry: Pending = { url, signal: init?.signal ?? undefined, resolve: () => resolve(ok(null)), reject };
        init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        pending.push(entry);
      });
    }));
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const flush = async () => {
    for (let i = 0; i < 6; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  };
  const tileCalls = () => calls.filter((url) => !url.endsWith("index.json"));
  const images = (raster: LayerRaster) => [...raster.el.querySelectorAll<SVGImageElement>("image")].map((el) => el.dataset.tile);

  async function drawn(level = 14, layers = [LAYER]) {
    const raster = new LayerRaster(layers, BASE);
    raster.render(cameraAt(level), VIEWPORT, UNIT);
    await flush();
    return raster;
  }

  test("the index is fetched once, relative to the map directory, when the layer first comes into view", async () => {
    const raster = await drawn();
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    raster.render(cameraAt(15), VIEWPORT, UNIT);
    await flush();
    expect(calls.filter((url) => url.endsWith("index.json"))).toEqual([`${location.origin}${BASE}layers/atlas/index.json`]);
  });

  test("no index is fetched for a layer out of view, and one is fetched when the view reaches it", async () => {
    const raster = new LayerRaster([LAYER], BASE);
    raster.render(cameraAt(14, project(48.85, 2.35)), VIEWPORT, UNIT);
    await flush();
    expect(calls).toEqual([]);
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    expect(calls.filter((url) => url.endsWith("index.json"))).toHaveLength(1);
  });

  test("a layer without a tile index is never fetched", async () => {
    const raster = new LayerRaster([{ ...LAYER, tiles: null }], BASE);
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    expect(calls).toEqual([]);
  });

  test.each([
    ["a failed request", () => Promise.reject(new Error("offline"))],
    ["an error status", () => ({ ok: false, status: 404 }) as Response],
    ["a body that is not an index", () => ok({ hello: "world" })],
    ["a body that is not JSON", () => ({ ok: true, json: async () => { throw new SyntaxError("bad"); } }) as unknown as Response],
  ])("%s means no layer and no crash", async (_name, response) => {
    indexResponse = response;
    const raster = await drawn();
    expect(tileCalls()).toEqual([]);
    expect(raster.el.querySelector("clipPath")).toBeNull();
    expect(images(raster)).toEqual([]);
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    expect(calls).toHaveLength(1);
  });

  test("requests the tiles of the level matching the view, at their contract paths", async () => {
    await drawn(14);
    const levels = new Set(tileCalls().map((url) => url.split("/layers/atlas/")[1].split("/")[0]));
    expect([...levels]).toEqual(["14"]);
    expect(tileCalls().every((url) => /\/layers\/atlas\/14\/\d+\/\d+\.jpg$/.test(url))).toBe(true);
    const [ci, cj] = cellAt(14);
    expect(tileCalls()).toContain(`${location.origin}${BASE}layers/atlas/14/${ci}/${cj}.jpg`);
  });

  test("requests nothing when the view is coarser than the shallowest level by more than a factor of 2", async () => {
    await drawn(9);
    expect(tileCalls()).toEqual([]);
  });

  test("limits the tiles loading at once", async () => {
    await drawn(14);
    expect(pending.length).toBeGreaterThan(0);
    expect(pending.length).toBeLessThanOrEqual(6);
  });

  test("tiles that leave the view are aborted, and ones still wanted are not", async () => {
    const raster = await drawn(14);
    const first = [...pending];
    expect(first.length).toBeGreaterThan(0);
    const east = { x: CENTER.x + 20 * span(14), y: CENTER.y };
    raster.render(cameraAt(14, east), VIEWPORT, UNIT);
    await flush();
    expect(first.every((entry) => entry.signal!.aborted)).toBe(true);
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    const again = pending.filter((entry) => !first.includes(entry));
    expect(again.length).toBeGreaterThan(0);
    expect(again.every((entry) => !entry.signal!.aborted)).toBe(true);
  });

  test("an aborted tile is not cached as failed: it loads when the view returns", async () => {
    const raster = await drawn(14);
    const url = pending[0].url;
    raster.render(cameraAt(14, { x: CENTER.x + 20 * span(14), y: CENTER.y }), VIEWPORT, UNIT);
    await flush();
    pending.length = 0;
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    expect(pending.map((entry) => entry.url)).toContain(url);
  });

  test("a loaded tile is drawn at its world position and size", async () => {
    const raster = await drawn(14);
    const [ci, cj] = cellAt(14);
    pending.find((entry) => entry.url.endsWith(`/14/${ci}/${cj}.jpg`))!.resolve();
    await flush();
    const image = raster.el.querySelector<SVGImageElement>(`image[data-tile="14/${ci}/${cj}"]`)!;
    expect(Number(image.getAttribute("x"))).toBeCloseTo(ci * span(14), 9);
    expect(Number(image.getAttribute("y"))).toBeCloseTo(cj * span(14), 9);
    expect(Number(image.getAttribute("width"))).toBeCloseTo(span(14), 3);
    expect(image.getAttribute("href")).toMatch(/^blob:/);
  });

  test("while a tile loads, its loaded parent is drawn in its place, and leaves once the tile is in", async () => {
    const raster = await drawn(13);
    for (const entry of [...pending]) entry.resolve();
    await flush();
    const [pi, pj] = cellAt(13);
    expect(images(raster)).toContain(`13/${pi}/${pj}`);
    pending.length = 0;

    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    const [ci, cj] = cellAt(14);
    expect(images(raster)).toContain(`13/${pi}/${pj}`);
    expect(images(raster)).not.toContain(`14/${ci}/${cj}`);

    pending.find((entry) => entry.url.endsWith(`/14/${ci}/${cj}.jpg`))!.resolve();
    await flush();
    expect(images(raster)).toContain(`14/${ci}/${cj}`);
    // The parent still stands in for the child tiles that have not loaded.
    const groups = [...raster.el.querySelectorAll<SVGGElement>("g[data-level]")].map((g) => g.dataset.level);
    expect(groups).toEqual(["13", "14"]);
  });

  test("a coarser level's group is drawn before a finer one's", async () => {
    const raster = await drawn(14);
    for (const entry of [...pending]) entry.resolve();
    await flush();
    raster.render(cameraAt(13), VIEWPORT, UNIT);
    await flush();
    for (const entry of [...pending]) entry.resolve();
    await flush();
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    const levels = [...raster.el.querySelectorAll<SVGGElement>("g[data-level]")].map((g) => Number(g.dataset.level));
    expect(levels).toEqual([...levels].sort((a, b) => a - b));
  });

  test("the layer is clipped to its footprints, one polygon per plate in world coordinates", async () => {
    const wire = wireIndex(12, 15, {
      footprints: [
        { plate: "a", polygon: [[1, 2], [3, 2], [3, 4], [1, 2]] },
        { plate: "b", polygon: [[5, 6], [7, 6], [7, 8], [5, 6]] },
      ],
    });
    indexResponse = () => ok(wire);
    const raster = await drawn(14);
    const clip = raster.el.querySelector("clipPath")!;
    expect([...clip.querySelectorAll("polygon")].map((p) => p.getAttribute("points"))).toEqual(["1,2 3,2 3,4 1,2", "5,6 7,6 7,8 5,6"]);
    const group = raster.el.querySelector("g[data-layer]")!;
    expect(group.getAttribute("clip-path")).toBe(`url(#${clip.id})`);
    // The footprints are in world units; the group they sit in scales them to the world element's px.
    expect(raster.el.querySelector("g[data-layer]")!.parentElement!.getAttribute("transform")).toBe(`scale(${UNIT})`);
  });

  test("polygonPoints writes world coordinates verbatim", () => {
    expect(polygonPoints([{ x: 1.5, y: 2 }, { x: 3, y: 4.25 }, { x: 0, y: 0 }])).toBe("1.5,2 3,4.25 0,0");
  });

  test("a failed tile is not retried and the rest still draw", async () => {
    const raster = await drawn(14);
    const failing = pending[0];
    failing.reject(new Error("404"));
    for (const entry of pending.slice(1)) entry.resolve();
    await flush();
    const before = tileCalls().filter((url) => url === failing.url).length;
    raster.render(cameraAt(14), VIEWPORT, UNIT);
    await flush();
    expect(tileCalls().filter((url) => url === failing.url)).toHaveLength(before);
    expect(images(raster).length).toBeGreaterThan(0);
  });

  test("tiles drop out of the drawing when the view leaves the layer, and their requests are aborted", async () => {
    const raster = await drawn(14);
    const first = [...pending];
    raster.render(cameraAt(14, project(48.85, 2.35)), VIEWPORT, UNIT);
    await flush();
    expect(first.every((entry) => entry.signal!.aborted)).toBe(true);
    expect(images(raster)).toEqual([]);
  });

  test("the state attribute is busy while tiles load and idle once they are in", async () => {
    const raster = await drawn(14);
    expect(raster.el.dataset.mossPlacesLayersState).toBe("busy");
    for (let round = 0; round < 12 && pending.length; round++) {
      for (const entry of pending.splice(0)) entry.resolve();
      await flush();
    }
    expect(raster.el.dataset.mossPlacesLayersState).toBe("idle");
  });
});
