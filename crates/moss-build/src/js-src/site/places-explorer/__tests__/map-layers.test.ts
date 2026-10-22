/**
 * A mounted map with detail layers: the zoom ceiling, the true-aspect
 * drawing, the credit slot and the markers that must stay unscaled.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { FIT, EDGE_FADE } from "./explorer-fit";
import "./armed-timers";
import { aspectX, openingMaxZoom, screenScale, screenToWorld, worldToScreen } from "../camera";
import { degreesPerPxAtZoom, type DetailLayer } from "../layers";
import { mountPlacesMap } from "../map";
import { project, unproject } from "../projection";
import { readUrlState } from "../state";

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';
const VIEWPORT = { width: 800, height: 500 };
const PLACES = {
  works: [{ id: "w1", title: "W1", byline: [], authors: [], companions: [], places: ["p1"], date: "2024-01-01", description: "", cover: null, url: "/w1/" }],
  places: [{ id: "p1", name: "P1", lat: 40.752, lng: -73.981, precision: "exact" }],
} as any;
const LAYER: DetailLayer = { name: "atlas", extent: [-74.03, 40.68, -73.9, 40.88], maxZoom: 17, credit: "Atlas of the City, 1930", tiles: null };

beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width: VIEWPORT.width, height: VIEWPORT.height, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, x: 0, y: 0, toJSON() {},
  } as DOMRect);
  vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
});

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  history.replaceState(null, "", "/places/");
});

/** A camera `degreesPerPx` of longitude per CSS px deep, centred on `lat`/`lng`. */
function cityCamera(lat: number, lng: number, degreesPerPx: number) {
  const c = project(lat, lng);
  return { x: c.x, y: c.y, zoom: openingMaxZoom(VIEWPORT, degreesPerPx) };
}

function mount(camera: { x: number; y: number; zoom: number }, layers?: DetailLayer[], withCredit = true) {
  history.replaceState(null, "", `/places/?p=patterson&z=${camera.zoom}&x=${camera.x}&y=${camera.y}`);
  const figure = document.createElement("figure");
  if (withCredit) figure.innerHTML = '<p class="moss-place-map-credit"></p>';
  document.body.append(figure);
  const controller = mountPlacesMap(figure, {
    worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4,
    tileOrigins: {}, tileColumns: 36, tileRows: 18, fit: FIT, edgeFade: EDGE_FADE, places: PLACES, layers, lang: "en",
  })!;
  return { figure, controller };
}

const credit = (figure: HTMLElement) => figure.querySelector(".moss-place-map-credit")!.textContent;
const zoomIn = (figure: HTMLElement) => figure.querySelector<HTMLElement>('[data-control="zoom-in"]')!;

describe("zoom ceiling", () => {
  // 0.0005 degrees per px is ten times deeper than the world ceiling (0.005).
  const deep = cityCamera(40.75, -73.98, 0.0005);

  test("inside a layer's extent the zoom-in control still works past the world ceiling", () => {
    const { figure } = mount(deep, [LAYER]);
    expect(zoomIn(figure).disabled).toBe(false);
  });

  test("without layers the same camera is at the ceiling", () => {
    const { figure } = mount(deep);
    expect(zoomIn(figure).disabled).toBe(true);
  });

  test("outside the extent a layer grants nothing", () => {
    const { figure } = mount(cityCamera(35, 139, 0.0005), [LAYER]);
    expect(zoomIn(figure).disabled).toBe(true);
  });

  test("zoom stops at the layer's max_zoom", () => {
    const atMax = cityCamera(40.75, -73.98, degreesPerPxAtZoom(17));
    const { figure } = mount(atMax, [LAYER]);
    expect(zoomIn(figure).disabled).toBe(true);
  });

  test("leaving the extent eases back to the ceiling in steps", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
    vi.stubGlobal("cancelAnimationFrame", () => {});
    const { figure } = mount(cityCamera(35, 139, 0.0005), [LAYER]);
    const worldScale = () => Number(/scale\(([^,]+),/.exec(figure.querySelector<HTMLElement>(".moss-places-world")!.style.transform)![1]);
    const zooms = [worldScale()];
    for (let i = 0; i < 80 && frames.length; i++) {
      frames.shift()!(0);
      zooms.push(worldScale());
    }
    expect(zooms[1]).toBeLessThan(zooms[0]);
    expect(zooms[1]).toBeGreaterThan(zooms[zooms.length - 1]);
    expect(zoomIn(figure).disabled).toBe(true);
    expect(frames).toHaveLength(0);
  });
});

