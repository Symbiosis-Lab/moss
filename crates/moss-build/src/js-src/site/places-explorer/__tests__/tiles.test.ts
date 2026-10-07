/**
 * Tests for tiles.ts's pure geometry — the DOM/fetch half (`TileLayer`) is
 * covered by the render gates for a real, composited viewport (the same
 * split `markers.ts`'s own tests draw for `MarkerLayer`), except for
 * `TileLayer`'s own neighbour-load-state bookkeeping below: whether a
 * neighbour is loaded, failed, or still in flight only exists on a real
 * `TileLayer` instance (`tileEdgeMask` itself is pure
 * and take whatever cell list they're handed), so that part is exercised
 * here instead, against a mocked, controllable `fetch`.
 */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, test, expect, vi } from "vitest";
import { detailMaxZoom, MIN_ZOOM } from "../camera";
import { WORLD_WIDTH, WORLD_HEIGHT } from "../projection";
import { TileLayer, tileCellBounds, tileEdgeMask, tileFadeOpacity, tileOverlayTransform, tilesForView } from "../tiles";

// `rasterizeOrFallback` defaults to delegating to the real implementation
// (the no-op jsdom fallback every other test in this file relies on), and
// one test below overrides it twice, in sequence, to pin exactly when each
// of two concurrent loads of the SAME cell resolves — the race the
// generation-token fix in `TileLayer.load` exists to settle.
const { rasterizeOrFallbackSpy } = vi.hoisted(() => ({ rasterizeOrFallbackSpy: vi.fn() }));
vi.mock("../raster", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../raster")>();
  rasterizeOrFallbackSpy.mockImplementation(actual.rasterizeOrFallback);
  return { ...actual, rasterizeOrFallback: rasterizeOrFallbackSpy };
});

/** The canvas origins a build writes to `tiles.json` for `cells`: the padded cell's top-left, snapped down to a whole canvas unit. */
const GRID = { columns: 36, rows: 18 };

function originsFor(cells: Array<[number, number]>, k: number, bleed: number): Record<string, [number, number]> {
  const origins: Record<string, [number, number]> = {};
  for (const [x, y] of cells) {
    const bounds = tileCellBounds(x, y);
    origins[`${x},${y}`] = [Math.floor((bounds.minX - bleed) * k), Math.floor((bounds.minY - bleed) * k)];
  }
  return origins;
}

describe("tileCellBounds", () => {
  test("the world's own four corner cells sit at the canvas edges", () => {
    // x=0 is the westmost cell (-180..-170), y=0 the southmost (-90..-80).
    const southwest = tileCellBounds(0, 0);
    expect(southwest.minX).toBeCloseTo(0);
    expect(southwest.maxY).toBeCloseTo(WORLD_HEIGHT);
    // x=35 is the eastmost cell (170..180), y=17 the northmost (80..90).
    const northeast = tileCellBounds(35, 17);
    expect(northeast.maxX).toBeCloseTo(WORLD_WIDTH);
    expect(northeast.minY).toBeCloseTo(0);
  });

  test("adjacent cells share an edge with no gap or overlap", () => {
    const a = tileCellBounds(10, 5);
    const b = tileCellBounds(11, 5);
    expect(a.maxX).toBeCloseTo(b.minX);
  });
});

describe("tilesForView", () => {
  const cells: Array<[number, number]> = [
    [0, 0],
    [10, 5],
    [11, 5],
    [35, 17],
  ];

  test("a cover-zoom camera over the whole world sees every cell", () => {
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: 1 };
    const viewport = { width: 1200, height: 700 };
    const visible = tilesForView(cells, camera, viewport);
    expect(visible).toEqual(expect.arrayContaining(cells));
  });

  test("a tight zoom over one cell excludes a cell on the far side of the map", () => {
    const bounds = tileCellBounds(10, 5);
    const centerX = (bounds.minX + bounds.maxX) / 2;
    const centerY = (bounds.minY + bounds.maxY) / 2;
    const camera = { x: centerX, y: centerY, zoom: 20 };
    const viewport = { width: 800, height: 500 };
    const visible = tilesForView(cells, camera, viewport);
    expect(visible).toEqual(expect.arrayContaining([[10, 5]]));
    expect(visible).not.toEqual(expect.arrayContaining([[0, 0]]));
    expect(visible).not.toEqual(expect.arrayContaining([[35, 17]]));
  });

  test("a zero pad keeps only cells that touch the view, not the loaded margin around it", () => {
    const bounds = tileCellBounds(10, 5);
    const camera = { x: (bounds.minX + bounds.maxX) / 2, y: (bounds.minY + bounds.maxY) / 2, zoom: 40 };
    const viewport = { width: 800, height: 500 };
    const neighbours: Array<[number, number]> = [[10, 5], [11, 5]];
    expect(tilesForView(neighbours, camera, viewport)).toEqual(neighbours);
    expect(tilesForView(neighbours, camera, viewport, 0)).toEqual([[10, 5]]);
  });

  test("an empty cell list selects nothing, however wide the view", () => {
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: 1 };
    const viewport = { width: 1200, height: 700 };
    expect(tilesForView([], camera, viewport)).toEqual([]);
  });
});

