/**
 * Tests for map.ts's own camera clamp — the one shared re-clamp every
 * camera change passes through in `applyCamera`, covering what a module
 * test of `camera.ts`'s pure `clampCamera` cannot: the clamp's ceiling
 * argument is recomputed from the LIVE camera on every call
 * (`currentMaxZoom`, which reads `tileLayer.hasVisibleTiles`), so only a
 * real mount can show what happens when that ceiling changes out from
 * under an in-flight gesture.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { coverCamera, detailMaxZoom, resizeCamera, screenScale, tileDetailMaxZoom, worldToScreen } from "../camera";
import { mountPlacesMap } from "../map";
import { project } from "../projection";
import { readUrlState } from "../state";
import type { LabelsData, Place, Work } from "../types";

const raster = vi.hoisted(() => ({
  rasterizeOrFallback: vi.fn(),
  restore: () => {},
}));
vi.mock("../raster", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../raster")>();
  raster.rasterizeOrFallback.mockImplementation(actual.rasterizeOrFallback);
  raster.restore = () => raster.rasterizeOrFallback.mockImplementation(actual.rasterizeOrFallback);
  return { ...actual, rasterizeOrFallback: raster.rasterizeOrFallback };
});

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';
const VIEWPORT = { width: 800, height: 500 };
// Lng 20..30, lat 10..20 — a tile cell nowhere near the world-centre corner
// the initial cover camera starts at, so zooming toward its centre is a
// real camera move, not a zoom-in-place.
const TILE_CELL: [number, number] = [20, 10];

beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width: VIEWPORT.width, height: VIEWPORT.height, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, x: 0, y: 0, toJSON() {},
  } as DOMRect);
  // No real tile SVG is needed — only `hasVisibleTiles` (pure geometry) is
  // under test here, never a loaded tile element — so a rejected fetch is
  // fine; `TileLayer.render` already catches it into "failed".
  vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
  history.replaceState(null, "", "/places/");
});

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  raster.restore();
  history.replaceState(null, "", "/places/");
});

/** Dispatch a ctrl-wheel zoom-in at a FIXED screen point, so repeated calls keep converging the camera toward the same world point under it (the real gesture a reader holding the cursor still over a cell while scrolling produces). */
function wheelZoomIn(viewportEl: HTMLElement, clientX: number, clientY: number, times: number): void {
  for (let i = 0; i < times; i++) {
    viewportEl.dispatchEvent(new WheelEvent("wheel", { ctrlKey: true, deltaY: -100, clientX, clientY, bubbles: true, cancelable: true }));
  }
}

function keyboardPan(viewportEl: HTMLElement, key: string, times: number): void {
  for (let i = 0; i < times; i++) {
    viewportEl.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey: true, bubbles: true, cancelable: true }));
  }
}