describe("true aspect", () => {
  const camera = cityCamera(40.752, -73.981, 0.0005);

  test("the world layer is stretched by the factor, markers are placed by it but never transformed", () => {
    const { figure } = mount(camera, [LAYER]);
    const k = aspectX(camera, VIEWPORT);
    expect(k).toBeLessThan(0.95);
    const world = figure.querySelector<HTMLElement>(".moss-places-world")!;
    expect(world.style.transform).toContain(`scale(${camera.zoom * k}, ${camera.zoom})`);

    const marker = figure.querySelector<HTMLElement>(".moss-places-marker")!;
    const expected = worldToScreen(project(40.752, -73.981), camera, VIEWPORT);
    expect(parseFloat(marker.style.left)).toBeCloseTo(expected.x, 3);
    expect(parseFloat(marker.style.top)).toBeCloseTo(expected.y, 3);
    // Not a descendant of the scaled world, and carrying no transform of its own.
    expect(world.contains(marker)).toBe(false);
    expect(marker.style.transform).toBe("");
    expect(marker.style.width).toBe("");
    const labels = figure.querySelector<HTMLElement>(".moss-places-labels")!;
    expect(world.contains(labels)).toBe(false);
    expect(labels.style.transform).toBe("");
  });

  test("without correction (a world-scale camera) the world scales uniformly", () => {
    const wide = { ...project(40.752, -73.981), zoom: 2 };
    const { figure } = mount(wide, [LAYER]);
    expect(figure.querySelector<HTMLElement>(".moss-places-world")!.style.transform).toContain("scale(2, 2)");
  });

  test("a marker's displayed position hit-tests back to its place", () => {
    const { figure } = mount(camera, [LAYER]);
    const marker = figure.querySelector<HTMLElement>(".moss-places-marker")!;
    const screen = { x: parseFloat(marker.style.left), y: parseFloat(marker.style.top) };
    const back = unproject(screenToWorld(screen, camera, VIEWPORT).x, screenToWorld(screen, camera, VIEWPORT).y);
    expect(back.lat).toBeCloseTo(40.752, 5);
    expect(back.lng).toBeCloseTo(-73.981, 5);
    expect(screenScale(camera, VIEWPORT)).toBeGreaterThan(0);
  });

  test("the marker is still clickable and selects its work", () => {
    vi.stubGlobal("CSS", { escape: (value: string) => value });
    Element.prototype.scrollIntoView = vi.fn();
    const { figure } = mount(camera, [LAYER]);
    figure.querySelector<HTMLElement>(".moss-places-marker")!.click();
    expect(new URLSearchParams(location.search).get("article")).toBe("w1");
  });

  test("the saved camera in the URL stays the plain camera", () => {
    mount(camera, [LAYER]);
    const saved = readUrlState().camera!;
    expect(saved.zoom).toBeCloseTo(camera.zoom, 2);
  });
});

describe("credit slot", () => {
  test("shows the credit of a layer in view, clears it elsewhere, and survives the mount", () => {
    const inView = mount(cityCamera(40.75, -73.98, 0.0005), [LAYER]);
    expect(credit(inView.figure)).toBe("Atlas of the City, 1930");
    document.body.innerHTML = "";
    const away = mount(cityCamera(35, 139, 0.0005), [LAYER]);
    expect(credit(away.figure)).toBe("");
  });

  test("two credited layers in view are joined", () => {
    const { figure } = mount(cityCamera(40.75, -73.98, 0.0005), [LAYER, { ...LAYER, name: "streets", credit: "Street data" }]);
    expect(credit(figure)).toBe("Atlas of the City, 1930 · Street data");
  });

  test("an uncredited layer leaves the slot empty", () => {
    const { figure } = mount(cityCamera(40.75, -73.98, 0.0005), [{ ...LAYER, credit: "" }]);
    expect(credit(figure)).toBe("");
  });

  test("credit text is text, never markup", () => {
    const { figure } = mount(cityCamera(40.75, -73.98, 0.0005), [{ ...LAYER, credit: "<img src=x onerror=1>" }]);
    expect(figure.querySelector(".moss-place-map-credit img")).toBeNull();
    expect(credit(figure)).toBe("<img src=x onerror=1>");
  });

  test("with no layers the slot is left as the build wrote it and the map is unchanged", () => {
    history.replaceState(null, "", "/places/");
    const figure = document.createElement("figure");
    figure.innerHTML = '<p class="moss-place-map-credit">static</p>';
    document.body.append(figure);
    mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4,
      tileOrigins: {}, tileColumns: 36, tileRows: 18, fit: FIT, edgeFade: EDGE_FADE, places: PLACES, lang: "en",
    });
    expect(credit(figure)).toBe("static");
    expect(figure.querySelector<HTMLElement>(".moss-places-world")!.style.transform).toMatch(/scale\(([\d.]+), \1\)/);
  });
});

