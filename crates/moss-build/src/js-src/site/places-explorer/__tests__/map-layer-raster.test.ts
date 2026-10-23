/**
 * A mounted map with a historical layer's raster: where it sits among the
 * other layers, and that it is placed and clipped by the same transform as
 * the land and the markers, true-aspect correction included.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { FIT, EDGE_FADE } from "./explorer-fit";
import "./armed-timers";
import { aspectX, openingMaxZoom, screenScale, worldToScreen } from "../camera";
import type { DetailLayer } from "../layers";
import { mountPlacesMap } from "../map";
import { project } from "../projection";

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';
const VIEWPORT = { width: 800, height: 500 };
const PLACES = {
  works: [{ id: "w1", title: "W1", byline: [], authors: [], companions: [], places: ["p1"], date: "2024-01-01", description: "", cover: null, url: "/w1/" }],
  places: [{ id: "p1", name: "P1", lat: 40.752, lng: -73.981, precision: "exact" }],
} as any;
const LAYER: DetailLayer = { name: "atlas", extent: [-74.03, 40.68, -73.9, 40.88], maxZoom: 17, credit: "", tiles: "layers/atlas/index.json" };
const CENTER = project(40.752, -73.981);
const FOOTPRINT = [[CENTER.x - 0.02, CENTER.y - 0.01], [CENTER.x + 0.03, CENTER.y - 0.01], [CENTER.x + 0.03, CENTER.y + 0.02], [CENTER.x - 0.02, CENTER.y - 0.01]];
const INDEX = { min_level: 12, max_level: 17, tile_size: 256, levels: {}, footprints: [{ plate: "p1", polygon: FOOTPRINT }] };

beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width: VIEWPORT.width, height: VIEWPORT.height, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, x: 0, y: 0, toJSON() {},
  } as DOMRect);
  vi.stubGlobal("fetch", vi.fn((url: string) => {
    if (url.endsWith("index.json")) return Promise.resolve({ ok: true, json: async () => INDEX });
    if (url.endsWith("tile-10-13.svg")) return Promise.resolve({ ok: true, text: async () => TILE_SVG });
    return Promise.reject(new Error("no network in tests"));
  }));
});

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  history.replaceState(null, "", "/places/");
});

const TILE_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"><g data-map-layer="rivers"><path d="M0 0L9 9" stroke-width="1"/></g></svg>';

function mount(layers: DetailLayer[], degreesPerPx = 0.0005) {
  const zoom = openingMaxZoom(VIEWPORT, degreesPerPx);
  const camera = { x: CENTER.x, y: CENTER.y, zoom };
  history.replaceState(null, "", `/places/?p=patterson&z=${zoom}&x=${camera.x}&y=${camera.y}`);
  const figure = document.createElement("figure");
  figure.innerHTML = '<p class="moss-place-map-credit"></p>';
  document.body.append(figure);
  mountPlacesMap(figure, {
    worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells: [[10, 13]], tileK: 4,
    tileOrigins: { "10,13": [0, 0] }, tileColumns: 36, tileRows: 18, fit: FIT, edgeFade: EDGE_FADE, places: PLACES, layers, lang: "en",
  })!;
  return { figure, camera, world: figure.querySelector<HTMLElement>(".moss-places-world")! };
}

const flush = async () => {
  for (let i = 0; i < 6; i++) await new Promise((resolve) => setTimeout(resolve, 0));
};

describe("draw order", () => {
  test("the raster sits after the land and before the rivers, and the whole world before labels and markers", () => {
    const { figure, world } = mount([LAYER]);
    const order = [...world.children].map((el) => (el.matches("[data-moss-places-layers]") ? "raster" : el.className));
    const raster = order.indexOf("raster");
    const tiles = order.indexOf("moss-places-tiles");
    const tileRivers = order.lastIndexOf("moss-places-tiles");
    expect(tiles).toBeGreaterThanOrEqual(0);
    expect(raster).toBeGreaterThan(tiles);
    expect(tileRivers).toBeGreaterThan(raster);
    const viewport = figure.querySelector(".moss-places-viewport")!;
    const top = [...viewport.children].map((el) => el.className);
    expect(top.indexOf("moss-places-world")).toBeLessThan(top.indexOf("moss-places-labels"));
    expect(top.indexOf("moss-places-labels")).toBeLessThan(top.indexOf("moss-places-markers"));
  });

  test("a regional tile's rivers draw after the raster, and its land before it", async () => {
    const { world } = mount([LAYER]);
    await flush();
    const [land, , rivers] = [...world.children].filter((el) => el.matches(".moss-places-tiles, [data-moss-places-layers]"));
    const raster = world.querySelector("[data-moss-places-layers]")!;
    expect(land.querySelectorAll('[data-moss-places-tile]')).toHaveLength(1);
    expect(land.querySelector(".moss-places-rivers")).toBeNull();
    expect(rivers.querySelectorAll(".moss-places-rivers")).toHaveLength(1);
    expect(land.compareDocumentPosition(raster) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(raster.compareDocumentPosition(rivers) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  test("a layer with no tile index adds neither the raster nor a second rivers container", () => {
    const { world } = mount([{ ...LAYER, tiles: null }]);
    expect(world.querySelector("[data-moss-places-layers]")).toBeNull();
    expect(world.querySelectorAll(".moss-places-tiles")).toHaveLength(1);
  });

  test("with no layers at all the world is as it was", () => {
    const { world } = mount([]);
    expect(world.querySelector("[data-moss-places-layers]")).toBeNull();
    expect(world.querySelectorAll(".moss-places-tiles")).toHaveLength(1);
  });
});

describe("placement under true aspect", () => {
  /** Where the world element's transform puts world point `p`, read back from its inline style: the element's top-left sits at the viewport's centre, and its local px are world units times `unit`. */
  function screenOf(world: HTMLElement, p: { x: number; y: number }) {
    const m = /translate\(([-\d.e]+)px, ([-\d.e]+)px\) scale\(([-\d.e]+), ([-\d.e]+)\)/.exec(world.style.transform)!;
    const [tx, ty, sx, sy] = m.slice(1).map(Number);
    const unit = screenScale({ x: 0, y: 0, zoom: 1 }, VIEWPORT);
    const svgUnit = Number(/scale\(([-\d.e]+)\)/.exec(world.querySelector("[data-moss-places-layers] > g")!.getAttribute("transform")!)![1]);
    expect(svgUnit).toBeCloseTo(unit, 9);
    return { x: VIEWPORT.width / 2 + tx + p.x * svgUnit * sx, y: VIEWPORT.height / 2 + ty + p.y * svgUnit * sy };
  }

  test("a footprint vertex lands where the markers' projection puts that world point", async () => {
    const { world, camera } = mount([LAYER]);
    await flush();
    expect(aspectX(camera, VIEWPORT)).toBeLessThan(0.95);
    const svg = world.querySelector<SVGSVGElement>("[data-moss-places-layers]")!;
    const vertices = [...svg.querySelectorAll("clipPath polygon")][0].getAttribute("points")!.split(" ").map((pair) => pair.split(",").map(Number));
    expect(vertices).toHaveLength(FOOTPRINT.length);
    for (const [x, y] of vertices) {
      const expected = worldToScreen({ x, y }, camera, VIEWPORT);
      const actual = screenOf(world, { x, y });
      expect(actual.x).toBeCloseTo(expected.x, 3);
      expect(actual.y).toBeCloseTo(expected.y, 3);
    }
  });

  test("the world is anchored at its own top-left, so the browser's rounding of its size is never multiplied by the zoom", () => {
    const { world } = mount([LAYER]);
    expect(world.style.transformOrigin).toMatch(/^0(px)? 0(px)?$/);
    expect(world.style.transform).not.toContain("calc(");
  });

  test("the raster is inside the scaled world while markers and labels are outside it", async () => {
    const { figure, world } = mount([LAYER]);
    await flush();
    expect(world.querySelector("[data-moss-places-layers]")).not.toBeNull();
    expect(world.contains(figure.querySelector(".moss-places-markers"))).toBe(false);
    expect(world.contains(figure.querySelector(".moss-places-labels"))).toBe(false);
  });
});

describe("credit", () => {
  const CREDITED: DetailLayer = { ...LAYER, credit: "Atlas of the City, 1930" };
  const credit = (figure: HTMLElement) => figure.querySelector(".moss-place-map-credit")!.textContent;

  test("is empty while the extent is in view but no level is drawn, and appears once one is", async () => {
    const far = mount([CREDITED], 0.05);
    await flush();
    expect(credit(far.figure)).toBe("");
    document.body.innerHTML = "";
    const near = mount([CREDITED]);
    expect(credit(near.figure)).toBe("");
    await flush();
    expect(credit(near.figure)).toBe("Atlas of the City, 1930");
  });
});
