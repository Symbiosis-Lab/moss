/**
 * embed.ts — progressive-enhancement wiring for a `style:map` card or an
 * article's own locator, both built as a plain `<figure class="moss-place-map">`
 * host by the Rust build (`folder_embed.rs`'s `style:map` dispatch,
 * `place_map/context.rs`'s `render_locator`) and hydrated here, lazily, to
 * the canonical interactive map.
 *
 * Two independent halves share this one file because they are two ends of
 * the SAME iframe, never two different features:
 *
 * - `initPlaceEmbeds` runs on the HOST page (an article, a folder listing)
 *   and finds every host the build marked `data-moss-place-embed`. Near
 *   the viewport (or after an explicit Save-Data/slow-connection action) it creates
 *   a same-origin iframe pointed at the host's own `data-hydrate-url` —
 *   the places root, scoped by `place=`/`article=` and carrying `embed=1`
 *   — in the same stable host. A loading state remains until the iframe's
 *   own explorer posts ready; then the iframe becomes the only visible map.
 *   A failed or timed-out load restores the static SVG fallback.
 * - `attachEmbedModeIfRequested` runs INSIDE that iframe — it is the SAME
 *   `index.ts` boot, on the SAME places root page, just loaded with
 *   `embed=1` in its own URL — and switches the freshly-mounted
 *   `PlacesMapController` into the small embed's own presentation:
 *   cooperative gestures (`gestures.ts`'s own mode, not a second gesture
 *   implementation), no card for the article the embed is already inside,
 *   and a re-fit on every expand/collapse transition.
 *
 * The expand control is the existing immersive/fullscreen mechanism
 * (`immersive-mode.ts`'s `setupImmersiveIframe`, exported for exactly this
 * reuse) — not reimplemented here. Its own `onFullscreenChange` hook is what
 * tells the iframe content when it has actually been expanded or collapsed.
 * `setupImmersiveIframe`'s open-in-new-tab control is declined (its own
 * `showOpenInNewTab` parameter, passed `false`) — a places embed has no
 * canonical single-page URL the control's `data-open-url` would need to
 * carry, and the control's default corner is the one the breadcrumb chip
 * claims once expanded.
 *
 * The breadcrumb chip (`chip.ts`) is mounted by `map.ts` on every explorer,
 * this embed included, and needs no code here to work: it already reads
 * scope and the selected work directly, so an expanded locator embed shows
 * "All › This article" with no embed-specific chip logic at all; this module
 * builds no chip UI of its own. The collapsed embed hides the chip entirely (CSS, keyed off
 * `data-moss-places-embed-mode`, in `places-explorer.css`) — there is no
 * room for it in the small collapsed frame, and nothing to widen from
 * before the reader has asked to expand.
 */
import { setupImmersiveIframe } from "../immersive-mode";
import type { PlacesMapController } from "./map";
import { copyFor } from "./strings";

const READY_MESSAGE = "moss-places-embed-ready";
const MODE_MESSAGE = "moss-places-embed-mode";
const HYDRATE_TIMEOUT_MS = 8000;
const NEAR_VIEWPORT_MARGIN = "200px";
const activeHostAttempts = new WeakMap<HTMLElement, () => void>();

/** The one or two fields this module reads off `navigator.connection` — not in lib.dom.d.ts. */
interface NetworkInformationLike {
  saveData?: boolean;
  effectiveType?: string;
}

/** Save-Data, or a measured slow connection: the Review Focus case where hydration is opt-in-on-tap, never an automatic (and wasted) fetch. */
function prefersMinimalData(): boolean {
  const connection = (navigator as unknown as { connection?: NetworkInformationLike }).connection;
  if (!connection) return false;
  if (connection.saveData) return true;
  return connection.effectiveType === "slow-2g" || connection.effectiveType === "2g";
}

/**
 * Create the lazy iframe in `poster`, wire the ready handshake, and expose
 * it only after the iframe's own explorer confirms it mounted. Fetch
 * failures inside the iframe are not observable here, so timeout restores
 * the host's static SVG fallback.
 */
