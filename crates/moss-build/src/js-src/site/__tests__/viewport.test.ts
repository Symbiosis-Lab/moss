/**
 * Tests for viewport.ts — the shared clamp that keeps a floating surface on
 * screen.
 *
 * These pin arithmetic, not rendering. Whether the engine then paints the
 * surface where these numbers say is a render-gate question, and
 * tests/render-gates/site/edge-clamp.spec.ts is where it is asked.
 */

import { describe, test, expect, beforeEach, afterEach } from "vitest";
import {
  GUTTER,
  clampLeft,
  maxSurfaceWidth,
  verticalSlot,
  visibleHeight,
  visibleWidth,
} from "../viewport";

/**
 * Pretend the document element reports a visible band, the way a real engine
 * does. jsdom leaves `clientWidth`/`clientHeight` at 0, which is exactly the
 * case the `|| window.innerWidth` fallback exists for — so the fallback is
 * covered by every test that does NOT call this.
 */
function setBand(width: number, height: number): void {
  Object.defineProperty(document.documentElement, "clientWidth", {
    configurable: true,
    value: width,
  });
  Object.defineProperty(document.documentElement, "clientHeight", {
    configurable: true,
    value: height,
  });
}

afterEach(() => {
  for (const prop of ["clientWidth", "clientHeight"]) {
    Object.defineProperty(document.documentElement, prop, {
      configurable: true,
      value: 0,
    });
  }
});

describe("the visible band", () => {
  test("is the document element's client box, not the window", () => {
    // A 1000px band inside a 1024px window is the 24px scrollbar gutter the
    // whole module exists to exclude.
    setBand(1000, 700);
    expect(visibleWidth()).toBe(1000);
    expect(visibleHeight()).toBe(700);
  });

  test("falls back to the window when there is no client box", () => {
    expect(visibleWidth()).toBe(window.innerWidth);
    expect(visibleHeight()).toBe(window.innerHeight);
  });

  test("a surface may fill the band minus both gutters", () => {
    setBand(400, 700);
    expect(maxSurfaceWidth()).toBe(400 - GUTTER * 2);
    expect(maxSurfaceWidth(12)).toBe(400 - 24);
  });

  test("never offers a negative width on a band narrower than its gutters", () => {
    setBand(10, 700);
    expect(maxSurfaceWidth()).toBe(0);
  });
});

describe("clampLeft", () => {
  beforeEach(() => setBand(400, 700));

  test("leaves a surface that already fits where it asked to be", () => {
    expect(clampLeft(100, 200)).toBe(100);
  });

  test("pushes a left-margin surface in to the gutter", () => {
    // The reported bug: a selection near the left margin centred a 212px-wide
    // popover on x=155, asking for a left edge of 49 — which is fine — but the
    // same popover on x=20 asks for -86 and loses its first buttons.
    expect(clampLeft(-86, 212)).toBe(GUTTER);
  });

  test("pulls a right-edge surface back inside", () => {
    // 400 - 8 - 200 = 192 is the rightmost left edge a 200px surface can have.
    expect(clampLeft(350, 200)).toBe(192);
  });

  test("prefers the left gutter when the surface is wider than the band", () => {
    // Overflowing right keeps the beginning of the text readable; overflowing
    // left would cut off the words the reader needs first.
    expect(clampLeft(200, 900)).toBe(GUTTER);
  });

  test("honours a caller's wider gutter", () => {
    expect(clampLeft(0, 100, 12)).toBe(12);
    expect(clampLeft(400, 100, 12)).toBe(400 - 12 - 100);
  });
});

describe("verticalSlot", () => {
  beforeEach(() => setBand(400, 700));

  test("sits on the preferred side when it fits", () => {
    const rect = { top: 300, bottom: 320 };
    expect(verticalSlot(rect, 36, "above")).toEqual({ top: 300 - 36 - 8, flipped: false });
    expect(verticalSlot(rect, 36, "below")).toEqual({ top: 328, flipped: false });
  });

  test("flips rather than overlapping its own anchor", () => {
    // A selection on the first visible line: "above" is off the top of the
    // screen, and clamping to the top gutter would cover the selection itself.
    const first = { top: 4, bottom: 24 };
    expect(verticalSlot(first, 36, "above")).toEqual({ top: 32, flipped: true });

    // A link near the bottom: "below" runs off, so the card goes above.
    const last = { top: 660, bottom: 680 };
    expect(verticalSlot(last, 36, "below")).toEqual({ top: 660 - 36 - 8, flipped: true });
  });

  test("pins inside the band when neither side fits", () => {
    // A surface taller than the window has nowhere to go; the top gutter at
    // least keeps its beginning readable.
    setBand(400, 100);
    expect(verticalSlot({ top: 40, bottom: 60 }, 300, "above").top).toBe(GUTTER);
  });
});