describe("tileFadeOpacity", () => {
  const viewport = { width: 1200, height: 700 };

  test("is zero at the world's own cover zoom — tiles are absent at rest", () => {
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: MIN_ZOOM };
    expect(tileFadeOpacity(camera, viewport)).toBe(0);
  });

  test("reaches full opacity exactly at the world's own detail ceiling — tiles are present past it", () => {
    const ceiling = detailMaxZoom(viewport);
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: ceiling };
    expect(tileFadeOpacity(camera, viewport)).toBe(1);
  });

  test("ramps linearly across the fade band and stays zero short of it", () => {
    const ceiling = detailMaxZoom(viewport);
    const fadeStart = ceiling * 0.8; // the last 20% of the zoom range
    const justShort = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: fadeStart - 0.01 };
    const midBand = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: (fadeStart + ceiling) / 2 };
    expect(tileFadeOpacity(justShort, viewport)).toBe(0);
    expect(tileFadeOpacity(midBand, viewport)).toBeCloseTo(0.5);
  });
});

describe("tileOverlayTransform", () => {
  const unitScale = 2.3;

  test("places the tile's own (0, 0) at its origin, scaled by unitScale / k", () => {
    const { translateX, translateY, scale } = tileOverlayTransform([-37, 1204], unitScale, 4);
    expect(scale).toBe(unitScale / 4);
    expect(translateX).toBe(-37 * (unitScale / 4));
    expect(translateY).toBe(1204 * (unitScale / 4));
  });

  // The origins a real build wrote (`tiles.json`'s `origins`) sit on whole
  // canvas units at or just before the padded cell's corner, which is what
  // puts every tile's integer vertices on one lattice. The exact floor is
  // pinned on the Rust side by
  // `tile_viewbox_covers_its_cell_padded_by_bleed_on_whole_units`; here the
  // client's own view of the cell must agree to within one unit of `k`.
  const mapDir = join(__dirname, "../../../../../tests/fixtures/snapshot-sites/places-site/expected/_moss");
  const index = JSON.parse(
    readFileSync(join(mapDir, readdirSync(mapDir).find((name) => name.startsWith("map."))!, "tiles.json"), "utf8"),
  ) as { k: number; cells: Array<[number, number]>; origins: Record<string, [number, number]> };

  test("every emitted tile's origin is a whole-unit pair at or before its cell's top-left corner", () => {
    expect(index.cells.length).toBeGreaterThan(0);
    for (const [x, y] of index.cells) {
      const bounds = tileCellBounds(x, y);
      const origin = index.origins[`${x},${y}`];
      expect(Number.isInteger(origin[0]) && Number.isInteger(origin[1]), `(${x},${y}) whole units`).toBe(true);
      expect(origin[0], `(${x},${y}) west`).toBeLessThanOrEqual(bounds.minX * index.k);
      expect(origin[1], `(${x},${y}) north`).toBeLessThanOrEqual(bounds.minY * index.k);
      expect(bounds.minX * index.k - origin[0], `(${x},${y}) west gap`).toBeLessThan(index.k);
      expect(bounds.minY * index.k - origin[1], `(${x},${y}) north gap`).toBeLessThan(index.k);
      const { translateX, translateY, scale } = tileOverlayTransform(origin, unitScale, index.k);
      expect(translateX / scale).toBeCloseTo(origin[0], 9);
      expect(translateY / scale).toBeCloseTo(origin[1], 9);
    }
  });
});

/**
 * Draw order is part of the picture: where two tiles overlap, the one with the
 * higher z-index wins. The index is a fixed function of the cell, so the same
 * coastline is drawn by the same neighbour on every load, whatever order the
 * fetches finish in.
 */
