/**
 * places-explorer/index.ts — stub entry point for the places-explorer
 * interactive layer.
 *
 * This is the first landing (build-side wiring only): it finds the
 * handshake element moss-build's `PlaceMapRenderContext::render_term_map`
 * leaves on a place-typed namespace root's static map figure
 * (`data-moss-places-explorer`, carrying `data-world`/`data-tiles`/
 * `data-places`/`data-scope`) and marks it pending. Nothing visual, and
 * nothing reads `data-world`/`data-tiles`/`data-places` yet — the actual
 * world-pan/tile-detail/place-pin runtime lands in a later task.
 *
 * A directory, not a single file, because that runtime will need several
 * modules; this stub is deliberately the only one so far.
 */
export {};

/** Exported so the test can call it directly without waiting on jsdom's
 * `DOMContentLoaded`, which has already fired by the time a test module
 * attaches a listener for it — the same pattern `scroll-row.ts` uses. */
export function initPlacesExplorer(): void {
  const el = document.querySelector<HTMLElement>("[data-moss-places-explorer]");
  if (!el) return;
  el.setAttribute("data-moss-places-explorer-ready", "pending");
}

document.addEventListener("DOMContentLoaded", initPlacesExplorer);