describe("mountPlacesMap — embed seams", () => {
  const PLACES = {
    works: [
      { id: "w1", title: "W1", byline: [], authors: [], companions: [], places: ["p1"], date: "2024-01-01", description: "", cover: null, url: "/w1/" },
      { id: "w2", title: "W2", byline: [], authors: [], companions: [], places: ["p1"], date: "2024-01-02", description: "", cover: null, url: "/w2/" },
    ],
    places: [{ id: "p1", name: "P1", lat: 10, lng: 10, precision: "city", parent: null }],
  } as any;

  function mount(places: any = PLACES) {
    const figure = document.createElement("figure");
    document.body.append(figure);
    const controller = mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG,
      tilesBaseUrl: "/_moss/map.abc/",
      tileCells: [],
      tileK: 4,
      tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places,
      lang: "en",
    })!;
    return { figure, controller };
  }

  test("world-only initial paint waits for a current-size bake after a viewport resize", async () => {
    let viewport = { ...VIEWPORT };
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(() => ({
      width: viewport.width, height: viewport.height, top: 0, left: 0,
      right: viewport.width, bottom: viewport.height, x: 0, y: 0, toJSON() {},
    } as DOMRect));
    const pending: Array<(surface: { el: HTMLCanvasElement | SVGSVGElement; release: () => void }) => void> = [];
    raster.rasterizeOrFallback.mockImplementation(() => new Promise((resolve) => pending.push(resolve)));
    const { controller } = mount();
    let ready = false;
    const waiting = controller.waitForInitialPaint().then((result) => { ready = result; });
    await vi.waitFor(() => expect(pending).toHaveLength(1));

    viewport = { width: 1600, height: 500 };
    pending[0]!({ el: document.createElement("canvas"), release: vi.fn() });
    await vi.waitFor(() => expect(pending).toHaveLength(2));
    expect(ready).toBe(false);
    pending[1]!({ el: document.createElement("canvas"), release: vi.fn() });
    await waiting;
    expect(ready).toBe(true);
  });

  test("camera paint coalesces while wheel input keeps the final camera", () => {
    vi.useFakeTimers();
    try {
      const frames: FrameRequestCallback[] = [];
      const request = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
        frames.push(callback);
        return frames.length;
      });
      const { figure, controller } = mount();
      request.mockClear();
      frames.length = 0;
      const world = figure.querySelector<HTMLElement>(".moss-places-world")!;
      const before = world.style.transform;
      const beforeZoom = readUrlState().camera!.zoom;
      for (let i = 0; i < 10; i++) {
        controller.viewportEl.dispatchEvent(new WheelEvent("wheel", { deltaY: -10, clientX: 400, clientY: 250, cancelable: true }));
      }
      expect(request).toHaveBeenCalledTimes(1);
      expect(world.style.transform).toBe(before);
      frames[0](16);
      expect(world.style.transform).not.toBe(before);
      expect(readUrlState().camera!.zoom).toBe(beforeZoom);
      vi.advanceTimersByTime(120);
      expect(readUrlState().camera!.zoom).toBeCloseTo(beforeZoom * 1.08, 2);
      expect(world.hasAttribute("data-gesture")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  test("pointerup paints the final pan immediately and cancels its queued frame", () => {
    vi.spyOn(window, "requestAnimationFrame").mockReturnValue(42);
    const cancel = vi.spyOn(window, "cancelAnimationFrame");
    const { figure, controller } = mount();
    const viewport = controller.viewportEl;
    viewport.setPointerCapture = vi.fn();
    const world = figure.querySelector<HTMLElement>(".moss-places-world")!;
    const before = world.style.transform;
    const send = (type: string, clientX: number) => {
      const event = new MouseEvent(type, { clientX, clientY: 250, button: 0, bubbles: true });
      Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: "mouse" } });
      viewport.dispatchEvent(event);
    };
    send("pointerdown", 400);
    send("pointermove", 460);
    expect(world.style.transform).toBe(before);
    send("pointerup", 460);
    expect(world.style.transform).not.toBe(before);
    expect(cancel).toHaveBeenCalledWith(42);
    expect(world.hasAttribute("data-gesture")).toBe(false);
  });

  test("setCurrentArticle drops that work's own card from the row without touching its marker selection", () => {
    history.replaceState(null, "", "/places/?article=w1");
    const { figure, controller } = mount();
    controller.setCurrentArticle("w1");
    const cardIds = [...figure.querySelectorAll("[data-work-id]")].map((el) => el.getAttribute("data-work-id"));
    expect(cardIds).not.toContain("w1");
    expect(cardIds).toContain("w2");
    const marker = figure.querySelector('[data-selected="true"]');
    expect(marker).not.toBeNull();
  });

  test("setCurrentArticle(null) restores the work's card", () => {
    history.replaceState(null, "", "/places/?article=w1");
    const { figure, controller } = mount();
    controller.setCurrentArticle("w1");
    controller.setCurrentArticle(null);
    const cardIds = [...figure.querySelectorAll("[data-work-id]")].map((el) => el.getAttribute("data-work-id"));
    expect(cardIds).toContain("w1");
  });

  test("the current article stays off the row after widening scope — in both modes", () => {
    history.replaceState(null, "", "/places/?article=w1&embed=1");
    const { figure, controller } = mount();
    controller.setCurrentArticle("w1");
    controller.setScope({ kind: "all" });
    const cardIds = [...figure.querySelectorAll("[data-work-id]")].map((el) => el.getAttribute("data-work-id"));
    expect(cardIds).not.toContain("w1");
  });

  test("an article mounted into a 0x0 frame is framed on the work once the frame gets a real size", () => {
    history.replaceState(null, "", "/places/?article=w1");
    const rect = (w: number, h: number) =>
      vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
        width: w, height: h, top: 0, left: 0, right: w, bottom: h, x: 0, y: 0, toJSON() {},
      } as DOMRect);
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    rect(0, 0);
    mount();
    rect(346, 231);
    onResize();
    const camera = readUrlState().camera!;
    expect(camera.zoom).toBeGreaterThan(coverCamera([], { width: 346, height: 231 }).zoom * 3);
  });

  test("leaving All articles re-fits the article to the frame it is resized to afterwards, not the fullscreen scale", () => {
    history.replaceState(null, "", "/places/?article=w1&embed=1");
    const size = (w: number, h: number) =>
      vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
        width: w, height: h, top: 0, left: 0, right: w, bottom: h, x: 0, y: 0, toJSON() {},
      } as DOMRect);
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    // Two places far enough apart that the fit is bound by the frame, not by the zoom ceiling.
    const twoPlaces = {
      works: [{ ...PLACES.works[0], places: ["p1", "p2"] }, PLACES.works[1]],
      places: [...PLACES.places, { id: "p2", name: "P2", lat: 40, lng: 60, precision: "city", parent: null }],
    };
    size(1440, 900);
    const { controller } = mount(twoPlaces);
    controller.setScope({ kind: "article", id: "w1" });
    controller.setCurrentArticle("w1");
    controller.setArticleMode(false);
    // The collapse message arrives while the frame is still fullscreen-sized; the host resizes it afterwards.
    controller.setArticleMode(true);
    size(346, 231);
    onResize();
    const fresh = coverCamera([], { width: 346, height: 231 });
    const camera = readUrlState().camera!;
    expect(camera.zoom).toBeGreaterThan(fresh.zoom);
    // The same camera a page that mounted at the small size would have.
    document.body.innerHTML = "";
    history.replaceState(null, "", "/places/?article=w1");
    mount(twoPlaces);
    expect(camera.zoom).toBeCloseTo(readUrlState().camera!.zoom, 5);
  });

  test("switching to This article by the chip keeps the reader's pan and zoom through the next resize", () => {
    history.replaceState(null, "", "/places/?article=w1&embed=1");
    const size = (w: number, h: number) =>
      vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
        width: w, height: h, top: 0, left: 0, right: w, bottom: h, x: 0, y: 0, toJSON() {},
      } as DOMRect);
    let onResize: () => void = () => {};
    vi.stubGlobal("ResizeObserver", class { constructor(cb: () => void) { onResize = cb; } observe() {} disconnect() {} });
    size(800, 500);
    const { controller } = mount();
    controller.setCurrentArticle("w1");
    controller.setArticleMode(false);
    document.querySelector<HTMLElement>('.moss-places-chip [data-scope="article"]')!.click();
    keyboardPan(controller.viewportEl, "ArrowRight", 3);
    const before = readUrlState().camera!;
    size(700, 500);
    onResize();
    const after = readUrlState().camera!;
    const expected = resizeCamera(before, { width: 800, height: 500 }, { width: 700, height: 500 });
    expect(after.zoom).toBeCloseTo(expected.zoom, 1);
    expect(after.x).toBeCloseTo(expected.x, 0);
    expect(after.y).toBeCloseTo(expected.y, 0);
  });

  test("the saved All articles view comes back at the same scale after the viewport changed size in between", () => {
    history.replaceState(null, "", "/places/?article=w1&embed=1&p=patterson&z=3&x=400&y=200");
    const size = (w: number, h: number) =>
      vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
        width: w, height: h, top: 0, left: 0, right: w, bottom: h, x: 0, y: 0, toJSON() {},
      } as DOMRect);
    size(1440, 900);
    const { controller } = mount();
    controller.setCurrentArticle("w1");
    controller.setArticleMode(true);
    size(800, 500);
    controller.setArticleMode(false);
    const expected = resizeCamera({ x: 400, y: 200, zoom: 3 }, { width: 1440, height: 900 }, { width: 800, height: 500 });
    expect(readUrlState().camera!.zoom).toBeCloseTo(expected.zoom, 1);
  });

  test("the card row counts a work as in view when only a later place of it is", () => {
    vi.useFakeTimers();
    const twoPlaces = {
      works: [{ ...PLACES.works[0], places: ["p1", "p2"] }],
      places: [...PLACES.places, { id: "p2", name: "P2", lat: 40, lng: 60, precision: "city", parent: null }],
    };
    history.replaceState(null, "", "/places/");
    const { figure, controller } = mount(twoPlaces);
    controller.setScope({ kind: "article", id: "w1" });
    const camera = readUrlState().camera!;
    const second = worldToScreen(project(40, 60), camera, VIEWPORT);
    wheelZoomIn(controller.viewportEl, second.x, second.y, 40);
    vi.advanceTimersByTime(1000);
    // The work's first place has scrolled out; only its second is in view.
    const first = worldToScreen(project(10, 10), readUrlState().camera!, VIEWPORT);
    expect(first.x < -22 || first.x > VIEWPORT.width + 22 || first.y < -22 || first.y > VIEWPORT.height + 22).toBe(true);
    expect(figure.querySelector('[data-work-id="w1"]')).not.toBeNull();
    vi.useRealTimers();
  });

  test("refitScopeIfClipped re-fits the camera when the scope's own points fall outside a new, much narrower viewport", () => {
    history.replaceState(null, "", "/places/?place=p1");
    const { controller } = mount();
    // Shrink the viewport drastically (the embed's own collapse) and move
    // the camera off to a corner that leaves p1's marker far outside the
    // new frame — the shape of damage an aspect-ratio-changing resize can
    // do that the ordinary re-clamp (same centre, new ceiling) does not fix.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      width: 160, height: 120, top: 0, left: 0, right: 160, bottom: 120, x: 0, y: 0, toJSON() {},
    } as DOMRect);
    controller.refitScopeIfClipped();
    const after = readUrlState();
    expect(after.camera).not.toBeNull();
    // A successful re-fit lands the camera back near p1's own projected
    // point (lat 10, lng 10) rather than wherever the pre-shrink camera sat.
    const p1 = project(10, 10);
    expect(after.camera!.x).toBeCloseTo(p1.x, 0);
    expect(after.camera!.y).toBeCloseTo(p1.y, 0);
  });
});

