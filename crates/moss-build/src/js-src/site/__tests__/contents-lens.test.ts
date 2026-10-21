import { describe, expect, test } from "vitest";

import { falloff, focusAt, layoutRows, LABEL_ROW, PIC_PAD, REST_ROW, type Strip } from "../nav/contents-lens";

/** The stylesheet's 40px picture plus the padding the lens adds. */
const PIC_ROW = 40 + PIC_PAD;

const strips = (n: number, top = 300): Strip[] =>
  Array.from({ length: n }, (_, i) => ({ top: top + i * REST_ROW, height: REST_ROW }));

describe("falloff", () => {
  test("is 1 at the focus and 0 at and beyond the radius", () => {
    expect(falloff(0)).toBe(1);
    expect(falloff(2.5)).toBe(0);
    expect(falloff(7)).toBe(0);
  });
  test("is symmetric, falls monotonically, and is a cosine at half the radius", () => {
    expect(falloff(-1)).toBeCloseTo(falloff(1), 10);
    expect(falloff(0.5)).toBeGreaterThan(falloff(1));
    expect(falloff(1)).toBeGreaterThan(falloff(2));
    expect(falloff(1.25)).toBeCloseTo(0.5, 10);
  });
});

describe("focusAt", () => {
  const s = strips(6);
  test("maps the pointer to row index plus fraction, a row's centre being its index", () => {
    expect(focusAt(310, s)).toBeCloseTo(0, 10);
    expect(focusAt(300 + 2 * 20 + 10, s)).toBeCloseTo(2, 10);
    expect(focusAt(300 + 2 * 20 + 20, s)).toBeCloseTo(2.5, 10);
    expect(focusAt(300 + 2 * 20 + 15, s)).toBeCloseTo(2.25, 10);
  });
  test("is bounded by the half rows beyond the ends", () => {
    expect(focusAt(100, s)).toBe(-0.5);
    expect(focusAt(900, s)).toBe(5.5);
  });
});

describe("layoutRows", () => {
  const base = { open: 1, restTop: 300, minY: 40, maxY: 792 };
  const heights = (n: number) => Array.from({ length: n }, () => PIC_ROW);
  /** Where the point at focus `f` is drawn: the drawn top of its row plus the way through it. */
  const drawnAt = (f: number, n: number, over = {}) => {
    const input = { ...base, f, heights: heights(n), ...over };
    const rows = layoutRows(input);
    const t = f + 0.5;
    const i = Math.min(n - 1, Math.floor(t));
    const h = REST_ROW + (PIC_ROW - REST_ROW) * rows[i].m;
    const top = base.restTop + i * REST_ROW + REST_ROW / 2 + rows[i].dy - h / 2;
    return { y: top + (t - i) * h, rows };
  };

  test.each([0, 2.5, 5, 0.3, 4.7])("keeps the point at focus %s under the pointer", (f) => {
    const pointer = base.restTop + (f + 0.5) * REST_ROW;
    expect(drawnAt(f, 6).y).toBeCloseTo(pointer, 6);
  });

  test("the focused row is centred at the pointer, grown to the picture's height, and its neighbours less so", () => {
    const rows = layoutRows({ ...base, f: 2, heights: heights(6) });
    expect(rows[2].dy).toBeCloseTo(0, 10);
    expect(rows[2].m).toBe(1);
    expect(rows[1].m).toBeCloseTo(falloff(1), 10);
    expect(rows[3].m).toBeCloseTo(falloff(1), 10);
    expect(rows[1].m).toBeLessThan(rows[2].m);
    expect(rows[0].m).toBeCloseTo(falloff(2), 10);
    expect(rows[5].m).toBe(0);
  });

  test("rows above move up and rows below move down, symmetrically", () => {
    const rows = layoutRows({ ...base, f: 3, heights: heights(7) });
    for (let d = 1; d <= 3; d++) expect(rows[3 - d].dy).toBeCloseTo(-rows[3 + d].dy, 8);
    expect(rows[2].dy).toBeLessThan(0);
    expect(rows[4].dy).toBeGreaterThan(0);
  });

  test("drawn rows tile: each row starts where the last ended", () => {
    const rows = layoutRows({ ...base, f: 2.3, heights: heights(7) });
    for (let j = 0; j < 6; j++) {
      const h = (k: number) => REST_ROW + (PIC_ROW - REST_ROW) * rows[k].m;
      const gap = rows[j + 1].dy + REST_ROW - rows[j].dy - (h(j) + h(j + 1)) / 2;
      expect(gap).toBeCloseTo(0, 6);
    }
  });

  test("a row with no picture grows only to the label's height", () => {
    const mixed = layoutRows({ ...base, f: 1, heights: [PIC_ROW, LABEL_ROW, PIC_ROW, PIC_ROW] });
    expect(mixed[1].m).toBe(1);
    const grown = REST_ROW + (LABEL_ROW - REST_ROW) * mixed[1].m;
    expect(grown).toBe(LABEL_ROW);
  });

  test("closed, nothing moves", () => {
    const rows = layoutRows({ ...base, open: 0, f: 2, heights: heights(6) });
    expect(rows.every((r) => r.dy === 0 && r.m === 0)).toBe(true);
  });

  test("clamps at the top: the open list stays below minY", () => {
    const rows = layoutRows({ ...base, f: -0.5, restTop: 50, minY: 48, heights: heights(6) });
    expect(50 + rows[0].dy - (REST_ROW + (PIC_ROW - REST_ROW) * rows[0].m) / 2 + REST_ROW / 2).toBeGreaterThanOrEqual(48 - 1e-9);
  });

  test("clamps at the bottom: the open list stays above maxY", () => {
    const n = 6;
    const restTop = base.maxY - n * REST_ROW;
    const rows = layoutRows({ ...base, f: n - 0.5, restTop, heights: heights(n) });
    const last = rows[n - 1];
    const h = REST_ROW + (PIC_ROW - REST_ROW) * last.m;
    const bottom = restTop + (n - 1) * REST_ROW + last.dy + REST_ROW / 2 + h / 2;
    expect(bottom).toBeLessThanOrEqual(base.maxY + 1e-9);
  });

  test("with forty rows the growth is capped to the room there is, and nothing leaves the window", () => {
    const n = 40;
    const restTop = 40;
    const rows = layoutRows({ ...base, f: 20, restTop, minY: 40, maxY: 40 + n * REST_ROW + 30, heights: heights(n) });
    const grown = rows.reduce((a, r) => a + (PIC_ROW - REST_ROW) * r.m, 0);
    expect(grown).toBeLessThanOrEqual(30 + 1e-9);
    expect(rows[20].m).toBeGreaterThan(0);
  });

  test("all open at once: every row full size, the list centred", () => {
    const n = 6;
    const rows = layoutRows({ ...base, f: 0, all: true, heights: heights(n) });
    expect(rows.every((r) => r.m === 1)).toBe(true);
    expect(rows[0].dy).toBeCloseTo(-rows[n - 1].dy, 8);
    expect(rows[1].dy - rows[0].dy).toBeCloseTo(PIC_ROW - REST_ROW, 8);
  });
});