describe("TileLayer — each tile's z-index is fixed by its cell, north to south then west to east, whatever order the fetches finish", () => {
  const TILE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"></svg>';
  const BASE_URL = "/_moss/tiles/";
  const VIEWPORT = { width: 1200, height: 700 };
  const CELLS: Array<[number, number]> = [[9, 5], [10, 5], [11, 5], [10, 4], [10, 6]];
  // North to south, west to east: a southern tile is drawn over the bleed of the one above it.
  const EXPECTED: Array<[number, number]> = [[10, 6], [9, 5], [10, 5], [11, 5], [10, 4]];
  const CAMERA = {
    x: (tileCellBounds(10, 5).minX + tileCellBounds(10, 5).maxX) / 2,
    y: (tileCellBounds(10, 5).minY + tileCellBounds(10, 5).maxY) / 2,
    zoom: detailMaxZoom(VIEWPORT),
  };

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("a tile that finishes last but has the lowest z-index still gets it", async () => {
    const resolvers = new Map<string, () => void>();
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) => new Promise((resolve) => {
        resolvers.set(url, () => resolve({ ok: true, text: () => Promise.resolve(TILE_SVG) } as unknown as Response));
      })),
    );
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: CELLS, k: 4, origins: originsFor(CELLS, 4, 0.1), ...GRID });
    layer.render(CAMERA, VIEWPORT, 1);

    // Finish in the reverse of the expected order, so the lowest z-index is the last to arrive.
    for (const [x, y] of [...EXPECTED].reverse()) {
      resolvers.get(`${BASE_URL}tile-${x}-${y}.svg`)!();
      await new Promise((resolve) => setTimeout(resolve, 0));
      await new Promise((resolve) => setTimeout(resolve, 0));
    }

    const zIndexes = new Map(
      [...container.querySelectorAll<HTMLElement>("[data-moss-places-tile]")].map((el) => [el.dataset.mossPlacesTile, Number(el.style.zIndex)]),
    );
    expect(zIndexes.size).toBe(EXPECTED.length);
    // Rows run 17 (north) to 0 (south) over 36 columns, and each z-index is its place in that order.
    expect(Object.fromEntries(zIndexes)).toEqual(Object.fromEntries(EXPECTED.map(([x, y]) => [`${x},${y}`, (17 - y) * 36 + x])));
    const byIndex = [...zIndexes].sort((a, b) => a[1] - b[1]).map(([key]) => key);
    expect(byIndex).toEqual(EXPECTED.map(([x, y]) => `${x},${y}`));
  });
});

describe("tileEdgeMask", () => {
  const cells: Array<[number, number]> = [
    [10, 5],
    [11, 5], // east neighbour of (10, 5)
    [10, 6], // north neighbour of (10, 5)
  ];

  test("a tile with every neighbour present carries no mask at all", () => {
    // Give (10, 5) all four neighbours for this one case only.
    const surrounded: Array<[number, number]> = [...cells, [9, 5], [10, 4]];
    expect(tileEdgeMask(10, 5, surrounded, 4)).toBeNull();
  });

  test("a tile missing exactly one neighbour carries exactly one gradient layer, fading that edge", () => {
    // (11, 5): west (10,5), north (11,6) and south (11,4) all present;
    // east (12,5) absent — the one edge this mask should fade.
    const onlyEastMissing: Array<[number, number]> = [[10, 5], [11, 5], [11, 6], [11, 4]];
    const mask = tileEdgeMask(11, 5, onlyEastMissing, 4);
    expect(mask).not.toBeNull();
    expect(mask!.split("linear-gradient").length - 1).toBe(1); // exactly one layer
    expect(mask).toContain("to left"); // fades the EAST edge, the one missing neighbour
  });

  test("a corner tile missing two neighbours on different axes carries two layers, composed to intersect", () => {
    // (10, 5) here has no west (9,5), no south (10,4) — both absent from
    // `cells` — but does have east (11,5) and north (10,6).
    const mask = tileEdgeMask(10, 5, cells, 4);
    expect(mask).not.toBeNull();
    expect(mask!.split("linear-gradient").length - 1).toBe(2); // exactly two layers
    expect(mask).toContain("to right"); // fades the WEST edge, which is missing
    expect(mask).toContain("to top"); // fades the SOUTH edge, which is missing
  });

  test("the fade width scales with k, not a fixed pixel amount", () => {
    const mask1 = tileEdgeMask(10, 5, cells, 1)!;
    const mask4 = tileEdgeMask(10, 5, cells, 4)!;
    expect(mask1).toContain("1.5px");
    expect(mask4).toContain("6px");
  });
});

