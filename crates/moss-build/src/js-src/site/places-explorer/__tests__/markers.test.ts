/**
 * Tests for markers.ts's pure half — turning works into projected points.
 * The DOM-building half (MarkerLayer) is covered by the render gates, the
 * layer that can actually see a clicked button and a bloomed ring.
 */
import { describe, test, expect } from "vitest";
import { pointsForWorks } from "../markers";
import { project } from "../projection";
import type { Place, Work } from "../types";

const LISBON: Place = { id: "places/lisbon", name: "Lisbon", precision: "city", lat: 38.72, lng: -9.14 };
const PORTO: Place = { id: "places/porto", name: "Porto", precision: "city", lat: 41.15, lng: -8.61 };
const COUNTRY: Place = { id: "places/portugal", name: "Portugal", precision: "country", lat: 39.5, lng: -8.0 };
const PLACES = [LISBON, PORTO, COUNTRY];

function work(id: string, places: string[]): Work {
  return { id, title: id, url: `/${id}/`, byline: [], places, companions: [] };
}

describe("pointsForWorks", () => {
  test("one point per work, at its FIRST place only", () => {
    const works = [work("a", ["places/lisbon", "places/porto"])];
    const points = pointsForWorks(works, PLACES, { kind: "all" });
    expect(points).toHaveLength(1);
    const expected = project(LISBON.lat, LISBON.lng);
    expect(points[0]).toMatchObject({ id: "a", x: expected.x, y: expected.y, precision: "city" });
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
