/**
 * state.ts — the places explorer's URL round trip.
 *
 * Three independent pieces of state live in the query string, each read
 * once at boot and written back with `history.replaceState` (never
 * `pushState` — panning the map is not a navigation a reader expects Back
 * to undo):
 *
 *   - `place=<id>` — the scope (`scope.ts`'s `Scope`). This landing only
 *     ever writes `place` or omits it (the `all` scope); `article` scope is
 *     a selection, not a scope — see below.
 *   - `article=<id>` — the selected/expanded work.
 *   - `p=patterson&z=<zoom>&x=<world-x>&y=<world-y>` — the camera, written
 *     on gesture end (never mid-drag: a replaceState per animation frame
 *     would thrash session history and the back/forward cache). `p` names
 *     the projection so a future non-Patterson build never misreads an old
 *     link's `x`/`y` as its own coordinate space; an unrecognised `p` (or a
 *     missing/non-finite `z`/`x`/`y`) makes the whole camera param set
 *     ignored, same as if none were present.
 */
import type { Camera, Scope } from "./types";

const PROJECTION = "patterson";

export interface UrlState {
  scope: Scope;
  /** Work id to select on load, or `null` for none. */
  articleId: string | null;
  /** Saved camera, or `null` when absent or malformed — the caller falls back to `coverCamera`/`fitPoints`. */
  camera: Camera | null;
}

function finiteNumber(value: string | null): number | null {
  // `Number("")` is 0, not NaN — an empty param (`x=`) would otherwise read
  // as a real, finite zero instead of the missing value it actually is.
  if (value == null || value === "") return null;
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

/** Read the explorer's own params off `location.search`. Pure except for that one read, so a test can pass a `URLSearchParams` directly. */
export function readUrlState(params: URLSearchParams = new URLSearchParams(location.search)): UrlState {
  const placeId = params.get("place");
  const scope: Scope = placeId ? { kind: "place", id: placeId } : { kind: "all" };
  const articleId = params.get("article");

  let camera: Camera | null = null;
  if (params.get("p") === PROJECTION) {
    const zoom = finiteNumber(params.get("z"));
    const x = finiteNumber(params.get("x"));
    const y = finiteNumber(params.get("y"));
    if (zoom != null && zoom > 0 && x != null && y != null) camera = { x, y, zoom };
  }

  return { scope, articleId, camera };
}

/** Write the camera to the URL, replacing whatever `p`/`z`/`x`/`y` (and nothing else) it already carried. Call on gesture end, not per frame. */
export function writeCamera(camera: Camera): void {
  const url = new URL(location.href);
  url.searchParams.set("p", PROJECTION);
  url.searchParams.set("z", camera.zoom.toFixed(3));
  url.searchParams.set("x", camera.x.toFixed(2));
  url.searchParams.set("y", camera.y.toFixed(2));
  history.replaceState(history.state, "", url);
}

/** Write (or clear) the selected work. */
export function writeSelection(articleId: string | null): void {
  const url = new URL(location.href);
  if (articleId) url.searchParams.set("article", articleId);
  else url.searchParams.delete("article");
  history.replaceState(history.state, "", url);
}

/** Write (or clear) the place scope — `all` clears `place` entirely, matching `readUrlState`'s own absent-means-`all` rule. */
export function writeScope(scope: Scope): void {
  const url = new URL(location.href);
  if (scope.kind === "place") url.searchParams.set("place", scope.id);
  else url.searchParams.delete("place");
  history.replaceState(history.state, "", url);
}
