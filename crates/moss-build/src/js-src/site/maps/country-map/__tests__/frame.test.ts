/** Framing: the country is centred in the space beside the panel, between 1x and 2.5x the world view. */
import { describe, expect, test } from "vitest";
import { clampView, framed, MAX_ZOOM, openingView, pannedBy, parseBox, sameView, wholeView, zoomedAt, type Box, type ViewBox } from "../frame";

const BASE: ViewBox = [0, 0, 1000, 435];
const centreOf = (b: Box) => [(b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2];

describe("framed", () => {
  test("a small country zooms to the 2.5x cap, not further", () => {
    const [, , w, h] = framed({ x0: 700, y0: 200, x1: 704, y1: 204 }, BASE, 0.55);
    expect(w).toBeCloseTo(400);
    expect(h).toBeCloseTo(174);
  });
  test("a vast country never shows less than the whole world's scale", () => {
    const [, , w] = framed({ x0: 560, y0: 40, x1: 960, y1: 160 }, BASE, 0.55);
    expect(w).toBeCloseTo(1000);
  });
  test("the country's centre lands in the middle of the free space", () => {
    const box = { x0: 200, y0: 180, x1: 260, y1: 230 };
    const [x, y, w, h] = framed(box, BASE, 0.5);
    const [cx, cy] = centreOf(box);
    expect(x + w * 0.25).toBeCloseTo(cx);
    expect(y + h / 2).toBeCloseTo(cy);
  });
  test("a medium country zooms only as far as it fits beside the panel", () => {
    const [, , w] = framed({ x0: 200, y0: 180, x1: 400, y1: 230 }, BASE, 0.5);
    expect(w).toBeGreaterThan(400);
    expect(w).toBeLessThan(1000);
  });
});

describe("parseBox", () => {
  test("reads the build's four numbers and refuses anything else", () => {
    expect(parseBox("1 2.5 30 40")).toEqual({ x0: 1, y0: 2.5, x1: 30, y1: 40 });
    expect(parseBox(undefined)).toBeNull();
    expect(parseBox("1 2 3")).toBeNull();
  });
});

const WORLD: ViewBox = [0, 0, 1000, 435];
const WIDE_ASPECT = 1000 / 435;
const PHONE_ASPECT = 23 / 20;

describe("the whole view and the opening view", () => {
  test("in a box with the world's own ratio both are the world", () => {
    expect(wholeView(WORLD, WIDE_ASPECT)).toEqual(WORLD);
    expect(openingView(WORLD, WIDE_ASPECT)).toEqual(WORLD);
  });
  test("in a squarer box the whole view shows the world's width, centred with room above and below", () => {
    const [x, y, w, h] = wholeView(WORLD, PHONE_ASPECT);
    expect([x, w]).toEqual([0, 1000]);
    expect(w / h).toBeCloseTo(PHONE_ASPECT);
    expect(y + h / 2).toBeCloseTo(435 / 2);
  });
  test("in a squarer box the map opens on the world's full height, centred on its middle", () => {
    const [x, y, w, h] = openingView(WORLD, PHONE_ASPECT);
    expect([y, h]).toEqual([0, 435]);
    expect(w / h).toBeCloseTo(PHONE_ASPECT);
    expect(x + w / 2).toBeCloseTo(500);
  });
});

describe("zoom limits", () => {
  const whole = wholeView(WORLD, WIDE_ASPECT);
  test("zooming out stops at the whole view and zooming in at 8 times it", () => {
    expect(zoomedAt(whole, 0.5, 0.5, 0.5, WORLD, whole)[2]).toBeCloseTo(1000);
    expect(zoomedAt(whole, 100, 0.5, 0.5, WORLD, whole)[2]).toBeCloseTo(1000 / MAX_ZOOM);
  });
  test("zooming keeps the point under the pointer where it was", () => {
    const [x, y, w, h] = zoomedAt(whole, 2, 0.25, 0.75, WORLD, whole);
    expect(x + 0.25 * w).toBeCloseTo(250);
    expect(y + 0.75 * h).toBeCloseTo(0.75 * 435);
  });
  test("the view never leaves the world", () => {
    const zoomed = zoomedAt(whole, 4, 0.5, 0.5, WORLD, whole);
    const [x, y, w, h] = pannedBy(zoomed, 1e6, 1e6, { width: 1000, height: 435 }, WORLD, whole);
    expect([x, y]).toEqual([0, 0]);
    const [x2, y2] = pannedBy(zoomed, -1e6, -1e6, { width: 1000, height: 435 }, WORLD, whole);
    expect([x2 + w, y2 + h]).toEqual([1000, 435]);
  });
  test("at the whole view there is nothing to pan", () => {
    expect(pannedBy(whole, 50, 50, { width: 1000, height: 435 }, WORLD, whole)).toEqual(whole);
  });
  test("a view wider than the world is centred on it", () => {
    const phone = wholeView(WORLD, PHONE_ASPECT);
    expect(clampView([0, -50, 1000, phone[3]], WORLD, phone)[1]).toBeCloseTo(phone[1]);
  });
  test("two views within half a unit are the same", () => {
    expect(sameView([0, 0, 1000, 435], [0.2, 0, 1000, 434.8])).toBe(true);
    expect(sameView([0, 0, 1000, 435], [0, 0, 900, 435])).toBe(false);
  });
});
