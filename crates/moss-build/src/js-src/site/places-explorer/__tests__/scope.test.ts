/**
 * Tests for scope.ts — the places-explorer's reading scope: everywhere, one
 * work, or one place (and its direct children).
 *
 * Fixture data is synthetic, not drawn from any real site: a two-level
 * place hierarchy (a country with two cities) and three works, one of which
 * has no location at all.
 */

import { describe, test, expect } from "vitest";
import { inScope, crumbs, childrenOf } from "../scope";
import type { Place, Work } from "../types";

const country: Place = { id: "places/freedonia", name: "Freedonia", precision: "country", lat: 10, lng: 10 };
const cityA: Place = { id: "places/alpha", name: "Alpha", parent: "places/freedonia", precision: "city", lat: 10.1, lng: 10.1 };
const cityB: Place = { id: "places/beta", name: "Beta", parent: "places/freedonia", precision: "city", lat: 10.2, lng: 10.2 };
const places: Place[] = [country, cityA, cityB];

function work(id: string, placeIds: string[]): Work {
  return { id, title: id, url: `/${id}`, byline: [], places: placeIds, companions: [] };
}

const workAlpha = work("alpha-piece", ["places/alpha"]);
const workBeta = work("beta-piece", ["places/beta"]);
const workCountry = work("country-piece", ["places/freedonia"]);
const workUnlocated = work("unlocated-piece", []);
const works: Work[] = [workAlpha, workBeta, workCountry, workUnlocated];

describe("inScope", () => {
  test("'all' includes every work", () => {
    for (const w of works) expect(inScope(w, places, { kind: "all" })).toBe(true);
  });

  test("'article' matches only that work's own id", () => {
    expect(inScope(workAlpha, places, { kind: "article", id: "alpha-piece" })).toBe(true);
    expect(inScope(workBeta, places, { kind: "article", id: "alpha-piece" })).toBe(false);
  });

  test("'place' matches a work located directly at that place", () => {
    expect(inScope(workAlpha, places, { kind: "place", id: "places/alpha" })).toBe(true);
    expect(inScope(workBeta, places, { kind: "place", id: "places/alpha" })).toBe(false);
  });

  test("'place' also includes a work located at a child place", () => {
    expect(inScope(workAlpha, places, { kind: "place", id: "places/freedonia" })).toBe(true);
    expect(inScope(workBeta, places, { kind: "place", id: "places/freedonia" })).toBe(true);
    expect(inScope(workCountry, places, { kind: "place", id: "places/freedonia" })).toBe(true);
  });

  test("a work with no locations matches only 'all'", () => {
    expect(inScope(workUnlocated, places, { kind: "all" })).toBe(true);
    expect(inScope(workUnlocated, places, { kind: "place", id: "places/freedonia" })).toBe(false);
  });
});

describe("crumbs", () => {
  test("'all' is just itself", () => {
    expect(crumbs({ kind: "all" }, places)).toEqual([{ kind: "all" }]);
  });

  test("an article scope is all > article", () => {
    expect(crumbs({ kind: "article", id: "alpha-piece" }, places)).toEqual([
      { kind: "all" },
      { kind: "article", id: "alpha-piece" },
    ]);
  });

  test("a top-level place scope is all > place", () => {
    expect(crumbs({ kind: "place", id: "places/freedonia" }, places)).toEqual([
      { kind: "all" },
      { kind: "place", id: "places/freedonia" },
    ]);
  });

  test("a child place scope is all > parent > place", () => {
    expect(crumbs({ kind: "place", id: "places/alpha" }, places)).toEqual([
      { kind: "all" },
      { kind: "place", id: "places/freedonia" },
      { kind: "place", id: "places/alpha" },
    ]);
  });
});

describe("childrenOf", () => {
  test("'all' lists top-level places, sorted by article count then name", () => {
    const children = childrenOf({ kind: "all" }, places, works);
    expect(children.map((child) => child.place.id)).toEqual(["places/freedonia"]);
    // freedonia's own count includes its children's works (one hop), so it's 3.
    expect(children[0].count).toBe(3);
  });

  test("a place scope lists its direct children, denser first", () => {
    const children = childrenOf({ kind: "place", id: "places/freedonia" }, places, works);
    expect(children.map((child) => child.place.id)).toEqual(["places/alpha", "places/beta"]);
    expect(children.every((child) => child.count === 1)).toBe(true);
  });

  test("a leaf place has no children", () => {
    expect(childrenOf({ kind: "place", id: "places/alpha" }, places, works)).toEqual([]);
  });

  test("an article scope has no children", () => {
    expect(childrenOf({ kind: "article", id: "alpha-piece" }, places, works)).toEqual([]);
  });
});
