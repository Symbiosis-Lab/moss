/**
 * Tests for tiles.ts's pure geometry — the DOM/fetch half (`TileLayer`) is
 * covered by the render gates for a real, composited viewport (the same
 * split `markers.ts`'s own tests draw for `MarkerLayer`), except for
 * `TileLayer`'s own neighbour-load-state bookkeeping below: whether a
 * neighbour is loaded, failed, or still in flight only exists on a real
 * `TileLayer` instance (`tileClipInset`/`tileEdgeMask` themselves are pure
 * and take whatever cell list they're handed), so that part is exercised
 * here instead, against a mocked, controllable `fetch`.
 */
import { afterEach, describe, test, expect, vi } from "vitest";
import { detailMaxZoom, MIN_ZOOM } from "../camera";
import { WORLD_WIDTH, WORLD_HEIGHT } from "../projection";
import { TileLayer, tileCellBounds, tileClipInset, tileEdgeMask, tileFadeOpacity, tileOverlayTransform, tilesForView } from "../tiles";

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
  // A tile's own native SVG size, as `explorer::emit_tile_svg` actually
  // emits it (`Projection::canvas_size`, backed by
  // `PattersonProjection::for_tile`): its cell's world-unit rectangle,
  // padded by `bleed` on every edge, times `k`. Re-derived here from
  // `tileCellBounds` rather than imported, so this test can check the
  // geometric CONTRACT (a rendered-and-scaled tile covers its own cell plus
  // a deliberate overlap margin) rather than restate the production
  // formula under a different name.
  function nativeTileSize(x: number, y: number, k: number, bleed: number): { width: number; height: number } {
    const bounds = tileCellBounds(x, y);
    return { width: (bounds.maxX - bounds.minX + 2 * bleed) * k, height: (bounds.maxY - bounds.minY + 2 * bleed) * k };
  }

  test("maps a cell to its own rectangle padded by bleed, at any unitScale/k", () => {
    const unitScale = 2.3;
    const k = 4;
    const bleed = 0.1;
    for (const [x, y] of [[10, 5], [0, 0], [35, 17], [20, 14]] as const) {
      const bounds = tileCellBounds(x, y);
      const { width: nativeWidth, height: nativeHeight } = nativeTileSize(x, y, k, bleed);
      const { translateX, translateY, scale } = tileOverlayTransform(x, y, unitScale, k, bleed);

      // Top-left corner lands bleed world units outside the cell's own
      // origin, not exactly on it.
      expect(translateX).toBeCloseTo((bounds.minX - bleed) * unitScale);
      expect(translateY).toBeCloseTo((bounds.minY - bleed) * unitScale);
      // Rendered (native size * scale) equals the cell's own rectangle
      // padded by bleed on every edge, in the SAME unitScale px space the
      // world layer's internal layout uses — not the tile's own larger,
      // k-magnified canvas.
      expect(nativeWidth * scale).toBeCloseTo((bounds.maxX - bounds.minX + 2 * bleed) * unitScale);
      expect(nativeHeight * scale).toBeCloseTo((bounds.maxY - bounds.minY + 2 * bleed) * unitScale);
    }
  });

  test("adjacent tiles overlap by exactly 2 * bleed at their shared edge, at any k", () => {
    const unitScale = 1.7;
    const bleed = 0.1;
    for (const k of [1, 4, 10]) {
      const a = tileOverlayTransform(10, 5, unitScale, k, bleed);
      const { width: nativeWidthA } = nativeTileSize(10, 5, k, bleed);
      const b = tileOverlayTransform(11, 5, unitScale, k, bleed);
      const aRightEdge = a.translateX + nativeWidthA * a.scale;
      // A's own right edge lands PAST b's own left edge by 2 * bleed world
      // units (in unitScale px) — an overlap, not a meet — so a sub-pixel
      // rounding disagreement between the two leaves overlap, not a gap.
      expect(aRightEdge - b.translateX).toBeCloseTo(2 * bleed * unitScale);
      expect(aRightEdge).toBeGreaterThan(b.translateX);
    }
  });
});