describe("mountPlacesMap — a work's fit ceiling follows tile coverage", () => {
  const PLACES = {
    works: [{ id: "w1", title: "W1", byline: [], authors: [], companions: [], places: ["p1"], date: "2024-01-01", description: "", cover: null, url: "/w1/" }],
    places: [{ id: "p1", name: "P1", lat: 15, lng: 25, precision: "city", parent: null }],
  } as any;
  function fitZoom(tileCells: Array<[number, number]>): number {
    history.replaceState(null, "", "/places/?article=w1");
    const figure = document.createElement("figure");
    document.body.append(figure);
    mountPlacesMap(figure, { worldSvgText: WORLD_SVG, tilesBaseUrl: "/_moss/map.abc/", tileCells, tileK: 4, tileOrigins: {}, tileColumns: 36, tileRows: 18, places: PLACES, lang: "en" });
    return readUrlState().camera!.zoom;
  }

  test("a work whose cell has a tile reaches past the world ceiling; with no tiles the fit stops at it", () => {
    const worldCeiling = detailMaxZoom(VIEWPORT);
    expect(fitZoom([TILE_CELL])).toBeGreaterThan(worldCeiling + 0.1);
    expect(fitZoom([])).toBeLessThanOrEqual(worldCeiling + 1e-2);
  });
});