/**
 * `TileLayer`'s own edge-mask bookkeeping: a neighbour the manifest names
 * counts as present until its fetch permanently fails (failures are cached
 * and never retried, this file's own `TileLayer` doc), after which the edge
 * it shared fades like any other outer edge. A tile is never clipped toward
 * a neighbour: the clip edge antialiased into a light line between tiles.
 */
describe("TileLayer — current-frame cells get priority over padding", () => {
  const TILE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"></svg>';
  const BASE_URL = "/_moss/tiles/";
  const CELLS: Array<[number, number]> = [[10, 5], [11, 5]];
  const VIEWPORT = { width: 800, height: 500 };
  const bounds = tileCellBounds(10, 5);
  const CAMERA = { x: (bounds.minX + bounds.maxX) / 2, y: (bounds.minY + bounds.maxY) / 2, zoom: 40 };

  function stubControllableFetch(): Map<string, { resolve: () => void; reject: () => void }> {
    const requests = new Map<string, { resolve: () => void; reject: () => void }>();
    vi.stubGlobal("fetch", vi.fn((url: string) => new Promise((resolve, reject) => {
      requests.set(url, {
        resolve: () => resolve({ ok: true, text: () => Promise.resolve(TILE_SVG) } as unknown as Response),
        reject: () => reject(new Error("tile fetch failed")),
      });
    })));
    return requests;
  }

  async function flush(): Promise<void> {
    for (let i = 0; i < 4; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  }

  afterEach(() => vi.unstubAllGlobals());

  test("loads and decodes the visible cell before scheduling one padding tile after two frames", async () => {
    const requests = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: CELLS, k: 4, origins: originsFor(CELLS, 4, 0.1), ...GRID });
    const frames: FrameRequestCallback[] = [];
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    try {
      expect(tilesForView(CELLS, CAMERA, VIEWPORT, 0)).toEqual([[10, 5]]);
      expect(tilesForView(CELLS, CAMERA, VIEWPORT)).toEqual(CELLS);
      layer.render(CAMERA, VIEWPORT, 1, true);
      expect([...requests.keys()]).toEqual([`${BASE_URL}tile-10-5.svg`]);

      const ready = layer.waitForVisibleTiles();
      requests.get(`${BASE_URL}tile-10-5.svg`)!.resolve();
      await flush();
      await expect(ready).resolves.toBe("ready");
      expect(container.querySelectorAll(".moss-places-tile")).toHaveLength(1);
      expect(requests.has(`${BASE_URL}tile-11-5.svg`)).toBe(false);

      frames.shift()!(0);
      expect(requests.has(`${BASE_URL}tile-11-5.svg`)).toBe(false);
      frames.shift()!(16);
      expect(requests.has(`${BASE_URL}tile-11-5.svg`)).toBe(true);
      expect(raf).toHaveBeenCalledTimes(2);
      requests.get(`${BASE_URL}tile-11-5.svg`)!.resolve();
      await flush();
    } finally {
      raf.mockRestore();
    }
  });

  test("a pan after clear starts its newly visible tile while stale padding decode is held", async () => {
    const requests = stubControllableFetch();
    const container = document.createElement("div");
    const cells: Array<[number, number]> = [[10, 4], [10, 5], [10, 6], [11, 5], [12, 5]];
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: cells, k: 4, origins: originsFor(cells, 4, 0.1), ...GRID });
    const frames: FrameRequestCallback[] = [];
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    let releaseBackgroundBake!: () => void;
    const originalRasterize = rasterizeOrFallbackSpy.getMockImplementation()!;
    let rasterizeCalls = 0;
    rasterizeOrFallbackSpy.mockImplementation(async () => {
      rasterizeCalls++;
      if (rasterizeCalls === 2) await new Promise<void>((resolve) => { releaseBackgroundBake = resolve; });
      return { el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} };
    });
    try {
      layer.render(CAMERA, VIEWPORT, 1, true);
      requests.get(`${BASE_URL}tile-10-5.svg`)!.resolve();
      await flush();
      expect(frames).toHaveLength(1);

      frames.shift()!(0);
      frames.shift()!(16);
      const background = [...requests.keys()].find((url) => url !== `${BASE_URL}tile-10-5.svg`)!;
      expect(background).toBeDefined();
      expect(requests.size).toBe(2); // only one padding load can occupy the background slot
      requests.get(background)!.resolve();
      await flush();
      expect(rasterizeCalls).toBe(2);

      const nextBounds = tileCellBounds(12, 5);
      layer.render({ ...CAMERA, zoom: 1 }, VIEWPORT, 1, true); // invalidate the in-flight background generation
      layer.render({ ...CAMERA, x: (nextBounds.minX + nextBounds.maxX) / 2 }, VIEWPORT, 1, true);
      expect(requests.has(`${BASE_URL}tile-12-5.svg`)).toBe(true);
      expect(requests.size).toBe(3); // the new visible tile starts while the background decode is still held
      releaseBackgroundBake();
      requests.get(`${BASE_URL}tile-12-5.svg`)!.resolve();
      await flush();
      layer.render({ ...CAMERA, zoom: 1 }, VIEWPORT, 1, true);
      await flush();
    } finally {
      releaseBackgroundBake?.();
      rasterizeOrFallbackSpy.mockImplementation(originalRasterize);
      raf.mockRestore();
    }
  });

  test("a failed visible tile does not release padding while another visible tile is still pending", async () => {
    const requests = stubControllableFetch();
    const cells: Array<[number, number]> = [[10, 5], [11, 5], [12, 5]];
    const camera = { ...CAMERA, zoom: 20 };
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: cells, k: 4, origins: originsFor(cells, 4, 0.1), ...GRID });
    const frames: FrameRequestCallback[] = [];
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    try {
      expect(tilesForView(cells, camera, VIEWPORT, 0)).toEqual([[10, 5], [11, 5]]);
      expect(tilesForView(cells, camera, VIEWPORT)).toEqual(cells);
      layer.render(camera, VIEWPORT, 1, true);
      const ready = layer.waitForVisibleTiles();
      requests.get(`${BASE_URL}tile-10-5.svg`)!.reject();
      await flush();
      await expect(ready).resolves.toBe("failed");
      expect(requests.has(`${BASE_URL}tile-12-5.svg`)).toBe(false);

      requests.get(`${BASE_URL}tile-11-5.svg`)!.resolve();
      await flush();
      expect(requests.has(`${BASE_URL}tile-12-5.svg`)).toBe(false);
      frames.shift()!(0);
      frames.shift()!(16);
      expect(requests.has(`${BASE_URL}tile-12-5.svg`)).toBe(true);
      requests.get(`${BASE_URL}tile-12-5.svg`)!.resolve();
      await flush();
    } finally {
      raf.mockRestore();
    }
  });
});

