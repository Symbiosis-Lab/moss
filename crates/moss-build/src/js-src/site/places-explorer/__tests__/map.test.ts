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
import { coverCamera, detailMaxZoom, screenScale, tileDetailMaxZoom } from "../camera";
import { mountPlacesMap } from "../map";
import { project } from "../projection";
import { readUrlState } from "../state";
import type { LabelsData } from "../types";

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
  history.replaceState(null, "", "/places/");
});

/** Dispatch a ctrl-wheel zoom-in at a FIXED screen point, so repeated calls keep converging the camera toward the same world point under it (the real gesture a reader holding the cursor still over a cell while scrolling produces). */
function wheelZoomIn(viewportEl: HTMLElement, clientX: number, clientY: number, times: number): void {
  for (let i = 0; i < times; i++) {
    viewportEl.dispatchEvent(new WheelEvent("wheel", { ctrlKey: true, deltaY: -1, clientX, clientY, bubbles: true, cancelable: true }));
  }
}

function keyboardPan(viewportEl: HTMLElement, key: string, times: number): void {
  for (let i = 0; i < times; i++) {
    viewportEl.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey: true, bubbles: true, cancelable: true }));
  }
}

describe("mountPlacesMap — the applyCamera re-clamp never forces a zoom-out on a pan", () => {
  test("panning off a tile patch, after zooming in on it past the world ceiling, keeps the zoom", () => {
    const figure = document.createElement("figure");
    document.body.append(figure);
    const controller = mountPlacesMap(figure, {
      worldSvgText: WORLD_SVG,
      tilesBaseUrl: "/_moss/map.abc/",
      tileCells: [TILE_CELL],
      tileK: 4,
      tileBleed: 0.1,
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

    const worldCeiling = detailMaxZoom(VIEWPORT);
    const tileCeiling = tileDetailMaxZoom(VIEWPORT, 4);
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
      tileBleed: 0.1,
      places: { works: [], places: [] },
      labels,
      lang: "en",
    });
    expect(controller).not.toBeNull();

    const labelEl = figure.querySelector<HTMLElement>(".moss-places-label[data-kind='city']");
    expect(labelEl).not.toBeNull();
    expect(labelEl!.hidden).toBe(true);
  });
});