describe("mountPlacesMap — the applyCamera re-clamp never forces a zoom-out on a pan", () => {
  test("panning off a tile patch, after zooming in on it past the world ceiling, keeps the zoom", async () => {
    const figure = document.createElement("figure");
    document.body.append(figure);
    const controller = mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG,
      tilesBaseUrl: "/_moss/map.abc/",
      tileCells: [TILE_CELL],
      tileK: 4,
      tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [], places: [] },
      lang: "en",
    });
    expect(controller).not.toBeNull();
    const viewportEl = figure.querySelector<HTMLElement>(".moss-places-viewport")!;

    // Zoom in anchored on the tile cell's own centre: compute, from the
    // KNOWN initial cover camera, the screen point that cell centre sits
    // under right now, then hold the cursor there across every wheel tick —
    // `zoomAt`'s own math keeps that anchored world point fixed under it,
    // so the camera converges toward the cell centre as zoom rises.
    const initialCamera = coverCamera([], VIEWPORT);
    const initialScale = screenScale(initialCamera, VIEWPORT);
    const target = project(15, 25); // the TILE_CELL's own centre (lat 15, lng 25)
    const anchorX = VIEWPORT.width / 2 + (target.x - initialCamera.x) * initialScale;
    const anchorY = VIEWPORT.height / 2 + (target.y - initialCamera.y) * initialScale;
    wheelZoomIn(viewportEl, anchorX, anchorY, 60);
    await new Promise((resolve) => setTimeout(resolve, 150));

    const worldCeiling = detailMaxZoom(VIEWPORT);
    const tileCeiling = tileDetailMaxZoom(VIEWPORT);
    const zoomedState = readUrlState();
    expect(zoomedState.camera).not.toBeNull();
    const zoomInZoom = zoomedState.camera!.zoom;
    // The zoom-in actually reached past the world's own ceiling — otherwise
    // this test would prove nothing about the raised, tile-covered one.
    expect(zoomInZoom).toBeGreaterThan(worldCeiling);
    // `writeCamera` round-trips zoom through `toFixed(3)`, so the read-back
    // value can be a hair past the true ceiling from rounding alone.
    expect(zoomInZoom).toBeLessThanOrEqual(tileCeiling + 1e-2);

    // Pan due east, far enough (way past the cell's own width plus the
    // fetch-ahead pad `tilesForView` gives it) to leave TILE_CELL and its
    // pad entirely — the only cell this layer has, so nothing else keeps
    // the ceiling raised once this lands.
    keyboardPan(viewportEl, "ArrowRight", 60);

    const pannedState = readUrlState();
    expect(pannedState.camera).not.toBeNull();
    // The bug this guards: re-clamping against a ceiling recomputed AFTER
    // the pan (now off the tile, so back to the plain world ceiling) would
    // snap the zoom down mid-drag. A pan must never change zoom at all.
    expect(pannedState.camera!.zoom).toBeCloseTo(zoomInZoom, 3);
  });
});