describe("opening frame", () => {
  const spots = [[40.70, -74.01], [40.85, -73.85], [40.76, -73.97], [40.74, -73.99]];
  const CITY = {
    works: spots.map((_, i) => ({ id: `w${i}`, title: `W${i}`, byline: [], authors: [], companions: [], places: [`p${i}`], date: "2024-01-01", description: "", cover: null, url: `/w${i}/` })),
    places: spots.map(([lat, lng], i) => ({ id: `p${i}`, name: `P${i}`, lat, lng, precision: "exact" })),
  } as any;

  function open(layers?: DetailLayer[]) {
    history.replaceState(null, "", "/places/");
    const figure = document.createElement("figure");
    document.body.append(figure);
    mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [], tileK: 4,
      tileOrigins: {}, tileColumns: 36, tileRows: 18, fit: FIT, edgeFade: EDGE_FADE, places: CITY, layers, lang: "en",
    });
    return { figure, zoom: readUrlState().camera!.zoom };
  }

  test("places inside a layer's extent open fitted to themselves as separate markers", () => {
    const { figure, zoom } = open([{ ...LAYER, extent: [-74.05, 40.65, -73.8, 40.9] }]);
    expect(zoom).toBeGreaterThan(openingMaxZoom(VIEWPORT, FIT.openingDegreesPerPx) * 2);
    expect(figure.querySelectorAll(".moss-places-marker")).toHaveLength(4);
  });

  test("the same places with no layers open at the opening cap", () => {
    const { zoom } = open();
    expect(zoom).toBeLessThanOrEqual(openingMaxZoom(VIEWPORT, FIT.openingDegreesPerPx) * (1 + 1e-6));
  });

  test("places only partly inside an extent still open fitted to themselves", () => {
    const { zoom } = open([{ ...LAYER, extent: [-74.0, 40.72, -73.9, 40.8] }]);
    expect(zoom).toBeGreaterThan(openingMaxZoom(VIEWPORT, FIT.openingDegreesPerPx) * 2);
  });

  test("places whose bounds miss every extent keep the cap", () => {
    const { zoom } = open([{ ...LAYER, extent: [-73.5, 41.2, -73.4, 41.3] }]);
    expect(zoom).toBeLessThanOrEqual(openingMaxZoom(VIEWPORT, FIT.openingDegreesPerPx) * (1 + 1e-6));
  });
});

describe("pan and tile visibility under the correction", () => {
  test("an arrow key pans the same screen distance east as south", () => {
    const camera = cityCamera(40.752, -73.981, 0.0005);
    const { figure } = mount(camera, [LAYER]);
    const marker = () => figure.querySelector<HTMLElement>(".moss-places-marker")!;
    const press = (key: string) => figure.querySelector<HTMLElement>(".moss-places-viewport")!.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    const x0 = parseFloat(marker().style.left);
    const y0 = parseFloat(marker().style.top);
    press("ArrowRight");
    const dx = x0 - parseFloat(marker().style.left);
    press("ArrowDown");
    const dy = y0 - parseFloat(marker().style.top);
    expect(dx).toBeGreaterThan(0);
    expect(dx).toBeCloseTo(dy, 3);
  });

  test("a tile just beyond the uncorrected edge is wanted once x is compressed", async () => {
    const { tilesForView, tileCellBounds } = await import("../tiles");
    const east = tileCellBounds(11, 12);
    const base = { x: 0, y: project(35, 0).y, zoom: openingMaxZoom(VIEWPORT, 0.0005) };
    const k = aspectX(base, VIEWPORT);
    expect(k).toBeLessThan(1);
    const reach = (VIEWPORT.width / 2 + 96) / screenScale(base, VIEWPORT);
    // Past the uncorrected reach, inside the corrected one.
    const camera = { ...base, x: east.minX - reach * ((1 + 1 / k) / 2) };
    expect(tilesForView([[11, 12]], camera, VIEWPORT)).toHaveLength(1);
  });
});
