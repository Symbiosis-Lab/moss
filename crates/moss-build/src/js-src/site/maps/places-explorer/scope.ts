/**
 * scope.ts — the places-explorer's reading scope: everywhere, one work, or
 * one place.
 *
 * A place scope reaches down through the WHOLE subtree: `places.<hash>.json`
 * emits every ancestor a used place has (`places_data.rs`'s `fold_ancestors`),
 * so a country's page includes a city's works even when a region with no
 * work of its own sits between them — each place names only its own direct
 * parent, but matching a scope walks that chain all the way up rather than
 * stopping after one hop. `parentOf` below also answers "does this place
 * resolve to a parent at all" — a place whose declared `parent` names no
 * entry in `places` (the gazetteer had no coordinates for that ancestor) is
 * treated as top-level rather than silently dropped from the menu.
 */

import type { Place, Scope, Work } from "./types";

/** `place.parent`, resolved against `places` — `undefined` both when there is no declared parent and when the declared parent names no entry in `places` (an ancestor the gazetteer had no coordinates for). Either way the caller must treat `place` as top-level: see this module's own doc. */
function parentOf(place: Place, byId: Map<string, Place>): Place | undefined {
  return place.parent != null ? byId.get(place.parent) : undefined;
}

/**
 * Whether `place` sits at or under `scopeId` — `place` itself, or any
 * ancestor along its parent chain. Bounded by `places.length` hops: the
 * chain is finite (`attach_parents`'s own `break_cycles` already fixed it to
 * one on the Rust side), so a defensive cap here only guards against a
 * malformed payload looping forever, the same posture `lies_under`'s Rust
 * counterpart takes for the same unbounded-walk risk.
 */
function placeMatchesScope(place: Place, scopeId: string, byId: Map<string, Place>): boolean {
  let current: Place | undefined = place;
  for (let hops = 0; current && hops <= byId.size; hops++) {
    if (current.id === scopeId) return true;
    current = parentOf(current, byId);
  }
  return false;
}

/** Whether `work` counts toward `scope`. */
export function inScope(work: Work, places: Place[], scope: Scope): boolean {
  if (scope.kind === "all") return true;
  if (scope.kind === "article") return work.id === scope.id;
  const byId = new Map(places.map((place) => [place.id, place]));
  return work.places.some((placeId) => {
    const place = byId.get(placeId);
    return place != null && placeMatchesScope(place, scope.id, byId);
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
 * The places one level below `scope`: top-level places for `all`, or
 * `scope`'s own direct children for a place scope. An article scope is a
 * leaf. "Top-level" is no declared `parent` OR a declared `parent` that
 * names no entry in `places` — a place whose ancestor the gazetteer had no
 * coordinates for is reachable from the root menu rather than dropped
 * (this module's own doc). Sorted by article count descending, then name —
 * the densest place first, ties broken alphabetically.
 */
export function childrenOf(scope: Scope, places: Place[], works: Work[]): ScopeChild[] {
  if (scope.kind === "article") return [];
  const byId = new Map(places.map((place) => [place.id, place]));
  const children =
    scope.kind === "place"
      ? places.filter((place) => place.parent === scope.id)
      : places.filter((place) => parentOf(place, byId) == null);
  return children
    .map((place) => ({
      place,
      count: works.filter((work) => inScope(work, places, { kind: "place", id: place.id })).length,
    }))
    .sort((a, b) => b.count - a.count || a.place.name.localeCompare(b.place.name));
}
