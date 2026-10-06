/**
 * The world raster's own re-bake: a settle that lands while a bake is still
 * decoding is handed that bake back, so the raster has to re-check its
 * target scale itself once the bake finishes.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { mountPlacesMap } from "../map";

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
