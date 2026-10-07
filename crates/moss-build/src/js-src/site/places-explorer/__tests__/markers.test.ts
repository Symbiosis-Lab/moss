/**
 * Tests for markers.ts's pure half — turning works into projected points.
 * The DOM-building half (MarkerLayer) is covered by the render gates, the
 * layer that can actually see a clicked button and a bloomed ring.
 */
import { describe, test, expect, vi } from "vitest";
import { MarkerLayer, pointsForWorks } from "../markers";
import { copyFor } from "../strings";
import { project } from "../projection";
import type { Place, Work } from "../types";

// No `: Place` annotation — these three always carry real coordinates, and
// `project(LISBON.lat, LISBON.lng)` below needs TS to know that (the
// interface's `lat`/`lng` are optional, absent on a grouping node — see
// `types.ts`'s own doc); the inferred literal type keeps them definite.
const LISBON = { id: "places/lisbon", name: "Lisbon", precision: "city", lat: 38.72, lng: -9.14 } as const satisfies Place;
const PORTO = { id: "places/porto", name: "Porto", precision: "city", lat: 41.15, lng: -8.61 } as const satisfies Place;
const COUNTRY = { id: "places/portugal", name: "Portugal", precision: "country", lat: 39.5, lng: -8.0 } as const satisfies Place;
const PLACES = [LISBON, PORTO, COUNTRY];

function work(id: string, places: string[]): Work {
  return { id, title: id, url: `/${id}/`, byline: [], authors: [], places, companions: [] };
}

describe("pointsForWorks", () => {
  test("one point per work, at its FIRST place only", () => {
    const works = [work("a", ["places/lisbon", "places/porto"])];
    const points = pointsForWorks(works, PLACES, { kind: "all" });
    expect(points).toHaveLength(1);
    const expected = project(LISBON.lat, LISBON.lng);
    expect(points[0]).toMatchObject({ id: "a", x: expected.x, y: expected.y, precision: "city" });
  });

  test("an article scope draws every place of that work, each with its own id and the work's id", () => {
    const works = [work("a", ["places/lisbon", "places/porto"]), work("b", ["places/porto"])];
    const points = pointsForWorks(works, PLACES, { kind: "article", id: "a" });
    expect(points.map((p) => p.id)).toEqual(["a", "a#1"]);
    expect(points.map((p) => p.workId)).toEqual(["a", "a"]);
    const porto = project(PORTO.lat, PORTO.lng);
    expect(points[1]).toMatchObject({ x: porto.x, y: porto.y });
  });

  test("a work whose declared places resolve to nothing is skipped", () => {
    const works = [work("ghost", ["places/nowhere"])];
    expect(pointsForWorks(works, PLACES, { kind: "all" })).toEqual([]);
  });

  test("a work with no places at all is skipped", () => {
    const works = [work("placeless", [])];
    expect(pointsForWorks(works, PLACES, { kind: "all" })).toEqual([]);
  });

  test("scope filters which works are projected at all", () => {
    const works = [work("lisbon-work", ["places/lisbon"]), work("porto-work", ["places/porto"])];
    const scoped = pointsForWorks(works, PLACES, { kind: "place", id: "places/lisbon" });
    expect(scoped.map((p) => p.id)).toEqual(["lisbon-work"]);
  });

  test("carries each work's own precision through for marker sizing", () => {
    const works = [work("a", ["places/lisbon"]), work("b", ["places/portugal"])];
    const points = pointsForWorks(works, PLACES, { kind: "all" });
    expect(points.find((p) => p.id === "a")?.precision).toBe("city");
    expect(points.find((p) => p.id === "b")?.precision).toBe("country");
  });
});

describe("MarkerLayer clustering", () => {
  test("two places of one work stay two markers however close they sit", () => {
    const container = document.createElement("div");
    const callbacks = { fitPoints: vi.fn(), focusPoint: vi.fn(), selectWork: vi.fn(), scopeRow: vi.fn(), announce: vi.fn(), maxZoom: () => 10 };
    const layer = new MarkerLayer(container, callbacks, copyFor("en"), "en");
    const points = [
      { id: "a", workId: "a", x: 100, y: 100, precision: "city" as const },
      { id: "a#1", workId: "a", x: 100.01, y: 100, precision: "city" as const },
    ];
    layer.render(points, { x: 100, y: 100, zoom: 1 }, { width: 400, height: 300 }, null, new Map([["a", work("a", [])]]));
    expect(container.querySelectorAll(".moss-places-marker")).toHaveLength(2);
  });
});

describe("MarkerLayer highlight", () => {
  test("a marker whose point carries the work's id under a suffix is not dimmed by that work's highlight", () => {
    const container = document.createElement("div");
    const callbacks = { fitPoints: vi.fn(), focusPoint: vi.fn(), selectWork: vi.fn(), scopeRow: vi.fn(), announce: vi.fn(), maxZoom: () => 10 };
    const layer = new MarkerLayer(container, callbacks, copyFor("en"), "en");
    layer.setHighlight(new Set(["a"]));
    const points = [{ id: "a#1", workId: "a", x: 100, y: 100, precision: "city" as const }];
    layer.render(points, { x: 100, y: 100, zoom: 1 }, { width: 400, height: 300 }, null, new Map([["a", work("a", [])]]));
    expect(container.querySelector(".moss-places-marker")!.hasAttribute("data-dimmed")).toBe(false);
  });
});

describe("MarkerLayer ring order", () => {
  test("an open ring's dots follow weight, the order the card row uses, not id or title", () => {
    const container = document.createElement("div");
    const callbacks = { fitPoints: vi.fn(), focusPoint: vi.fn(), selectWork: vi.fn(), scopeRow: vi.fn(), announce: vi.fn(), maxZoom: () => 1 };
    const layer = new MarkerLayer(container, callbacks, copyFor("en"), "en");
    const weighted = (id: string, weight: number): Work => ({ ...work(id, []), weight });
    const works = new Map([["a", weighted("a", 3)], ["b", weighted("b", 1)], ["c", weighted("c", 2)]]);
    const points = ["a", "b", "c"].map((id) => ({ id, workId: id, x: 100, y: 100, precision: "city" as const }));
    const camera = { x: 100, y: 100, zoom: 1 };
    const viewport = { width: 400, height: 300 };
    layer.render(points, camera, viewport, null, works);
    container.querySelector<HTMLElement>(".moss-places-marker")!.click();
    layer.render(points, camera, viewport, null, works);
    const dots = [...container.querySelectorAll(".moss-places-ring-dot")].map((dot) => dot.getAttribute("aria-label"));
    expect(dots).toEqual(["b", "c", "a"]);
  });
});
