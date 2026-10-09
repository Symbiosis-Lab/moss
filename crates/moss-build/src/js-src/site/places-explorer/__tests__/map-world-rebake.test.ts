/**
 * The world raster's own re-bake: a settle that lands while a bake is still
 * decoding is handed that bake back, so the raster has to re-check its
 * target scale itself once the bake finishes.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { mountPlacesMap } from "../map";
import { TileLayer } from "../tiles";

const { rasterizeSpy } = vi.hoisted(() => ({ rasterizeSpy: vi.fn() }));
vi.mock("../raster", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../raster")>();
  return { ...actual, rasterizeOrFallback: rasterizeSpy };
});

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';

function size(w: number, h: number): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width: w, height: h, top: 0, left: 0, right: w, bottom: h, x: 0, y: 0, toJSON() {},
  } as DOMRect);
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
  history.replaceState(null, "", "/places/");
});

afterEach(() => {
  document.body.innerHTML = "";
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  rasterizeSpy.mockReset();
});

describe("mountPlacesMap — the world raster after a bake that outlasted a resize", () => {
  test.each(["resize", "dpr", "zoom"])("uses physical magnification when %s changes", async (change) => {
    history.replaceState(null, "", `/places/?p=patterson&x=421&y=240&z=${change === "zoom" ? 1 : 8}`);
    vi.stubGlobal("devicePixelRatio", 1);
    rasterizeSpy.mockImplementation(async () => ({ el: document.createElement("canvas"), release: vi.fn() }));
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    size(400, 250);
    const figure = document.createElement("figure");
    document.body.append(figure);
    mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4, tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [], places: [] } as any, lang: "en",
    });
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(1);
    const firstWidth = rasterizeSpy.mock.calls[0][2];
    const firstSurface = await rasterizeSpy.mock.results[0].value;

    if (change === "resize") size(1600, 1000);
    else if (change === "dpr") vi.stubGlobal("devicePixelRatio", 2);
    else {
      const zoom = figure.querySelector<HTMLButtonElement>('[data-control="zoom-in"]')!;
      zoom.click();
      zoom.click();
    }
    onResize();
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(change === "resize" ? 1 : 2);
    expect(firstSurface.release).toHaveBeenCalledTimes(change === "resize" ? 0 : 1);
    if (change === "dpr") expect(rasterizeSpy.mock.calls[1][2]).toBe(firstWidth * 2);
    if (change === "zoom") expect(rasterizeSpy.mock.calls[1][2]).toBeGreaterThan(firstWidth * 1.5);
  });

  test.each(["before", "after"])("accepts a deeper camera %s the initial bake resolves before a same-scale resize", async (timing) => {
    history.replaceState(null, "", "/places/?p=patterson&x=421&y=240&z=1.6");
    vi.stubGlobal("devicePixelRatio", 1);
    vi.spyOn(TileLayer.prototype, "hasVisibleTiles").mockReturnValue(true);
    let resolveFirst!: () => void;
    const surface = () => ({ el: document.createElement("canvas"), release: vi.fn() });
    rasterizeSpy.mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = () => resolve(surface()); }));
    rasterizeSpy.mockImplementation(async () => surface());
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    size(400, 250);
    const figure = document.createElement("figure");
    document.body.append(figure);
    const controller = mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4, tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: {
        works: [{ id: "w", title: "Work", byline: [], authors: [], companions: [], places: ["p"], url: "/work/" }],
        places: [{ id: "p", name: "Place", lat: 10, lng: 10, precision: "city" }],
      } as any, lang: "en",
    })!;
    if (timing === "after") {
      resolveFirst();
      await vi.advanceTimersByTimeAsync(0);
    }
    controller.setScope({ kind: "article", id: "w" });
    if (timing === "before") {
      resolveFirst();
      await vi.advanceTimersByTimeAsync(0);
    }
    // Resize before the debounced bake check: the accepted camera must already be owned.
    size(800, 500);
    onResize();
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(1);
  });

  test("uses the camera's applied viewport while a resize observation is still pending", async () => {
    history.replaceState(null, "", "/places/?p=patterson&x=421&y=240&z=8");
    vi.stubGlobal("devicePixelRatio", 2);
    rasterizeSpy.mockImplementation(async () => ({ el: document.createElement("canvas"), release: vi.fn() }));
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    size(400, 250);
    const figure = document.createElement("figure");
    document.body.append(figure);
    mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4, tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [], places: [] } as any, lang: "en",
    });
    await vi.advanceTimersByTimeAsync(0);
    // WebKit can deliver the bake debounce after layout changes but before ResizeObserver.
    size(1600, 1000);
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(1);
    onResize();
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(1);
  });

  test("restores the world pixel budget when resize crosses back below the regional ceiling", async () => {
    history.replaceState(null, "", "/places/?p=patterson&x=421&y=240&z=8");
    vi.stubGlobal("devicePixelRatio", 1);
    rasterizeSpy.mockImplementation(async () => ({ el: document.createElement("canvas"), release: vi.fn() }));
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    size(400, 250);
    const figure = document.createElement("figure");
    document.body.append(figure);
    mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4, tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [], places: [] } as any, lang: "en",
    });
    await vi.advanceTimersByTimeAsync(500);
    size(3200, 2000);
    onResize();
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(2);
    expect(rasterizeSpy.mock.calls[1][2]).toBeGreaterThan(rasterizeSpy.mock.calls[0][2] * 4);
  });

  test("is re-baked for the larger size once the in-flight bake finishes", async () => {
    let resolveFirst!: () => void;
    const surface = () => ({ el: document.createElementNS("http://www.w3.org/2000/svg", "svg"), release() {} });
    rasterizeSpy.mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = () => resolve(surface()); }));
    rasterizeSpy.mockImplementation(async () => surface());
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    size(400, 250);
    const figure = document.createElement("figure");
    document.body.append(figure);
    mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4, tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [], places: [] } as any, lang: "en",
    });
    expect(rasterizeSpy).toHaveBeenCalledTimes(1); // the first bake, still decoding

    size(1600, 1000);
    onResize();
    await vi.advanceTimersByTimeAsync(500); // the debounce fires and is handed the in-flight bake
    expect(rasterizeSpy).toHaveBeenCalledTimes(1);

    resolveFirst();
    await vi.advanceTimersByTimeAsync(500);
    expect(rasterizeSpy).toHaveBeenCalledTimes(2);
    expect(rasterizeSpy.mock.calls[1][2]).toBeGreaterThan(rasterizeSpy.mock.calls[0][2] * 3);
  });
});
