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

  test("a place whose declared parent names no entry in `places` is treated as top-level, not dropped", () => {
    // `places_data.rs` can only emit an ancestor the gazetteer has
    // coordinates for (`fold_ancestors`'s own doc); a place whose immediate
    // parent has none still carries the dangling id. The menu must still
    // offer it rather than silently losing it.
    const orphan: Place = { id: "places/gamma", name: "Gamma", parent: "places/nowhere", precision: "city", lat: 1, lng: 1 };
    const orphanWork = work("gamma-piece", ["places/gamma"]);
    const children = childrenOf({ kind: "all" }, [...places, orphan], [...works, orphanWork]);
    expect(children.map((child) => child.place.id)).toContain("places/gamma");
  });
});

describe("a three-level hierarchy whose middle place has no work of its own", () => {
  // Country -> Region -> City, with the work located only at City. Region
  // (the middle place) still gets its own entry in `places`
  // (`places_data.rs`'s `fold_ancestors`) but no work ever names it
  // directly — exactly the 31-of-56 shape a real site had.
  const country2: Place = { id: "places/arcadia", name: "Arcadia", precision: "country", lat: 20, lng: 20 };
  const region: Place = { id: "places/midland", name: "Midland", parent: "places/arcadia", precision: "region", lat: 20.1, lng: 20.1 };
  const city: Place = { id: "places/leafburg", name: "Leafburg", parent: "places/midland", precision: "city", lat: 20.2, lng: 20.2 };
  const places3: Place[] = [country2, region, city];
  const cityWork = work("leafburg-piece", ["places/leafburg"]);
  const works3: Work[] = [cityWork];

  test("the root menu offers the top-level country, with the leaf's count rolled all the way up", () => {
    const children = childrenOf({ kind: "all" }, places3, works3);
    expect(children.map((child) => child.place.id)).toEqual(["places/arcadia"]);
    expect(children[0].count).toBe(1);
  });

  test("digging into the country reveals the work-less region, with the same rolled-up count", () => {
    const children = childrenOf({ kind: "place", id: "places/arcadia" }, places3, works3);
    expect(children.map((child) => child.place.id)).toEqual(["places/midland"]);
    expect(children[0].count).toBe(1);
  });

  test("digging into the region reveals the leaf city", () => {
    const children = childrenOf({ kind: "place", id: "places/midland" }, places3, works3);
    expect(children.map((child) => child.place.id)).toEqual(["places/leafburg"]);
    expect(children[0].count).toBe(1);
  });

  test("the country scope includes the leaf's work two hops down", () => {
    expect(inScope(cityWork, places3, { kind: "place", id: "places/arcadia" })).toBe(true);
  });
});

describe("a grouping node with no coordinates of its own", () => {
  // `places_data.rs`'s `fold_ancestors` emits one of these for an ancestor
  // it had to invent — no gazetteer row at all, or a row with no
  // coordinates — purely so the chip menu can still dig through it.
  // `lat`/`lng`/`precision` are absent (see `types.ts`'s own `Place` doc),
  // which none of `inScope`/`crumbs`/`childrenOf` ever reads: this pins
  // that the whole scope module works off `id`/`name`/`parent` alone and
  // never needs a point to walk, group or count through one.
  const country3: Place = { id: "places/borealia", name: "Borealia" };
  const region3: Place = { id: "places/tundra", name: "Tundra", parent: "places/borealia" };
  const city3: Place = { id: "places/frostport", name: "Frostport", parent: "places/tundra", precision: "city", lat: 60, lng: 60 };
  const places4: Place[] = [country3, region3, city3];
  const frostportWork = work("frostport-piece", ["places/frostport"]);
  const works4: Work[] = [frostportWork];

  test("it appears in the root menu with the rolled-up count", () => {
    const children = childrenOf({ kind: "all" }, places4, works4);
    expect(children.map((child) => child.place.id)).toEqual(["places/borealia"]);
    expect(children[0].count).toBe(1);
  });

  test("digging into it reveals the next grouping node, still coordinate-less", () => {
    const children = childrenOf({ kind: "place", id: "places/borealia" }, places4, works4);
    expect(children.map((child) => child.place.id)).toEqual(["places/tundra"]);
    expect(children[0].place.lat).toBeUndefined();
  });

  test("the scope reaches the leaf's work through two coordinate-less hops", () => {
    expect(inScope(frostportWork, places4, { kind: "place", id: "places/borealia" })).toBe(true);
  });

  test("the breadcrumb trail names it like any other place", () => {
    expect(crumbs({ kind: "place", id: "places/tundra" }, places4)).toEqual([
      { kind: "all" },
      { kind: "place", id: "places/borealia" },
      { kind: "place", id: "places/tundra" },
    ]);
  });
});