function buildIframe(poster: HTMLElement, hydrateUrl: string): () => void {
  const iframe = document.createElement("iframe");
  iframe.className = "moss-places-embed-frame";
  iframe.setAttribute("aria-hidden", "true"); // expose the map only after its own ready handshake
  // `data-embed-name` (`context.rs`'s `with_embed_hydration`) is the plain
  // place/article display name, never a finished sentence — this HOST page
  // may be a different locale than the embedded places root (a `style:map`
  // card inside an article translated into another language), so the
  // "Map: " wording comes from THIS page's own `<html lang>` through
  // strings.ts, same as every other piece of this explorer's UI text.
  const embedName = poster.dataset.embedName;
  if (embedName) {
    iframe.title = copyFor(document.documentElement.lang).mapEmbedTitle.replace("{name}", embedName);
  }

  let settled = false;
  let active = true;
  let timeout = 0;

  function cleanup(): void {
    window.removeEventListener("message", onMessage);
    window.clearTimeout(timeout);
  }

  function onMessage(event: MessageEvent): void {
    if (!active || settled) return;
    if (event.origin !== location.origin) return;
    if (event.source !== iframe.contentWindow) return;
    const data = event.data as { type?: unknown } | null;
    if (!data || data.type !== READY_MESSAGE) return;
    settled = true;
    cleanup();
    iframe.removeAttribute("aria-hidden");
    poster.setAttribute("data-moss-place-embed-ready", "ready");
    poster.dataset.mossPlaceEmbedState = "ready";
    poster.setAttribute("aria-busy", "false");
    poster.querySelector<HTMLElement>(":scope > .moss-places-embed-status")?.remove();
    // Hand off the accessible description: the poster figure's own
    // `role="img" aria-label="Map of X…"` (baked in by the Rust build for
    // the no-JS floor — its `<svg>` itself is already `aria-hidden`, a
    // decorative graphic even then) would otherwise still describe this
    // region as a single static image once the live, titled iframe (`title`,
    // set above) has replaced it underneath — announced twice, and
    // `role="img"` on a container can also keep some assistive tech from
    // ever reaching the iframe's own content at all. `inert` on the static
    // `<svg>` is this handoff's only remaining job: not focusable even if
    // that ever changes (theme.ts's own `.nav-links` pattern for "closed but
    // still in the DOM").
    poster.removeAttribute("role");
    poster.removeAttribute("aria-label");
    const staticFloor = poster.querySelector<SVGElement>(":scope > svg");
    staticFloor?.setAttribute("inert", "");
    // This class is the ready/pointer-events gate only; CSS does not fade
    // between two maps. The iframe is the sole visible map after READY.
    iframe.classList.add("moss-places-embed-frame--settled");
  }

  function cancel(): void {
    if (!active) return;
    active = false;
    cleanup();
    (iframe.parentElement ?? iframe).remove();
  }

  window.addEventListener("message", onMessage);
  timeout = window.setTimeout(() => {
    if (!active || settled) return;
    active = false;
    cleanup();
    // `setupImmersiveIframe` (below) always wraps the iframe before this
    // timeout can ever fire, so by now its parent is always the wrapper it
    // built — never `poster` directly. Removing just the bare iframe would
    // leave that wrapper (and its now-dangling expand control) behind, which
    // would leave a dead control in the host. Restore the static fallback.
    (iframe.parentElement ?? iframe).remove();
    poster.dataset.mossPlaceEmbedState = "fallback";
    poster.setAttribute("aria-busy", "false");
    poster.querySelector<HTMLElement>(":scope > .moss-places-embed-status")?.remove();
    poster.setAttribute("role", poster.dataset.fallbackRole ?? "img");
    if (poster.dataset.fallbackLabel) poster.setAttribute("aria-label", poster.dataset.fallbackLabel);
  }, HYDRATE_TIMEOUT_MS);

  // Attach before setting `src`: WebKit starts (and, once actually
  // connected, restarts) a navigation for a `src` set on a still-detached
  // iframe, so setting it first boots the embedded explorer twice.
  //
  // `setupImmersiveIframe` must run in this same detached window, not later
  // from the ready handler: it moves whatever iframe it is given into a new
  // wrapper element (`insertBefore` + `appendChild`), and every engine
  // re-navigates an iframe that is moved to a new parent AFTER it has
  // already loaded content. Calling it here — iframe attached to `poster`
  // but still carrying no `src` — means that move happens before the one
  // real navigation, not after it, so `src` triggers the only navigation
  // this iframe ever gets. Calling it from the ready handler (the previous
  // shape of this function) re-parented an already-loaded, already-mounted
  // iframe, which is a second real navigation in every browser — readers
  // saw the live map reset as soon as it first appeared, and in WebKit a real
  // `hover()`/`click()` during that second load hung forever waiting for
  // content that kept getting replaced out from under it.
  // `setupImmersiveIframe` itself requires `iframe.parentNode` to already
  // exist (it wraps relative to it), so `poster` is that temporary parent —
  // replaced, a line below, by the real wrapper it builds.
  poster.appendChild(iframe);
  setupImmersiveIframe(
    iframe,
    (expanded) => {
      // Where the host put its exit control is measured, not inferred from the engine: the chip inside the frame has to start clear of it.
      const control = iframe.parentElement?.querySelector(".immersive-fullscreen-btn")?.getBoundingClientRect();
      const controlAtTopLeft = expanded && !!control && control.left < window.innerWidth / 2 && control.top < window.innerHeight / 2;
      iframe.contentWindow?.postMessage({ type: MODE_MESSAGE, expanded, controlAtTopLeft }, location.origin);
    },
    false, // no open-in-new-tab control on a places embed — only expand/collapse
  );
  iframe.src = hydrateUrl;
  return cancel;
}