describe("TileLayer — a failed neighbour turns its shared edge into a fading outer edge", () => {
  const TILE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"></svg>';
  const BASE_URL = "/_moss/tiles/";
  const VIEWPORT = { width: 1200, height: 700 };
  const K = 4;
  const BLEED = 0.1;
  // Centred on the shared edge of (9,5)/(10,5), at the world's own detail
  // ceiling for this viewport — `TileLayer.render` is a no-op below it
  // (`tileFadeOpacity`, this file's own fade-band tests above), so a
  // cover-zoom camera (fine for the PURE `tilesForView` tests above, which
  // call it directly) would silently skip every fetch here. Both cells sit
  // comfortably inside `tilesForView`'s own pad at this zoom — a cell is
  // only ~23 world units wide, well under the ~90-unit half-width this
  // viewport/zoom/pad combination gives.
  const CAMERA = {
    x: (tileCellBounds(9, 5).minX + tileCellBounds(10, 5).maxX) / 2,
    y: (tileCellBounds(9, 5).minY + tileCellBounds(9, 5).maxY) / 2,
    zoom: detailMaxZoom(VIEWPORT),
  };

  /** A controllable stand-in for `fetch`, keyed by URL, so a test can resolve or reject one tile's request independently of any other's — the real shape a race between two in-flight cells takes. */
  function stubControllableFetch(): Map<string, { resolveOk: (text: string) => void; reject: (err: unknown) => void }> {
    const controllers = new Map<string, { resolveOk: (text: string) => void; reject: (err: unknown) => void }>();
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) => {
        return new Promise((resolve, reject) => {
          controllers.set(url, {
            resolveOk: (text: string) => resolve({ ok: true, text: () => Promise.resolve(text) } as unknown as Response),
            reject,
          });
        });
      }),
    );
    return controllers;
  }

  /** Two macrotask ticks — enough for the `response.ok` check, the `.text()` read, and the final append/clip `.then` (or the `.catch`) to all settle, since each is its own microtask hop past a resolved/rejected controller. */
  async function flush(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("a tile fades the edge toward a west neighbour whose fetch failed", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[9, 5], [10, 5]], k: K, origins: originsFor([[9, 5], [10, 5]], K, BLEED), ...GRID });

    layer.render(CAMERA, VIEWPORT, 1);
    controllers.get(`${BASE_URL}tile-10-5.svg`)!.resolveOk(TILE_SVG);
    controllers.get(`${BASE_URL}tile-9-5.svg`)!.reject(new Error("404"));
    await flush();

    const main = container.querySelector('[data-moss-places-tile="10,5"]') as unknown as HTMLElement;
    // The manifest named a west neighbour, but it will never arrive, so
    // that edge now reads as this layer's own outer edge and fades toward
    // the world layer instead of ending in a hard, undrawn line.
    expect(main.style.maskImage).toContain("to right");
  });

  test("a failed visible cell remains an initial-paint dependency even when zoom coverage ignores it", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const cells: Array<[number, number]> = [[9, 5]];
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: cells, k: K, origins: originsFor(cells, K, BLEED), ...GRID });

    layer.render(CAMERA, VIEWPORT, 1, true);
    const ready = layer.waitForVisibleTiles();
    controllers.get(`${BASE_URL}tile-9-5.svg`)!.reject(new Error("503"));
    await flush();

    await expect(ready).resolves.toBe("failed");
    expect(layer.hasVisibleTiles(CAMERA, VIEWPORT)).toBe(false);
    expect(layer.hasManifestTiles(CAMERA, VIEWPORT)).toBe(true);
  });

  test("clearing a pending visible-cell request reports superseded rather than a tile failure", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const cells: Array<[number, number]> = [[9, 5]];
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: cells, k: K, origins: originsFor(cells, K, BLEED), ...GRID });

    layer.render(CAMERA, VIEWPORT, 1, true);
    const ready = layer.waitForVisibleTiles();
    layer.render({ ...CAMERA, zoom: 1 }, VIEWPORT, 1, true);

    await expect(ready).resolves.toBe("superseded");
    controllers.get(`${BASE_URL}tile-9-5.svg`)!.reject(new Error("stale request"));
    await flush();
  });

  test("a pan — render() called again with only the camera moved — never reassigns an already-loaded tile's mask-image", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[9, 5], [10, 5]], k: K, origins: originsFor([[9, 5], [10, 5]], K, BLEED), ...GRID });

    layer.render(CAMERA, VIEWPORT, 1);
    controllers.get(`${BASE_URL}tile-10-5.svg`)!.resolveOk(TILE_SVG);
    controllers.get(`${BASE_URL}tile-9-5.svg`)!.resolveOk(TILE_SVG);
    await flush();

    const main = container.querySelector('[data-moss-places-tile="10,5"]') as unknown as HTMLElement;
    let maskImageSets = 0;
    let maskImageValue = main.style.maskImage;
    // Shadow the property on THIS style instance only — an own
    // property always wins the lookup over the inherited accessor, so this
    // counts every assignment `applyEdgeMask` (or a reverted `position`)
    // makes, without needing to know how the DOM implementation itself
    // wires up `CSSStyleDeclaration`.
    Object.defineProperty(main.style, "maskImage", {
      configurable: true,
      get: () => maskImageValue,
      set: (v: string) => { maskImageSets++; maskImageValue = v; },
    });

    for (let i = 0; i < 5; i++) layer.render({ ...CAMERA, x: CAMERA.x + i }, VIEWPORT, 1);

    expect(maskImageSets).toBe(0);
  });
});

