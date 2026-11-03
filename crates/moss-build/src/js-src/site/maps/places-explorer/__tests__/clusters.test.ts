/**
 * Tests for clusters.ts — grouping nearby points into one marker, and laying
 * a coincident group's members out on a ring when zooming can never pull
 * them apart.
 */

import { describe, test, expect } from "vitest";
import {
  CLUSTER_DISTANCE,
  RING_MAX,
  mergeByProximity,
  clusterVisible,
  staysMergedAtMaxZoom,
  separationZoom,
  ringZoom,
  ringLayout,
} from "../clusters";

const CEILING = 7.21; // a representative detailMaxZoom() value; clusters.ts takes it as a parameter rather than computing it

describe("mergeByProximity", () => {
  test("merges two points within the threshold into one cluster", () => {
    const clusters = mergeByProximity(
      [
        { id: "a", screen: { x: 0, y: 0 } },
        { id: "b", screen: { x: 1, y: 0 } },
      ],
      43
    );
    expect(clusters.length).toBe(1);
    expect(clusters[0].ids.sort()).toEqual(["a", "b"]);
  });

  test("keeps two points past the threshold in separate clusters", () => {
    const clusters = mergeByProximity(
      [
        { id: "a", screen: { x: 0, y: 0 } },
        { id: "b", screen: { x: 50, y: 0 } },
      ],
      43
    );
    expect(clusters.length).toBe(2);
  });

  test("three collinear points cluster the same way regardless of input order", () => {
    // Spaced so the greedy pass is order-sensitive if it trusts array order:
    // a-b and b-c are each within the threshold, but a-c is not, so whoever
    // the pass visits first claims the middle point and decides whether the
    // far point joins or starts its own cluster.
    const a = { id: "a", screen: { x: 0, y: 0 } };
    const b = { id: "b", screen: { x: 30, y: 0 } };
    const c = { id: "c", screen: { x: 60, y: 0 } };
    const threshold = 43;
    const orderings = [
      [a, b, c],
      [c, b, a],
      [b, a, c],
      [c, a, b],
    ];
    const results = orderings.map((items) =>
      mergeByProximity(items, threshold)
        .map((cluster) => [...cluster.ids].sort().join("|"))
        .sort()
    );
    for (const result of results) {
      expect(result).toEqual(results[0]);
    }
  });
});

describe("clusterVisible / staysMergedAtMaxZoom", () => {
  test("two points 1px apart (world units) stay merged at the detail ceiling, and ring", () => {
    const points = [
      { id: "a", x: 500, y: 280 },
      { id: "b", x: 500 + 1 / CEILING, y: 280 },
    ];
    expect(staysMergedAtMaxZoom(points, CEILING)).toBe(true);
    expect(clusterVisible(points, CEILING, CLUSTER_DISTANCE).length).toBe(1);
    const ring = ringLayout(points.length, { x: 500, y: 280 });
    expect(ring.positions).not.toBeNull();
    expect(ring.positions).toHaveLength(2);
  });

  test("two points 50 screen px apart at the ceiling separate", () => {
    const points = [
      { id: "a", x: 500, y: 280 },
      { id: "b", x: 500 + 50 / CEILING, y: 280 },
    ];
    expect(staysMergedAtMaxZoom(points, CEILING)).toBe(false);
    expect(clusterVisible(points, CEILING, CLUSTER_DISTANCE).length).toBe(2);
  });

  test("a single point is trivially merged", () => {
    expect(staysMergedAtMaxZoom([{ id: "a", x: 0, y: 0 }], CEILING)).toBe(true);
  });
});

describe("separationZoom", () => {
  test("a single point needs no separation zoom", () => {
    expect(separationZoom([{ id: "a", x: 0, y: 0 }], 1, CEILING)).toBe(1);
  });

  test("finds the zoom at which two nearby points separate", () => {
    const points = [
      { id: "a", x: 500, y: 280 },
      { id: "b", x: 510, y: 280 },
    ];
    // At zoom 1 (world units == screen px here) they are 10px apart, well
    // under CLUSTER_DISTANCE; the search must climb before they separate.
    expect(clusterVisible(points, 1, CLUSTER_DISTANCE).length).toBe(1);
    const zoom = separationZoom(points, 1, CEILING);
    expect(zoom).toBeGreaterThan(1);
    expect(zoom).toBeLessThanOrEqual(CEILING);
    expect(clusterVisible(points, zoom, CLUSTER_DISTANCE).length).toBe(2);
  });

  test("returns the floor unchanged when points can never separate under the ceiling", () => {
    const points = [
      { id: "a", x: 500, y: 280 },
      { id: "b", x: 500, y: 280 },
    ];
    expect(separationZoom(points, 1, CEILING)).toBe(1);
  });
});

describe("ringZoom", () => {
  test("finds the smallest zoom at which the target group has no outside neighbours", () => {
    const target = [
      { id: "a", x: 500, y: 280 },
      { id: "b", x: 500, y: 280 },
    ];
    const neighbour = { id: "c", x: 520, y: 280 };
    const zoom = ringZoom(target, [...target, neighbour], 1, CEILING);
    const clusters = clusterVisible([...target, neighbour], zoom, CLUSTER_DISTANCE);
    const targetCluster = clusters.find((cluster) => cluster.ids.includes("a"));
    expect(targetCluster?.ids.sort()).toEqual(["a", "b"]);
  });
});

describe("ringLayout", () => {
  test("a 9-member group returns a count and no ring", () => {
    const layout = ringLayout(9, { x: 0, y: 0 });
    expect(layout.count).toBe(9);
    expect(layout.positions).toBeNull();
  });

  test("ring order is deterministic across calls", () => {
    const first = ringLayout(5, { x: 10, y: 20 });
    const second = ringLayout(5, { x: 10, y: 20 });
    expect(first.positions).toEqual(second.positions);
  });

  test("ring positions are equidistant from the centre", () => {
    const center = { x: 10, y: 20 };
    const layout = ringLayout(RING_MAX, center);
    expect(layout.positions).not.toBeNull();
    const distances = layout.positions!.map((point) => Math.hypot(point.x - center.x, point.y - center.y));
    for (const distance of distances) {
      expect(Math.abs(distance - distances[0])).toBeLessThan(1e-9);
    }
  });
});