function rect(x: number, y: number, width: number, height: number): DOMRect {
  return { x, y, width, height, top: y, left: x, right: x + width, bottom: y + height, toJSON() {} } as DOMRect;
}

describe("mountPlacesMap — the label layer actually reserves the breadcrumb chip's own area", () => {
  test("a city label whose only possible positions all sit under the chip is hidden, not drawn over it", () => {
    const viewport = { width: 800, height: 600 };
    // A box around the viewport's centre generous enough to cover every one
    // of a city label's four candidate positions (right/left/above/below
    // its anchor, each offset by only a few px — see labels.ts's own
    // `candidateBox`), so however the dead-selector bug this guards against
    // would have let the label land, it still falls inside this rect.
    const chipRect = { x: 275, y: 175, width: 250, height: 250 };
    const cityPoint = project(10, 20); // arbitrary — no work sits here, so no marker competes for the same spot

    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const el = this as HTMLElement;
      if (el.classList.contains("moss-places-viewport")) return rect(0, 0, viewport.width, viewport.height);
      if (el.classList.contains("moss-places-chip")) return rect(chipRect.x, chipRect.y, chipRect.width, chipRect.height);
      // `labels.ts`'s own `measure()` reads this for a label's text size —
      // jsdom never lays out real text, so this stands in for it.
      if (el.classList.contains("moss-places-label")) return rect(0, 0, 60, 20);
      // Every other element (controls, cards, markers, the world…) reserves
      // nothing, so only the chip's own rect is in play.
      return rect(0, 0, 0, 0);
    });
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
    // Read back as the initial camera (map.ts's own `urlState.readUrlState`),
    // centred exactly on the city's world point — its unreserved screen
    // anchor is then exactly the viewport centre, deep inside `chipRect`.
    history.replaceState(null, "", `/places/?p=patterson&z=4&x=${cityPoint.x}&y=${cityPoint.y}`);

    const figure = document.createElement("figure");
    document.body.append(figure);
    const labels: LabelsData = {
      languages: ["en"],
      en: { cities: [{ name: "Testopolis", lat: 10, lng: 20, rank: 0 }], ranges: [], peaks: [], rivers: [] },
    };
    const controller = mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG,
      tilesBaseUrl: "/_moss/map.abc/",
      tileCells: [],
      tileK: 4,
      tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [], places: [] },
      labels,
      lang: "en",
    });
    expect(controller).not.toBeNull();

    const labelEl = figure.querySelector<HTMLElement>(".moss-places-label[data-kind='city']");
    expect(labelEl).not.toBeNull();
    expect(labelEl!.hidden).toBe(true);
  });

  test("a city label under the OPEN dig-down menu is hidden too, and returns once the menu closes", () => {
    const viewport = { width: 800, height: 600 };
    // The chip's own trail stays small and top-left, nowhere near the
    // city's own (viewport-centred, see the camera below) candidate
    // positions — only the MENU'S box is generous enough to reach them,
    // the same shape the sibling test above gives the chip for its own
    // assertion, so only the menu's own reservation can be what hides it.
    const chipRect = { x: 10, y: 10, width: 120, height: 30 };
    const menuRect = { x: 275, y: 175, width: 250, height: 250 };
    const cityPoint = project(10, 20);

    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const el = this as HTMLElement;
      if (el.classList.contains("moss-places-viewport")) return rect(0, 0, viewport.width, viewport.height);
      if (el.classList.contains("moss-places-chip-menu")) return rect(menuRect.x, menuRect.y, menuRect.width, menuRect.height);
      if (el.classList.contains("moss-places-chip")) return rect(chipRect.x, chipRect.y, chipRect.width, chipRect.height);
      if (el.classList.contains("moss-places-label")) return rect(0, 0, 60, 20);
      return rect(0, 0, 0, 0);
    });
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
    // Centred on the city's own world point, same as the sibling test
    // above — its unreserved anchor is the viewport centre, which the
    // menu's own box is generous enough to reach from every one of a
    // label's four candidate positions, the same reasoning that test's own
    // chipRect comment gives.
    history.replaceState(null, "", `/places/?p=patterson&z=4&x=${cityPoint.x}&y=${cityPoint.y}`);

    const figure = document.createElement("figure");
    document.body.append(figure);
    const labels: LabelsData = {
      languages: ["en"],
      en: { cities: [{ name: "Testopolis", lat: 10, lng: 20, rank: 0 }], ranges: [], peaks: [], rivers: [] },
    };
    const place: Place = { id: "p1", name: "Place One", precision: "city", lat: 1, lng: 1 };
    const work: Work = { id: "w1", title: "Work One", url: "/w1", byline: [], authors: [], places: ["p1"], companions: [] };
    const controller = mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG,
      tilesBaseUrl: "/_moss/map.abc/",
      tileCells: [],
      tileK: 4,
      tileOrigins: {}, tileColumns: 36, tileRows: 18,
      places: { works: [work], places: [place] },
      labels,
      lang: "en",
    });
    expect(controller).not.toBeNull();

    const labelEl = figure.querySelector<HTMLElement>(".moss-places-label[data-kind='city']");
    expect(labelEl).not.toBeNull();
    // Closed menu: the viewport centre is clear, so the label is free to
    // land on its own anchor (this is the dead-selector regression the
    // sibling test above already guards — re-asserted here only as the
    // baseline the next assertion's own change is measured against).
    expect(labelEl!.hidden).toBe(false);

    const trigger = figure.querySelector<HTMLButtonElement>(".moss-places-chip-crumb[data-terminal]");
    expect(trigger).not.toBeNull();
    // Opening the menu touches none of the four inputs `render()` keys on
    // (scope/selection/ring/locale), so nothing in the chip's own render
    // path would otherwise re-run `reservedLabelRects()` for it —
    // `chip.ts`'s own `menuToggled()` callback (wired to `applyCamera(true)`
    // in `map.ts`) is what the click below actually has to reach for the
    // label to react at all; this is the regression this test guards.
    trigger!.click();
    expect(figure.querySelector(".moss-places-chip-menu")).not.toBeNull();
    expect(labelEl!.hidden).toBe(true);

    const openMenu = figure.querySelector<HTMLElement>(".moss-places-chip-menu");
    const menuItem = openMenu?.querySelector<HTMLElement>(".moss-places-chip-menu-item");
    menuItem?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(figure.querySelector(".moss-places-chip-menu")).toBeNull();
    expect(labelEl!.hidden).toBe(false);
  });
});
