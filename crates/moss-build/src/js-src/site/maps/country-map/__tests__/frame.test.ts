/** Framing: the country is centred in the space beside the panel, between 1x and 2.5x the world view. */
import { describe, expect, test } from "vitest";
import { framed, parseBox, scrollLeftFor, type Box, type ViewBox } from "../frame";

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

describe("scrollLeftFor", () => {
  test("puts the country in the middle of the sideways box", () => {
    expect(scrollLeftFor(500, 1000, 2000, 390)).toBe(805);
  });
  test("never scrolls before the start", () => {
    expect(scrollLeftFor(10, 1000, 2000, 390)).toBe(0);
  });
});
