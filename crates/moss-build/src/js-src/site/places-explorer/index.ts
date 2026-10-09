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
 * This same module is also what boots when this bundle is loaded a SECOND
 * way: as the small embed `embed.ts`'s own lazy iframe points at (a
 * `style:map` card, an article's own locator), where this page's URL
 * carries `embed=1` — `attachEmbedModeIfRequested` below switches the
 * freshly-mounted controller into that presentation. The HOST page side of
 * that same embed (finding and lazily hydrating the posters) is
 * `initPlaceEmbeds`, called unconditionally alongside this module's own
 * boot since the two are independent: a page can carry an embed poster, the
 * explorer root figure, both, or neither.
 *
 * Classes this directory creates dynamically (none exist in any emitted
 * Rust HTML, so the desktop repo's own class allowlist needs this list by
 * hand): moss-places-viewport, moss-places-world, moss-places-world-surface,
 * moss-places-rivers, moss-places-tiles, moss-places-tile,
 * moss-places-labels, moss-places-label, moss-places-label-dot,
 * moss-places-markers, moss-places-marker, moss-places-ring-leg,
 * moss-places-ring-dot, moss-places-controls, moss-places-capsule,
 * moss-places-control, moss-places-chip, moss-places-chip-trail,
 * moss-places-chip-crumb, moss-places-chip-sep, moss-places-chip-chevron,
 * moss-places-chip-collapsible, moss-places-chip-ellipsis,
 * moss-places-chip-menu, moss-places-chip-menu-item,
 * moss-places-chip-menu-count, moss-places-cards, moss-places-card-select,
 * moss-places-card-detail, moss-places-card-read, moss-places-status,
 * moss-places-embed-frame.
 */
import { mountPlacesMap } from "./map";
import { attachEmbedModeIfRequested, initPlaceEmbeds, prepareEmbedLayoutIfRequested } from "./embed";
import { normalizePlacesData, type LabelsData, type PlacesDataWire } from "./types";

export {};

/**
 * Same-origin fetch, matching every other site runtime's own asset-loading
 * guard (`search.ts`'s `loadPagefind`, the source prototype's
 * `fetchLocalAsset`) — the handshake's URLs are always this build's own
 * output, never a third party.
 *
 * `cache: "force-cache"`: every caller's URL is content-hashed
 * (`world.<hash>.svg`, `places.<hash>.json`, `tiles.json` under the same
 * `_moss/map.<hash>/` directory) — a changed byte is a changed URL, so a
 * cached response is correct to reuse unconditionally rather than only
 * when a server's own `Cache-Control` says so. This is what makes "pay
 * once per visit" (a second located-article page's own embed never
 * re-fetches `world.svg`) hold regardless of what the HOSTING server
 * sends — a bare static file server included, which a render gate's own
 * scratch site is.
 */
async function fetchLocal(url: string): Promise<Response> {
  const parsed = new URL(url, location.href);
  if (parsed.origin !== location.origin) throw new Error("places explorer assets must be same-origin");
  const response = await fetch(parsed.href, { credentials: "same-origin", cache: "force-cache" });
  if (!response.ok) throw new Error(`asset HTTP ${response.status}`);
  return response;
}

/** `tiles.json`'s own shape (`emit::place_map_assets::emit`'s `TileIndex`): `k`, the factor a tile is drawn at over the world's own scale (`place_map::geometry::TILE_K`), `origins`, each tile's canvas top-left in whole canvas units keyed `"x,y"`, `columns` and `rows`, the grid's size in cells, alongside a `[x, y]` pair per emitted regional tile — all travel with the cells they were rendered at so the runtime's own detail ceiling and placement transform can never drift from them. */
interface TileIndex {
  k: number;
  columns: number;
  rows: number;
  cells: Array<[number, number]>;
  origins: Record<string, [number, number]>;
}

/** Exported so a test can call it directly without waiting on jsdom's `DOMContentLoaded`, which has already fired by the time a test module attaches a listener for it — the same pattern `scroll-row.ts` uses. `root` defaults to `document`; a render gate or a future second explorer on one page can pass a narrower scope. */
export async function initPlacesExplorer(root: ParentNode = document): Promise<void> {
  const figure = root.querySelector<HTMLElement>("[data-moss-places-explorer]");
  if (!figure) return;
  // Idempotency guard: WebKit has been observed firing `DOMContentLoaded`
  // twice for a document loaded into an embed's own lazily-created iframe
  // (never for a plain top-level navigation to this same page) — a second
  // `mountPlacesMap` on an already-mounting figure would replace its DOM out
  // from under the first run mid-flight, which is what surfaced as a render
  // gate's `hover()` call hanging forever (the element it resolved kept
  // getting torn down and rebuilt). This attribute is the one piece of
  // state every call shares, so checking it before touching anything else
  // makes a second, redundant call a no-op regardless of why it happened.
  if (figure.dataset.mossPlacesExplorerReady) return;
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
    const [worldSvgText, placesWire, tiles, labels] = await Promise.all([
      fetchLocal(worldUrl).then((response) => response.text()),
      fetchLocal(placesUrl).then((response) => response.json() as Promise<PlacesDataWire>),
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
    const places = normalizePlacesData(placesWire);
    const tilesBaseUrl = tilesUrl.slice(0, tilesUrl.lastIndexOf("/") + 1);
    prepareEmbedLayoutIfRequested(figure);
    const controller = mountPlacesMap(figure, {
      worldSvgText,
      places,
      labels,
      tileCells: tiles.cells,
      tileK: tiles.k,
      tileOrigins: tiles.origins,
      tileColumns: tiles.columns,
      tileRows: tiles.rows,
      tilesBaseUrl,
      lang: document.documentElement.lang,
    });
    if (controller) {
      figure.setAttribute("data-moss-places-explorer-ready", "ready");
      // A no-op unless THIS page's own URL carries embed=1 — see embed.ts's
      // own doc for why that is the same places root page, not a second one.
      attachEmbedModeIfRequested(controller, figure);
    }
    // A parse failure leaves `data-moss-places-explorer-ready="pending"` and
    // the static figure untouched — the same "nothing throws" floor a fetch
    // failure gets, below.
  } catch (error) {
    console.warn("[moss] places explorer could not load", error);
  }
}

document.addEventListener("DOMContentLoaded", () => {
  void initPlacesExplorer();
  // Independent of the explorer root above: a `style:map` embed or an
  // article's own locator poster can appear on a page that carries no
  // `[data-moss-places-explorer]` figure at all (an ordinary article), and
  // a no-op on a page that carries neither.
  initPlaceEmbeds();
});
