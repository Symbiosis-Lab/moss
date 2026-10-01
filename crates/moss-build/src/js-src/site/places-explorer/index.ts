/**
 * places-explorer/index.ts — boot and handshake.
 *
 * Finds the handshake element moss-build's `PlaceMapRenderContext::
 * render_term_map` leaves on a place-typed namespace root's static map
 * figure (`data-moss-places-explorer`, carrying `data-world`/`data-tiles`/
 * `data-places`/`data-labels`/`data-scope`), fetches the world SVG and the
 * explorer's data files, and — once the three REQUIRED ones are in hand —
 * hands them to `map.ts` to build the interactive layer in place of the
 * figure's static `<svg>` child. `data-labels` is the one optional URL: its
 * own fetch failing (or the attribute being absent) never blocks the map,
 * only the decorative place-name layer it feeds.
 *
 * Progressive enhancement is the whole point: the static SVG is the
 * no-JavaScript floor and stays exactly as rendered until the moment both
 * fetches resolve and the world SVG parses; a failed fetch (or malformed
 * SVG) leaves it untouched and throws nothing.
 *
 * Classes this directory creates dynamically (none exist in any emitted
 * Rust HTML, so the desktop repo's own class allowlist needs this list by
 * hand): moss-places-viewport, moss-places-world, moss-places-tiles,
 * moss-places-labels, moss-places-label, moss-places-label-dot,
 * moss-places-markers, moss-places-marker, moss-places-ring-leg,
 * moss-places-ring-dot, moss-places-controls, moss-places-capsule,
 * moss-places-control, moss-places-cards, moss-places-card-select,
 * moss-places-card-detail, moss-places-card-read, moss-places-status.
 */
import { mountPlacesMap } from "./map";
import type { LabelsData, PlacesData } from "./types";

export {};

/** Same-origin fetch, matching every other site runtime's own asset-loading guard (`search.ts`'s `loadPagefind`, the source prototype's `fetchLocalAsset`) — the handshake's URLs are always this build's own output, never a third party. */
async function fetchLocal(url: string): Promise<Response> {
  const parsed = new URL(url, location.href);
  if (parsed.origin !== location.origin) throw new Error("places explorer assets must be same-origin");
  const response = await fetch(parsed.href, { credentials: "same-origin" });
  if (!response.ok) throw new Error(`asset HTTP ${response.status}`);
  return response;
}

/** `tiles.json`'s own shape (`emit::place_map_assets::emit`'s `TileIndex`): `k`, the factor a tile is drawn at over the world's own scale (`place_map::geometry::TILE_K`), `bleed`, how far a tile's own canvas was padded past its nominal cell (`place_map::geometry::TILE_BLEED`), alongside a `[x, y]` pair per emitted regional tile — both travel with the cells they were rendered at so the runtime's own detail ceiling and placement transform can never drift from them. */
interface TileIndex {
  k: number;
  bleed: number;
  cells: Array<[number, number]>;
}

/** Exported so a test can call it directly without waiting on jsdom's `DOMContentLoaded`, which has already fired by the time a test module attaches a listener for it — the same pattern `scroll-row.ts` uses. `root` defaults to `document`; a render gate or a future second explorer on one page can pass a narrower scope. */
export async function initPlacesExplorer(root: ParentNode = document): Promise<void> {
  const figure = root.querySelector<HTMLElement>("[data-moss-places-explorer]");
  if (!figure) return;
  figure.setAttribute("data-moss-places-explorer-ready", "pending");

  const worldUrl = figure.dataset.world;
  const tilesUrl = figure.dataset.tiles;
  const placesUrl = figure.dataset.places;
  const labelsUrl = figure.dataset.labels;
  if (!worldUrl || !tilesUrl || !placesUrl) return;

  try {
    // The label layer is decorative, not core reading functionality (the
    // module doc's own framing): a missing `data-labels` attribute or a
    // failed fetch for it must never block the map itself, so it is the one
    // URL fetched outside the `Promise.all` that still gates everything
    // else — caught on its own, resolving to `null` (no labels) rather than
    // rejecting the whole boot.
    const [worldSvgText, places, tiles, labels] = await Promise.all([
      fetchLocal(worldUrl).then((response) => response.text()),
      fetchLocal(placesUrl).then((response) => response.json() as Promise<PlacesData>),
      fetchLocal(tilesUrl).then((response) => response.json() as Promise<TileIndex>),
      labelsUrl
        ? fetchLocal(labelsUrl)
            .then((response) => response.json() as Promise<LabelsData>)
            .catch((error) => {
              console.warn("[moss] places explorer labels could not load", error);
              return null;
            })
        : Promise.resolve(null),
    ]);
    const tilesBaseUrl = tilesUrl.slice(0, tilesUrl.lastIndexOf("/") + 1);
    const controller = mountPlacesMap(figure, {
      worldSvgText,
      places,
      labels,
      tileCells: tiles.cells,
      tileK: tiles.k,
      tileBleed: tiles.bleed,
      tilesBaseUrl,
      lang: document.documentElement.lang,
    });
    if (controller) figure.setAttribute("data-moss-places-explorer-ready", "ready");
    // A parse failure leaves `data-moss-places-explorer-ready="pending"` and
    // the static figure untouched — the same "nothing throws" floor a fetch
    // failure gets, below.
  } catch (error) {
    console.warn("[moss] places explorer could not load", error);
  }
}

document.addEventListener("DOMContentLoaded", () => void initPlacesExplorer());
