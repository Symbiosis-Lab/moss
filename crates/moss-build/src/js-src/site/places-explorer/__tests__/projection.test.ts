/**
 * Tests for projection.ts — the places-explorer world map's coordinate
 * space: a Patterson cylindrical projection over the shared, uncropped
 * `world.svg` (not the separate per-page static map, a fixed 720x480 canvas
 * that crops the sides — see `VIEWBOX_WIDTH`/`VIEWBOX_HEIGHT` in
 * `crates/moss-build/src/build/place_map/geometry.rs`).
 *
 * The cross-check below hardcodes expected coordinates computed
 * independently from the Rust formula (`PATTERSON_K1..K4`, `patterson_y`,
 * `PattersonProjection::new`, all read from `geometry.rs`) and a scratch
 * script, not by importing `WORLD_WIDTH`/`WORLD_HEIGHT` or any helper from
 * projection.ts itself. A wrong constant, a transposed sign, or the world
 * width reverting to the per-page map's cropped 720 would all fail here,
 * where importing the module's own canvas constants into the "independent"
 * check would hide exactly that last case.
 */

import { describe, test, expect } from "vitest";
import { project, unproject } from "../projection";

// Mirrors projection.ts's own WORLD_HEIGHT as a plain number, not an import,
// so this file still has an opinion about the canvas size. WORLD_WIDTH is
// not mirrored this way: it is itself the derived quantity under test (see
// the literal `W` below), so re-deriving it here would just check the module
// against itself.
const WORLD_HEIGHT = 480;
// W = WORLD_HEIGHT * PI / patterson_y(PI / 2), computed independently to 6
// decimals (see the formula's own derivation in projection.ts's header).
const W = 842.035025;

describe("project", () => {
  test("the equator maps to the vertical middle", () => {
    expect(project(0, 0).y).toBe(WORLD_HEIGHT / 2);
    expect(project(0, 123.4).y).toBe(WORLD_HEIGHT / 2);
  });

  test("matches the Rust Patterson formula, over the full uncropped world, at three hardcoded points", () => {
    // Expected x/y below were computed independently (a scratch script, not
    // this module) from the published formula and geometry.rs's own
    // PATTERSON_K1..K4 / patterson_y, plus WORLD_WIDTH = WORLD_HEIGHT * PI /
    // patterson_y(PI/2) — not derived from this file's own WORLD_HEIGHT/W
    // above, so a wrong constant, a transposed sign, or the world reverting
    // to the per-page map's cropped 720 all fail this assertion instead of
    // silently agreeing with itself.
    const cases: Array<[number, number, number, number]> = [
      [0, 0, 421.017513, 240],
      [45, 90, 631.526269, 127.117606],
      [-30, -120, 140.339171, 312.230777],
    ];
    for (const [lat, lng, expectedX, expectedY] of cases) {
      const actual = project(lat, lng);
      expect(Math.abs(actual.x - expectedX)).toBeLessThan(1e-6);
      expect(Math.abs(actual.y - expectedY)).toBeLessThan(1e-6);
    }
  });

  test("the antimeridian and the north pole land exactly on the canvas edges, not cropped off it", () => {
    // These are the cases a reversion to the per-page map's fixed 720-wide
    // canvas gets wrong: at width 720, 180°W/180°E land inside the canvas
    // instead of exactly on x = 0 / x = W (a literal here, not imported —
    // W itself is a different, wrong number at width 720).
    const west = project(0, -180);
    expect(Math.abs(west.x - 0)).toBeLessThan(1e-6);
    expect(west.y).toBe(240);
    const east = project(0, 180);
    expect(Math.abs(east.x - W)).toBeLessThan(1e-6);
    expect(east.y).toBe(240);
    expect(Math.abs(project(90, 0).y - 0)).toBeLessThan(1e-6);
  });
});

describe("unproject", () => {
  test("round trips within 1e-6", () => {
    for (const [lat, lng] of [
      [0, 0],
      [10, 20],
      [-45, 170],
      [60, -179],
      [-89, 5],
    ] as const) {
      const { x, y } = project(lat, lng);
      const back = unproject(x, y);
      expect(Math.abs(back.lat - lat)).toBeLessThan(1e-6);
      expect(Math.abs(back.lng - lng)).toBeLessThan(1e-6);
    }
  });
});
