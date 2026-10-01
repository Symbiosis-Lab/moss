/**
 * scope.ts — the places-explorer's reading scope: everywhere, one work, or
 * one place.
 *
 * A place scope reaches one hop down: it includes a place's own direct
 * children, the only hierarchy `places.<hash>.json`'s `parent` field
 * carries (a country's page naturally includes its cities' works, because
 * each city names the country as `parent` — never a full ancestor walk, and
 * never a second, looser name-based match).
 */

import type { Place, Scope, Work } from "./types";

/** Whether `place` itself, or its own direct parent, is `scopeId`. */
function placeMatchesScope(place: Place, scopeId: string): boolean {
  return place.id === scopeId || place.parent === scopeId;
}

/** Whether `work` counts toward `scope`. */
export function inScope(work: Work, places: Place[], scope: Scope): boolean {
  if (scope.kind === "all") return true;
  if (scope.kind === "article") return work.id === scope.id;
  const byId = new Map(places.map((place) => [place.id, place]));
  return work.places.some((placeId) => {
    const place = byId.get(placeId);
    return place != null && placeMatchesScope(place, scope.id);
  });
}

/** The breadcrumb trail for `scope`: `all`, or `all > article`, or `all > parent > place` (parent omitted for a top-level place). */
export function crumbs(scope: Scope, places: Place[]): Scope[] {
  const all: Scope = { kind: "all" };
  if (scope.kind === "all") return [all];
  if (scope.kind === "article") return [all, scope];
  const place = places.find((candidate) => candidate.id === scope.id);
  const parent = place?.parent != null ? places.find((candidate) => candidate.id === place.parent) : undefined;
  return parent ? [all, { kind: "place", id: parent.id }, scope] : [all, scope];
}

/** One place reachable from `scope`, with how many in-scope works sit under it. */
export interface ScopeChild {
  place: Place;
  count: number;
}

/**
 * The places one level below `scope`: top-level places (no `parent`) for
 * `all`, or `scope`'s own direct children for a place scope. An article
 * scope is a leaf. Sorted by article count descending, then name — the
 * densest place first, ties broken alphabetically.
 */
export function childrenOf(scope: Scope, places: Place[], works: Work[]): ScopeChild[] {
  if (scope.kind === "article") return [];
  const parentId = scope.kind === "place" ? scope.id : null;
  const children = places.filter((place) => (place.parent ?? null) === parentId);
  return children
    .map((place) => ({
      place,
      count: works.filter((work) => inScope(work, places, { kind: "place", id: place.id })).length,
    }))
    .sort((a, b) => b.count - a.count || a.place.name.localeCompare(b.place.name));
}