describe("tileClipInset", () => {
  const cells: Array<[number, number]> = [
    [10, 5],
    [11, 5], // east neighbour of (10, 5)
    [10, 6], // north neighbour of (10, 5)
  ];

  test("a tile clips the one edge whose lower-index neighbour is present, leaves the other alone", () => {
    // (11, 5)'s own west neighbour is (10, 5): left-clipped. Its own south
    // neighbour, (11, 4), is absent: not bottom-clipped.
    expect(tileClipInset(11, 5, cells, 4, 0.2)).toEqual({ bottom: 0, left: 0.8 });
    // (10, 6)'s own south neighbour is (10, 5): bottom-clipped. Its own
    // west neighbour, (9, 6), is absent: not left-clipped.
    expect(tileClipInset(10, 6, cells, 4, 0.2)).toEqual({ bottom: 0.8, left: 0 });
  });

  test("a tile with neither neighbour present clips nothing", () => {
    expect(tileClipInset(10, 5, cells, 4, 0.2)).toEqual({ bottom: 0, left: 0 });
  });

  test("the inset scales with k and bleed, not a fixed pixel amount", () => {
    expect(tileClipInset(11, 5, cells, 1, 0.2)).toEqual({ bottom: 0, left: 0.2 });
    expect(tileClipInset(11, 5, cells, 4, 0.5)).toEqual({ bottom: 0, left: 2 });
  });

  test("of two adjacent tiles, exactly one ever keeps its bleed toward the other — the lower-index one, bleeding toward the higher", () => {
    // (10,5)/(11,5): east-west pair. (10,5) is lower-index on x; it must
    // keep bleeding east (no left-clip of its own — left-clip only ever
    // fires on ITS OWN west side), while (11,5) must clip its own west.
    expect(tileClipInset(10, 5, cells, 4, 0.2).left).toBe(0);
    expect(tileClipInset(11, 5, cells, 4, 0.2).left).toBeGreaterThan(0);
    // (10,5)/(10,6): south-north pair. (10,5) is lower-index on y; it keeps
    // bleeding north (no bottom-clip of its own), while (10,6) clips its
    // own south.
    expect(tileClipInset(10, 5, cells, 4, 0.2).bottom).toBe(0);
    expect(tileClipInset(10, 6, cells, 4, 0.2).bottom).toBeGreaterThan(0);
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

  test("an edge a tile SHARES with a real neighbour never appears in its own mask — `tileClipInset` and `tileEdgeMask` never both touch the same edge", () => {
    for (const [x, y] of cells) {
      const clip = tileClipInset(x, y, cells, 4, 0.2);
      const mask = tileEdgeMask(x, y, cells, 4) ?? "";
      if (clip.left > 0) expect(mask).not.toContain("to right"); // west fade would duplicate a west-clip
      if (clip.bottom > 0) expect(mask).not.toContain("to top"); // south fade would duplicate a south-clip
    }
  });
});

/**
 * `TileLayer`'s own clip/mask bookkeeping: whether a neighbour counts as
 * present has to come from what's actually loaded and in the DOM, never
 * from the static manifest alone — a neighbour still in flight, or one
 * whose fetch permanently failed (failures are cached and never retried,
 * this file's own `TileLayer` doc), has not drawn the shared strip either,
 * so clipping toward it as though it had would leave that strip undrawn by
 * BOTH tiles.
 */
describe("TileLayer — neighbour load state, not the manifest, drives clip and edge-fade", () => {
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

  test("a tile keeps its full bleed toward a west neighbour still in flight, and clips once that neighbour loads", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[9, 5], [10, 5]], k: K, bleed: BLEED });

    layer.render(CAMERA, VIEWPORT, 1);
    controllers.get(`${BASE_URL}tile-10-5.svg`)!.resolveOk(TILE_SVG);
    await flush();
    const main = container.querySelector('[data-moss-places-tile="10,5"]') as unknown as HTMLElement;
    expect(main).not.toBeNull();
    // (9,5) is in the manifest but its own fetch hasn't resolved yet.
    expect(main.style.clipPath).toBe("");

    controllers.get(`${BASE_URL}tile-9-5.svg`)!.resolveOk(TILE_SVG);
    await flush();
    // Now that (9,5) is itself loaded and in the DOM, (10,5) — the
    // higher-index tile of the pair — clips its own west bleed away.
    expect(main.style.clipPath).toBe(`inset(0 0 0px ${BLEED * K}px)`);
  });

  test("a tile stays unclipped toward a west neighbour whose fetch failed, and fades that edge instead", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[9, 5], [10, 5]], k: K, bleed: BLEED });

    layer.render(CAMERA, VIEWPORT, 1);
    controllers.get(`${BASE_URL}tile-10-5.svg`)!.resolveOk(TILE_SVG);
    controllers.get(`${BASE_URL}tile-9-5.svg`)!.reject(new Error("404"));
    await flush();

    const main = container.querySelector('[data-moss-places-tile="10,5"]') as unknown as HTMLElement;
    // A permanently-failed neighbour never draws the shared strip either —
    // clipping here would draw NEITHER side of it.
    expect(main.style.clipPath).toBe("");
    // The manifest named a west neighbour, but it will never arrive, so
    // that edge now reads as this layer's own outer edge and fades toward
    // the world layer instead of ending in a hard, undrawn line.
    expect(main.style.maskImage).toContain("to right");
  });

  test("a pan — render() called again with only the camera moved — never reassigns an already-loaded tile's clip-path or mask-image", async () => {
    const controllers = stubControllableFetch();
    const container = document.createElement("div");
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[9, 5], [10, 5]], k: K, bleed: BLEED });

    layer.render(CAMERA, VIEWPORT, 1);
    controllers.get(`${BASE_URL}tile-10-5.svg`)!.resolveOk(TILE_SVG);
    controllers.get(`${BASE_URL}tile-9-5.svg`)!.resolveOk(TILE_SVG);
    await flush();

    const main = container.querySelector('[data-moss-places-tile="10,5"]') as unknown as HTMLElement;
    let clipPathSets = 0;
    let maskImageSets = 0;
    let clipPathValue = main.style.clipPath;
    let maskImageValue = main.style.maskImage;
    // Shadow the two properties on THIS style instance only — an own
    // property always wins the lookup over the inherited accessor, so this
    // counts every assignment `applyClipAndMask` (or a reverted `position`)
    // makes, without needing to know how the DOM implementation itself
    // wires up `CSSStyleDeclaration`.
    Object.defineProperty(main.style, "clipPath", {
      configurable: true,
      get: () => clipPathValue,
      set: (v: string) => { clipPathSets++; clipPathValue = v; },
    });
    Object.defineProperty(main.style, "maskImage", {
      configurable: true,
      get: () => maskImageValue,
      set: (v: string) => { maskImageSets++; maskImageValue = v; },
    });

    for (let i = 0; i < 5; i++) layer.render({ ...CAMERA, x: CAMERA.x + i }, VIEWPORT, 1);

    expect(clipPathSets).toBe(0);
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
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: [[10, 5]], k: K, bleed: BLEED });
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
    const layer = new TileLayer(container, { tilesBaseUrl: BASE_URL, availableTiles: CELLS, k: K, bleed: 0.1 });
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
});