/**
 * The race `TileLayer`'s own `generation` counter exists to settle: a
 * `clear()` landing while a cell's `load()` is in flight, followed by a
 * re-queue of that SAME cell, resets the per-cell sentinel back to
 * `"loading"` — the one signal the pre-existing staleness checks relied on.
 * Without the generation check, the ORIGINAL (pre-clear) load can still see
 * `"loading"` and finish as though nothing happened, if it happens to
 * resolve before the re-queued load does.
 */
describe("TileLayer — a clear()+re-queue of the same cell drops the stale load", () => {
  const TILE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"></svg>';
  const BASE_URL = "/_moss/tiles/";
  const VIEWPORT = { width: 1200, height: 700 };
  const K = 4;
  const BLEED = 0.1;
  const CELL_BOUNDS = tileCellBounds(10, 5);
  const IN_BAND_CAMERA = { x: (CELL_BOUNDS.minX + CELL_BOUNDS.maxX) / 2, y: (CELL_BOUNDS.minY + CELL_BOUNDS.maxY) / 2, zoom: detailMaxZoom(VIEWPORT) };
  const BELOW_BAND_CAMERA = { ...IN_BAND_CAMERA, zoom: MIN_ZOOM };

  function stubControllableFetch(): Map<string, { resolveOk: (text: string) => void }> {
    const controllers = new Map<string, { resolveOk: (text: string) => void }>();
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) => {
        return new Promise((resolve) => {
          controllers.set(url, { resolveOk: (text: string) => resolve({ ok: true, text: () => Promise.resolve(text) } as unknown as Response) });
        });
      }),
    );
    return controllers;
  }

  async function flush(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  afterEach(() => {
    vi.unstubAllGlobals();
    rasterizeOrFallbackSpy.mockClear();
  });

  test("a stale load that outlasts a clear()+re-queue releases its own surface and never appends; the fresh load wins", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[10, 5]], k: K, origins: originsFor([[10, 5]], K, BLEED), ...GRID });
    const url = `${BASE_URL}tile-10-5.svg`;

    const staleRelease = vi.fn();
    let resolveStaleRaster!: (surface: { el: Element; release: () => void }) => void;
    const staleRasterPromise = new Promise<{ el: Element; release: () => void }>((resolve) => {
      resolveStaleRaster = resolve;
    });
    const freshRelease = vi.fn();
    const freshEl = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    freshEl.setAttribute("data-test-fresh", "");

    // Call #1 (the stale, pre-clear load) pauses here until resolved below;
    // call #2 (the re-queued load) resolves as soon as it's awaited.
    rasterizeOrFallbackSpy.mockImplementationOnce(() => staleRasterPromise);
    rasterizeOrFallbackSpy.mockImplementationOnce(async () => ({ el: freshEl, release: freshRelease }));

    // Start the stale load: fetch is in flight.
    layer.render(IN_BAND_CAMERA, VIEWPORT, 1);
    controllers.get(url)!.resolveOk(TILE_SVG);
    await flush(); // past both fetch awaits and into `await rasterizeOrFallback`, now paused on staleRasterPromise

    // clear() (opacity drops to 0), then re-queue the SAME cell — a NEW
    // fetch starts (overwriting `controllers`' entry for this url) but is
    // not yet resolved, so the re-queued load is still just "loading".
    layer.render(BELOW_BAND_CAMERA, VIEWPORT, 1);
    layer.render(IN_BAND_CAMERA, VIEWPORT, 1);

    // The STALE load's own rasterize resolves first, while the re-queued
    // (fresh) load is still mid-fetch — the exact interleaving that lets a
    // stale load see the per-cell sentinel read "loading" again.
    resolveStaleRaster({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release: staleRelease });
    await flush();

    // Only now does the re-queued load's own fetch resolve.
    controllers.get(url)!.resolveOk(TILE_SVG);
    await flush();

    const tiles = container.querySelectorAll('[data-moss-places-tile="10,5"]');
    expect(tiles).toHaveLength(1); // never two wrappers for the same cell
    expect(container.querySelector('[data-moss-places-tile="10,5"] [data-test-fresh]')).not.toBeNull(); // the fresh raster is the one shown
    expect(staleRelease).toHaveBeenCalledTimes(1); // the stale surface is released, not left dangling
    expect(freshRelease).not.toHaveBeenCalled(); // the fresh surface stays live, owned by the layer
  });
});

