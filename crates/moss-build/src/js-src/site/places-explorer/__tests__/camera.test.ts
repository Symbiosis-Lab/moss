/**
 * Tests for camera.ts — the places-explorer's pan/zoom state over the fixed
 * world canvas `projection.ts` projects into.
 */

import { describe, test, expect } from "vitest";
import { WORLD_WIDTH, WORLD_HEIGHT, project } from "../projection";
import {
  DETAIL_MAX_SCALE,
  MIN_ZOOM,
  screenScale,
  detailMaxZoom,
  clampCamera,
  coverCamera,
  fitPoints,
  fitWork,
  resizeCamera,
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

  test("a frame narrower than the viewport raises zoom enough to re-centre on it without opening a gap", () => {
    const viewport = { width: 1000, height: 600 };
    // At this viewport, zoom=1 exactly covers the HEIGHT axis with zero
    // slack (480 world units * (600/480) scale = 600 = the viewport
    // height) — so centring on a shorter frame's own middle is only
    // possible at all by raising the zoom past the plain cover floor.
    const frame = { x: 0, y: 0, width: 1000, height: 450 };
    const framed = coverCamera([], viewport, frame);
    expect(framed.zoom).toBeGreaterThan(MIN_ZOOM);

    // The world point this camera draws at the VIEWPORT's own middle must
    // be the one that sits at the FRAME's middle instead — i.e. the point
    // 75 screen px (the gap between the two centres) above the viewport's
    // own vertical middle must map to true world-space y=WORLD_HEIGHT/2.
    const scale = screenScale(framed, viewport);
    const frameMiddleScreenY = frame.y + frame.height / 2;
    const worldAtFrameMiddle = framed.y + (frameMiddleScreenY - viewport.height / 2) / scale;
    expect(worldAtFrameMiddle).toBeCloseTo(WORLD_HEIGHT / 2, 5);

    // Still no visible gap at the REAL viewport's own top/bottom edges.
    const worldTopScreenY = viewport.height / 2 + (0 - framed.y) * scale;
    const worldBottomScreenY = viewport.height / 2 + (WORLD_HEIGHT - framed.y) * scale;
    expect(worldTopScreenY).toBeLessThanOrEqual(1e-6);
    expect(worldBottomScreenY).toBeGreaterThanOrEqual(viewport.height - 1e-6);
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

  test("a frame shorter than the viewport (a card row eating the bottom) keeps points out of the cropped band", () => {
    // The map is 1000x600; a card row reserves the bottom 150px, leaving a
    // 450px-tall free rectangle starting at the top. A point fit against
    // that frame must land ABOVE the reserved band, not centred in the
    // full viewport the way a frame-less fit would place it.
    const viewport = { width: 1000, height: 600 };
    const frame = { x: 0, y: 0, width: 1000, height: 450 };
    const point = { x: 500, y: 280 };
    const framed = fitPoints([point], viewport, detailMaxZoom(viewport), frame);
    const scale = screenScale(framed, viewport);
    const screenY = viewport.height / 2 + (point.y - framed.y) * scale;
    expect(screenY).toBeLessThan(frame.height);
    // Without a frame the same point centres in the full viewport instead.
    const unframed = fitPoints([point], viewport);
    const unframedScreenY = viewport.height / 2 + (point.y - unframed.y) * scale;
    expect(unframedScreenY).toBeCloseTo(viewport.height / 2);
  });
});

describe("fitWork", () => {
  test("a zero-size viewport (a hidden embed) yields a finite camera, never NaN", () => {
    for (const viewport of [{ width: 0, height: 0 }, { width: 0, height: 231 }, { width: 346, height: 0 }]) {
      const camera = fitWork([{ x: 300, y: 200 }], viewport);
      expect(Number.isFinite(camera.x)).toBe(true);
      expect(Number.isFinite(camera.y)).toBe(true);
      expect(Number.isFinite(camera.zoom)).toBe(true);
    }
  });

  test("a multi-place work still fits every one of its places inside the viewport", () => {
    const viewport = { width: 1000, height: 600 };
    const a = { x: 300, y: 200 };
    const b = { x: 700, y: 360 };
    const camera = fitWork([a, b], viewport);
    const scale = screenScale(camera, viewport);
    for (const point of [a, b]) {
      const screenX = viewport.width / 2 + (point.x - camera.x) * scale;
      const screenY = viewport.height / 2 + (point.y - camera.y) * scale;
      expect(screenX).toBeGreaterThan(0);
      expect(screenX).toBeLessThan(viewport.width);
      expect(screenY).toBeGreaterThan(0);
      expect(screenY).toBeLessThan(viewport.height);
    }
  });

  test("a frame shorter than the viewport (a card row eating the bottom) still keeps the point out of the cropped band, same as fitPoints", () => {
    const viewport = { width: 1000, height: 600 };
    const frame = { x: 0, y: 0, width: 1000, height: 450 };
    const point = { x: 500, y: 280 };
    const framed = fitWork([point], viewport, frame);
    const scale = screenScale(framed, viewport);
    const screenY = viewport.height / 2 + (point.y - framed.y) * scale;
    expect(screenY).toBeLessThan(frame.height);
  });
});

describe("fitWork framing", () => {
  type Cam = { x: number; y: number; zoom: number };
  type Vp = { width: number; height: number };
  const degreesWide = (camera: Cam, viewport: Vp) => (viewport.width / screenScale(camera, viewport) / WORLD_WIDTH) * 360;
  const inside = (camera: Cam, viewport: Vp, p: { x: number; y: number }) => {
    const s = worldToScreen(p, camera, viewport);
    return s.x > 0 && s.x < viewport.width && s.y > 0 && s.y < viewport.height;
  };
  const a = project(26.7, 119.6);
  const b = project(25.0, 121.3);
  // The tile ceiling (k=4) itself caps how tight a 1440 px frame can go: 1440 / (7.21*560/480*4) world units is about 18.3 degrees.
  const cases: [string, Vp, { x: number; y: number; width: number; height: number } | undefined, number][] = [
    ["346x231 embed", { width: 346, height: 231 }, undefined, 16],
    ["1440x776 with a card row", { width: 1440, height: 776 }, { x: 0, y: 0, width: 1440, height: 610 }, 19],
  ];
  for (const [name, viewport, frame, upper] of cases) {
    test(`two places 2 degrees apart frame regionally in ${name}`, () => {
      const camera = fitWork([a, b], viewport, frame);
      const span = degreesWide(camera, viewport);
      expect(span).toBeGreaterThan(4);
      expect(span).toBeLessThan(upper);
      expect(inside(camera, viewport, a) && inside(camera, viewport, b)).toBe(true);
    });
    test(`one place frames regionally in ${name}`, () => {
      const span = degreesWide(fitWork([a], viewport, frame), viewport);
      expect(span).toBeGreaterThan(4);
      expect(span).toBeLessThan(upper);
    });
  }
  test("a tiny free rectangle (overlays eating an embed) falls back to the whole viewport", () => {
    const viewport = { width: 346, height: 231 };
    const small = fitWork([a, b], viewport, { x: 0, y: 150, width: 346, height: 40 });
    expect(small).toEqual(fitWork([a, b], viewport));
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

describe("resizeCamera", () => {
  const small = { width: 352, height: 235 };
  const large = { width: 1440, height: 900 };

  test("keeps the scale and centre, so a bigger viewport shows more of the world", () => {
    const camera = { x: 700, y: 180, zoom: 20 };
    const grown = resizeCamera(camera, small, large);
    expect(screenScale(grown, large)).toBeCloseTo(screenScale(camera, small), 6);
    expect(grown.x).toBe(camera.x);
    expect(grown.y).toBe(camera.y);
    // Wider viewport at the same scale: more world units across.
    expect(large.width / screenScale(grown, large)).toBeGreaterThan(small.width / screenScale(camera, small));
  });

  test("is undone by resizing back", () => {
    const camera = { x: 700, y: 180, zoom: 20 };
    const back = resizeCamera(resizeCamera(camera, small, large), large, small);
    expect(back.zoom).toBeCloseTo(camera.zoom, 6);
  });

  test("a camera at the cover floor stays there", () => {
    expect(resizeCamera({ x: 421, y: 240, zoom: MIN_ZOOM }, large, small).zoom).toBe(MIN_ZOOM);
  });

  test("a viewport with no area leaves the camera alone", () => {
    const camera = { x: 700, y: 180, zoom: 20 };
    expect(resizeCamera(camera, { width: 0, height: 0 }, large)).toEqual(camera);
    expect(resizeCamera(camera, small, { width: 0, height: 0 })).toEqual(camera);
  });
});
