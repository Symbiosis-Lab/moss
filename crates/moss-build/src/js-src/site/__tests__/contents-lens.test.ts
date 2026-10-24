import { describe, expect, test } from "vitest";

import { faintOver, falloff, FOCUS_ROW, layoutRows, OPEN_ROW, PITCH, restFocus, REST_ROW, travel } from "../nav/contents-lens";

describe("falloff", () => {
  test("is 1 at the focus and 0 at and beyond the radius", () => {
    expect(falloff(0)).toBe(1);
    expect(falloff(2.2)).toBe(0);
    expect(falloff(7)).toBe(0);
  });
  test("is symmetric, falls monotonically, and is a cosine at half the radius", () => {
    expect(falloff(-1)).toBeCloseTo(falloff(1), 10);
    expect(falloff(0.5)).toBeGreaterThan(falloff(1));
    expect(falloff(1)).toBeGreaterThan(falloff(2));
    expect(falloff(1.1)).toBeCloseTo(0.5, 10);
  });
});

describe("restFocus", () => {
  test("is the row under the pointer, a row's centre being its index", () => {
    expect(restFocus(300 + 10, 300, 6)).toBeCloseTo(0, 10);
    expect(restFocus(300 + 2 * REST_ROW + 10, 300, 6)).toBeCloseTo(2, 10);
    expect(restFocus(300 + 2 * REST_ROW + 15, 300, 6)).toBeCloseTo(2.25, 10);
  });
  test("is clamped to the first and last row", () => {
    expect(restFocus(100, 300, 6)).toBe(0);
    expect(restFocus(900, 300, 6)).toBe(5);
  });
});

describe("travel", () => {
  test("moves one row per pitch of pointer travel from the entry point", () => {
    expect(travel(2, 400, 400 + PITCH, 7)).toEqual({ f: 3, y: 400 + PITCH });
    expect(travel(2, 400, 400 - PITCH / 2, 7)).toEqual({ f: 1.5, y: 400 - PITCH / 2 });
  });
  test("past either end the focus stops, and so does the height it is drawn at", () => {
    const below = travel(5, 400, 400 + 10 * PITCH, 7);
    expect(below).toEqual({ f: 6, y: 400 + PITCH });
    expect(travel(5, 400, 400 + 20 * PITCH, 7)).toEqual(below);
    expect(travel(1, 400, 0, 7)).toEqual({ f: 0, y: 400 - PITCH });
  });
});

describe("layoutRows", () => {
  const n = 7;

  test("rows tile, from the open height far away to the focus height at it", () => {
    const rows = layoutRows(3, 400, n);
    for (let i = 1; i < n; i++) expect(rows[i].top).toBeCloseTo(rows[i - 1].top + rows[i - 1].size, 10);
    expect(rows[3].size).toBeCloseTo(FOCUS_ROW, 10);
    expect(rows[0].size).toBeCloseTo(OPEN_ROW, 10);
    expect(rows[6].size).toBeCloseTo(OPEN_ROW, 10);
    expect(rows[2].size).toBeGreaterThan(OPEN_ROW);
    expect(rows[2].size).toBeLessThan(FOCUS_ROW);
    expect(rows[2].size).toBeCloseTo(rows[4].size, 10);
  });

  test("a whole focus is centred on the height it is held at", () => {
    for (const k of [0, 3, n - 1]) {
      const r = layoutRows(k, 400, n)[k];
      expect(r.top + r.size / 2).toBeCloseTo(400, 10);
    }
  });

  test("between two rows, the row at round(f) contains the height", () => {
    for (let f = 0; f <= n - 1; f += 0.05) {
      const r = layoutRows(f, 400, n)[Math.round(f)];
      expect(r.top, `f = ${f}`).toBeLessThanOrEqual(400 + 1e-9);
      expect(r.top + r.size, `f = ${f}`).toBeGreaterThanOrEqual(400 - 1e-9);
    }
  });

  test("moving the focus moves no row by a jump", () => {
    let prev = layoutRows(2, 400, n);
    for (let f = 2.02; f <= 3; f += 0.02) {
      const next = layoutRows(f, 400, n);
      next.forEach((r, i) => expect(Math.abs(r.top - prev[i].top), `row ${i} at f = ${f}`).toBeLessThan(3));
      prev = next;
    }
  });

  test("a list that would cross the window is held inside it, and a taller one fills it", () => {
    const bounds = { top: 16, bottom: 784 };
    const atTop = layoutRows(0, 20, n, bounds);
    expect(atTop[0].top).toBeCloseTo(16, 10);
    const atBottom = layoutRows(n - 1, 780, n, bounds);
    expect(atBottom[n - 1].top + atBottom[n - 1].size).toBeCloseTo(784, 10);
    expect(layoutRows(3, 400, n, bounds)).toEqual(layoutRows(3, 400, n));

    const many = 40;
    for (const [f, y] of [[0, 30], [39, 770], [20, 400]]) {
      const rows = layoutRows(f, y, many, bounds);
      expect(rows[0].top, `f = ${f}`).toBeLessThanOrEqual(16 + 1e-9);
      expect(rows[many - 1].top + rows[many - 1].size, `f = ${f}`).toBeGreaterThanOrEqual(784 - 1e-9);
      const focused = rows[Math.round(f)];
      expect(focused.top, `row ${f} is on screen`).toBeGreaterThanOrEqual(16 - 1e-9);
      expect(focused.top + focused.size).toBeLessThanOrEqual(784 + 1e-9);
    }
  });
});

describe("faintOver", () => {
  const lum = (c: number[]) => {
    const [r, g, b] = c.map((v) => v / 255).map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const ratio = (a: number[], b: number[]) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
  const worst = (c: number[], ground: number[]) => Math.min(...[0, 255].map((u) => ratio(c, ground.map((v) => v * 0.95 + u * 0.05))));

  test.each([
    ["the default light page", [44, 40, 37], [250, 248, 245]],
    ["the default dark page", [232, 228, 220], [28, 25, 20]],
    ["a page that is white in both themes", [17, 17, 17], [255, 255, 255]],
    ["a light band", [40, 36, 32], [214, 226, 240]],
    ["a dark band", [240, 238, 232], [30, 50, 80]],
  ])("holds 4.6:1 on the fade over a black and over a white picture: %s", (_name, text, ground) => {
    const faint = faintOver([text[0], text[1], text[2]], [ground[0], ground[1], ground[2]])!;
    expect(worst(faint, ground)).toBeGreaterThanOrEqual(4.6);
    expect(worst(faint, ground)).toBeLessThan(4.9);
  });
  test("is fainter than the text and closer to the ground", () => {
    const faint = faintOver([44, 40, 37], [250, 248, 245])!;
    expect(faint[0]).toBeGreaterThan(44);
    expect(faint[0]).toBeLessThan(250);
  });
  test("is null where the text itself does not reach it", () => {
    expect(faintOver([40, 36, 32], [40, 60, 90])).toBeNull();
    expect(faintOver([90, 86, 80], [28, 25, 20])).toBeNull();
  });
});