describe("TileLayer — re-bakes follow the density needed, within the same concurrency limit as loads", () => {
  const TILE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"></svg>';
  const BASE_URL = "/_moss/tiles/";
  const VIEWPORT = { width: 1200, height: 700 };
  const K = 4;
  // A block of cells all on screen at the detail ceiling.
  const CELLS: Array<[number, number]> = [8, 9, 10, 11].flatMap((x) => [4, 5].map((y) => [x, y] as [number, number]));
  const CAMERA = {
    x: (tileCellBounds(9, 5).minX + tileCellBounds(10, 5).maxX) / 2,
    y: (tileCellBounds(9, 5).minY + tileCellBounds(9, 5).maxY) / 2,
    zoom: detailMaxZoom(VIEWPORT),
  };
  /** `unitScale` that makes the layer's density (`unitScale / K * zoom`) equal `density` at CAMERA. */
  const unitScaleFor = (density: number) => (density * K) / CAMERA.zoom;

  function stubFetch(): void {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, text: () => Promise.resolve(TILE_SVG) }) as unknown as Response));
  }
  async function flush(): Promise<void> {
    for (let i = 0; i < 4; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  }
  /** Loads every cell at `density` and lets the layer settle. */
  async function loaded(density: number) {
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: CELLS, k: K, origins: originsFor(CELLS, K, 0.1), ...GRID });
    layer.render(CAMERA, VIEWPORT, unitScaleFor(density), true);
    await flush();
    expect(container.querySelectorAll(".moss-places-tile")).toHaveLength(CELLS.length);
    return layer;
  }

  afterEach(() => {
    vi.unstubAllGlobals();
    rasterizeOrFallbackSpy.mockReset();
  });

  test("no more than the load limit of re-bakes decode at once", async () => {
    stubFetch();
    rasterizeOrFallbackSpy.mockImplementation(async () => ({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} }));
    const layer = await loaded(2);
    rasterizeOrFallbackSpy.mockClear();
    let inFlight = 0;
    let peak = 0;
    const gates: Array<() => void> = [];
    rasterizeOrFallbackSpy.mockImplementation(async () => {
      inFlight++;
      peak = Math.max(peak, inFlight);
      await new Promise<void>((resolve) => gates.push(resolve));
      inFlight--;
      return { el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} };
    });
    layer.render(CAMERA, VIEWPORT, unitScaleFor(10), true);
    await flush();
    expect(peak).toBeGreaterThan(0);
    while (gates.length) {
      gates.shift()!();
      await flush();
    }
    expect(peak).toBeLessThanOrEqual(6);
    expect(rasterizeOrFallbackSpy).toHaveBeenCalledTimes(CELLS.length);
  });

  test("a tile already baked at the size cap is not decoded again when the density grows further", async () => {
    stubFetch();
    rasterizeOrFallbackSpy.mockImplementation(async () => ({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} }));
    const layer = await loaded(500); // the 100px canvas caps at 4096 / 100 = 40.96
    rasterizeOrFallbackSpy.mockClear();
    layer.render(CAMERA, VIEWPORT, unitScaleFor(900), true);
    await flush();
    expect(rasterizeOrFallbackSpy).not.toHaveBeenCalled();
  });

  test("a raster is baked smaller again once the density needed falls well below it", async () => {
    stubFetch();
    rasterizeOrFallbackSpy.mockImplementation(async () => ({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} }));
    const layer = await loaded(20);
    rasterizeOrFallbackSpy.mockClear();
    layer.render(CAMERA, VIEWPORT, unitScaleFor(4), true);
    await flush();
    expect(rasterizeOrFallbackSpy).toHaveBeenCalledTimes(CELLS.length);
    expect(rasterizeOrFallbackSpy.mock.calls[0][2]).toBeCloseTo(400, 0); // 100 canvas px at density 4, dpr 1
  });

  test("a tile that leaves the view and returns at the same density is not decoded again", async () => {
    stubFetch();
    rasterizeOrFallbackSpy.mockImplementation(async () => ({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} }));
    const layer = await loaded(20);
    rasterizeOrFallbackSpy.mockClear();
    layer.render({ ...CAMERA, x: CAMERA.x + 5000 }, VIEWPORT, unitScaleFor(20), true);
    await flush();
    layer.render(CAMERA, VIEWPORT, unitScaleFor(20), true);
    await flush();
    expect(rasterizeOrFallbackSpy).not.toHaveBeenCalled();
  });

  test("the container reports busy until every load and re-bake has finished, and idle after", async () => {
    stubFetch();
    rasterizeOrFallbackSpy.mockImplementation(async () => ({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} }));
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: CELLS, k: K, origins: originsFor(CELLS, K, 0.1), ...GRID });
    layer.render(CAMERA, VIEWPORT, unitScaleFor(2), true);
    expect(container.dataset.mossPlacesTilesState).toBe("busy");
    await flush();
    expect(container.dataset.mossPlacesTilesState).toBe("idle");
    // A gesture frame is never idle: the settled re-bake pass has not run yet.
    layer.render(CAMERA, VIEWPORT, unitScaleFor(10), false);
    expect(container.dataset.mossPlacesTilesState).toBe("busy");
    layer.render(CAMERA, VIEWPORT, unitScaleFor(10), true);
    expect(container.dataset.mossPlacesTilesState).toBe("busy");
    await flush();
    expect(container.dataset.mossPlacesTilesState).toBe("idle");
  });
});