function setupPoster(poster: HTMLElement): void {
  if (poster.dataset.mossPlaceEmbedBound) return;
  activeHostAttempts.get(poster)?.();
  const hydrateUrl = poster.dataset.hydrateUrl;
  if (!hydrateUrl) return;
  let disposed = false;
  let observer: IntersectionObserver | null = null;
  let cancelIframe = (): void => {};
  const dispose = (): void => {
    if (disposed) return;
    disposed = true;
    observer?.disconnect();
    cancelIframe();
    if (activeHostAttempts.get(poster) === dispose) activeHostAttempts.delete(poster);
  };
  activeHostAttempts.set(poster, dispose);
  poster.dataset.mossPlaceEmbedBound = "1";

  const strings = copyFor(document.documentElement.lang);
  const name = poster.dataset.embedName;
  const fallbackRole = poster.getAttribute("role");
  const fallbackLabel = poster.getAttribute("aria-label");
  if (fallbackRole) poster.dataset.fallbackRole = fallbackRole;
  if (fallbackLabel) poster.dataset.fallbackLabel = fallbackLabel;
  poster.dataset.mossPlaceEmbedState = "waiting";
  poster.setAttribute("role", "region");
  poster.setAttribute("aria-busy", "false");
  if (name) poster.setAttribute("aria-label", strings.mapEmbedTitle.replace("{name}", name));
  const status = document.createElement("span");
  status.className = "moss-places-embed-status";
  status.setAttribute("role", "status");
  status.textContent = strings.mapEmbedWaiting;
  poster.appendChild(status);

  let hydrated = false;
  const hydrate = (): void => {
    if (disposed || hydrated) return;
    hydrated = true;
    observer?.disconnect();
    poster.dataset.mossPlaceEmbedState = "loading";
    poster.setAttribute("aria-busy", "true");
    status.textContent = strings.mapEmbedLoading;
    poster.querySelector<HTMLButtonElement>(":scope > .moss-places-embed-load")?.remove();
    cancelIframe = buildIframe(poster, hydrateUrl);
  };

  if (prefersMinimalData()) {
    poster.dataset.mossPlaceEmbedState = "consent";
    poster.setAttribute("aria-busy", "false");
    status.remove();
    const button = document.createElement("button");
    button.type = "button";
    button.className = "moss-places-embed-load";
    button.textContent = strings.mapEmbedLoad;
    if (name) button.setAttribute("aria-label", `${strings.mapEmbedLoad}: ${name}`);
    button.addEventListener("click", hydrate, { once: true });
    poster.appendChild(button);
    return;
  }
  // An article carries exactly one locator and its own scoped URL, so it
  // should start with the article itself. Folder `style:map` embeds remain
  // near-viewport lazy to avoid fetching maps for distant rows.
  const isArticleMap = new URL(hydrateUrl, location.href).searchParams.has("article");
  if (isArticleMap) {
    hydrate();
    return;
  }
  if (typeof IntersectionObserver !== "function") {
    hydrate(); // no IO support: hydrate immediately rather than never
    return;
  }
  observer = new IntersectionObserver(
    (entries) => {
      if (entries.some((entry) => entry.isIntersecting)) hydrate();
    },
    { rootMargin: NEAR_VIEWPORT_MARGIN },
  );
  observer.observe(poster);
}

