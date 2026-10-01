/**
 * Tests for camera.ts — the places-explorer's pan/zoom state over the fixed
 * world canvas `projection.ts` projects into.
 */

import { describe, test, expect } from "vitest";
import { WORLD_WIDTH, WORLD_HEIGHT } from "../projection";
import {
  DETAIL_MAX_SCALE,
  MIN_ZOOM,
  screenScale,
  detailMaxZoom,
  clampCamera,
  coverCamera,
  fitPoints,
  worldToScreen,
  screenToWorld,
} from "../camera";

describe("coverCamera", () => {
  test.each([
    ["16:9 landscape", { width: 1280, height: 720 }],
    ["4:3 landscape", { width: 1024, height: 768 }],
    ["3:4 portrait", { width: 768, height: 1024 }],
    ["9:16 portrait", { width: 720, height: 1280 }],
  ])("leaves no empty band at %s", (_label, viewport) => {
    const camera = coverCamera([], viewport);
    const scale = screenScale(camera, viewport);
    // "No empty band" means the world, at this scale, covers the viewport on
    // both axes (floating point: allow a hair of slack, never an actual gap).
    expect(WORLD_WIDTH * scale).toBeGreaterThanOrEqual(viewport.width - 1e-6);
    expect(WORLD_HEIGHT * scale).toBeGreaterThanOrEqual(viewport.height - 1e-6);
  });

  test("pans to the densest window of points on the cropped axis", () => {
    // A wide viewport crops the world's height; two points near the bottom
    // of the map should pull the cover window down toward them instead of
    // leaving it centred on the whole globe.
    const viewport = { width: 2000, height: 400 };
    const dense = [
      { x: 500, y: 460 },
      { x: 520, y: 465 },
    ];
    const camera = coverCamera(dense, viewport);
    const centred = coverCamera([], viewport);
    expect(camera.y).toBeGreaterThan(centred.y);
  });
});

describe("detailMaxZoom / screenScale", () => {
  test("the ceiling is equal in pixels per world unit at any viewport size", () => {
    const narrow = { width: 360, height: 640 };
    const wide = { width: 1440, height: 900 };
    const narrowCeilingScale = screenScale({ x: 0, y: 0, zoom: detailMaxZoom(narrow) }, narrow);
    const wideCeilingScale = screenScale({ x: 0, y: 0, zoom: detailMaxZoom(wide) }, wide);
    expect(Math.abs(narrowCeilingScale - DETAIL_MAX_SCALE)).toBeLessThan(1e-9);
    expect(Math.abs(wideCeilingScale - DETAIL_MAX_SCALE)).toBeLessThan(1e-9);
  });
});

describe("clampCamera", () => {
  test("never drops below the cover floor nor past the detail ceiling", () => {
    const viewport = { width: 800, height: 450 };
    const tooLow = clampCamera({ x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: 0.1 }, viewport);
    expect(tooLow.zoom).toBe(MIN_ZOOM);
    const tooHigh = clampCamera({ x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: 999 }, viewport);
    expect(tooHigh.zoom).toBe(detailMaxZoom(viewport));
  });

  test("a zero-size viewport (before the first layout) never produces NaN", () => {
    const camera = { x: 400, y: 260, zoom: 2 };
    for (const viewport of [
      { width: 0, height: 0 },
      { width: 0, height: 480 },
      { width: 720, height: 0 },
    ]) {
      const clamped = clampCamera(camera, viewport);
      expect(Number.isFinite(clamped.x)).toBe(true);
      expect(Number.isFinite(clamped.y)).toBe(true);
      expect(Number.isFinite(clamped.zoom)).toBe(true);
    }
    expect(clampCamera(camera, { width: 0, height: 0 })).toEqual({ x: 400, y: 260, zoom: MIN_ZOOM });
  });

  test("preserves the geographic centre across a viewport resize", () => {
    const camera = { x: 340, y: 240, zoom: MIN_ZOOM };
    const before = clampCamera(camera, { width: 1200, height: 900 });
    const after = clampCamera(before, { width: 900, height: 750 });
    expect(after.x).toBe(before.x);
    expect(after.y).toBe(before.y);
  });
});

describe("fitPoints", () => {
  test("fits two far-apart points inside the viewport with padding", () => {
    const viewport = { width: 1000, height: 600 };
    const a = { x: 300, y: 200 };
    const b = { x: 700, y: 360 };
    const camera = fitPoints([a, b], viewport);
    const scale = screenScale(camera, viewport);
    for (const point of [a, b]) {
      const screenX = viewport.width / 2 + (point.x - camera.x) * scale;
      const screenY = viewport.height / 2 + (point.y - camera.y) * scale;
      expect(screenX).toBeGreaterThan(0);
      expect(screenX).toBeLessThan(viewport.width);
      expect(screenY).toBeGreaterThan(0);
      expect(screenY).toBeLessThan(viewport.height);
    }
    // A fit that needed to zoom OUT past the cover floor to include both
    // points should have been clamped up to it instead.
    expect(camera.zoom).toBeGreaterThanOrEqual(MIN_ZOOM);
  });

  test("fitting one point stops at the detail ceiling", () => {
    const viewport = { width: 1000, height: 600 };
    const camera = fitPoints([{ x: 500, y: 280 }], viewport);
    expect(camera.zoom).toBe(detailMaxZoom(viewport));
  });
});

describe("worldToScreen / screenToWorld", () => {
  test("the camera's own centre lands in the middle of the viewport", () => {
    const viewport = { width: 1000, height: 600 };
    const camera = { x: 300, y: 200, zoom: 2 };
    const screen = worldToScreen({ x: camera.x, y: camera.y }, camera, viewport);
    expect(screen.x).toBeCloseTo(viewport.width / 2);
    expect(screen.y).toBeCloseTo(viewport.height / 2);
  });

  test("round-trips through both directions", () => {
    const viewport = { width: 1000, height: 600 };
    const camera = { x: 410, y: 180, zoom: 3.4 };
    for (const point of [{ x: 0, y: 0 }, { x: WORLD_WIDTH, y: WORLD_HEIGHT }, { x: 410, y: 180 }]) {
      const back = screenToWorld(worldToScreen(point, camera, viewport), camera, viewport);
      expect(back.x).toBeCloseTo(point.x);
      expect(back.y).toBeCloseTo(point.y);
    }
  });
});