/**
 * Finds every poster the build marked for lazy hydration and wires it up.
 * Idempotent per element (`data-moss-place-embed-bound`), so a caller
 * re-running it after a DOM swap never double-wires an already-bound
 * poster. A no-op on any page with no such poster — the full places root
 * included, which `index.ts` calls this alongside unconditionally.
 */
export function initPlaceEmbeds(root: ParentNode = document): void {
  for (const poster of root.querySelectorAll<HTMLElement>("[data-moss-place-embed]")) {
    setupPoster(poster);
  }
}

document.addEventListener("moss-morph-patched", () => initPlaceEmbeds());

// ---------------------------------------------------------------------------
// The iframe-content half: this is the SAME places root page `index.ts`
// always boots, just loaded with `embed=1` in its own URL.
// ---------------------------------------------------------------------------

function readEmbedParams(): { embed: boolean; place: string | null; article: string | null } {
  const params = new URLSearchParams(location.search);
  return { embed: params.get("embed") === "1", place: params.get("place"), article: params.get("article") };
}

/** Apply embed geometry before mounting: the camera immediately allocates
 * its first raster from the viewport. Applying these styles afterward queues
 * an oversized SVG raster that WebKit must finish even after the resize. */
export function prepareEmbedLayoutIfRequested(figure: HTMLElement): void {
  if (!readEmbedParams().embed) return;
  document.documentElement.setAttribute("data-moss-embed", "");
  figure.setAttribute("data-moss-places-embed-mode", "collapsed");
}

/**
 * Switches a freshly-mounted explorer into the small embed's own
 * presentation when (and only when) its own URL carries `embed=1` — a
 * no-op, synchronously, on every other page this bundle ever runs on (the
 * full places root, every render-gate fixture that isn't this one).
 * Called once, right after `mountPlacesMap` succeeds, by `index.ts`.
 */
export async function attachEmbedModeIfRequested(controller: PlacesMapController, figure: HTMLElement): Promise<void> {
  const { embed, place, article } = readEmbedParams();
  if (!embed) return;

  controller.setCooperativeGestures(true);

  // Fix scope from the URL through the existing scope state. `place=`
  // already round-trips through the ordinary boot path (`state.ts`'s
  // `readUrlState`, read by `mountPlacesMap` itself) — only an
  // `article=`-scoped embed (the locator) needs anything extra: it scopes
  // the little map to exactly that work's own point(s), the same view the
  // static locator already drew. The chip then offers "This article | All
  // articles" (`setCurrentArticle`), and the article stays off its own card
  // row (the reader is already reading it).
  if (article) {
    // `place=` rides along once a place was picked under "All articles"; the
    // article's identity must survive a reload of that state too.
    if (!place) controller.setScope({ kind: "article", id: article });
    controller.setCurrentArticle(article);
  }

  window.addEventListener("message", (event) => {
    if (event.origin !== location.origin) return;
    // Origin alone is not the sender: any same-origin window (a sibling
    // iframe, a popup) can post here too. `buildIframe`'s own ready-message
    // handler already checks `event.source` against the one window it
    // expects; this is that same symmetry on the child side — the only
    // sender this page should ever act on is its own hosting frame.
    if (event.source !== window.parent) return;
    const data = event.data as { type?: unknown; expanded?: unknown; controlAtTopLeft?: unknown } | null;
    if (!data || data.type !== MODE_MESSAGE) return;
    const expanded = Boolean(data.expanded);
    figure.setAttribute("data-moss-places-embed-mode", expanded ? "expanded" : "collapsed");
    figure.toggleAttribute("data-moss-places-host-control-top-left", Boolean(data.controlAtTopLeft));
    controller.setCooperativeGestures(!expanded);
    // The collapsed frame has no chip to return from "all articles" with.
    if (!expanded) controller.setArticleMode(true);
    controller.refitScopeIfClipped();
  });

  // Post READY once visible regional tiles are decoded and either the world
  // raster is sharp enough or fully opaque regional tile canvases cover the
  // current viewport.
  if (!(await controller.waitForInitialPaint())) return;
  window.parent.postMessage({ type: READY_MESSAGE }, location.origin);
}
