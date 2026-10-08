/**
 * map.ts — the controller: the world layer, DOM scaffold, and the camera
 * transform.
 *
 * Owns the one mutable `Camera` the whole explorer reads and writes
 * (`gestures.ts`, `markers.ts`'s callbacks, card selection, the URL layer)
 * and the single `applyCamera` that turns it into the `.moss-places-world`
 * CSS transform, the river custom property, and a re-render of markers,
 * tiles and cards. Regional tiles are `tiles.ts`'s own `TileLayer`,
 * constructed once here and driven by `applyCamera` the same way
 * `markers.ts`'s `MarkerLayer` is — everything else in this directory is a
 * module `mountPlacesMap` wires together, not a second place that touches
 * the DOM this one owns.
 *
 * The world layer used to be the fetched world SVG, parsed and inlined
 * live, with its filters (relief shading, the coast halo) re-run by the
 * browser on every repaint of the pan/zoom transform above it — measured
 * costing Chromium whole frames and WebKit whole SECONDS. Neither engine's
 * cost was really about the FILTER's own parameters (a spike here halving
 * blur radii changed nothing in WebKit); it was about a LIVE, filtered element sitting inside a transformed subtree. This file
 * now builds that layer as a fixed-pixel canvas instead (`raster.ts`'s
 * `splitMapSvg`/`rasterizeOrFallback`), composited once and then only ever
 * moved by the SAME transform, with the world's own box permanently
 * promoted to its own compositor layer (`places-explorer.css`) rather than
 * only for the span of a gesture — nothing left for either engine to
 * re-invalidate on a pan or a zoom click. `tiles.ts`'s `TileLayer` does the
 * same for each regional tile, baking its lighting and band shadows once
 * when that tile is decoded.
 *
 * Two things the live SVG let CSS drive no longer can, because a
 * rasterised resource has no access to the page's own custom properties:
 * rivers (which must keep a constant ON-SCREEN width while the world
 * scales under them) stay a separate, live, UNFILTERED overlay — cheap,
 * since it is thin strokes, not fills with a shadow filter on every band —
 * and the old continuous `--moss-place-relief-strength` fade, which is gone
 * outright: the world and tiles bake the same lighting and tint ladder
 * into their fixed-pixel canvases, so hand-over changes resolution, not
 * terrain treatment.
 */
import {
  clampCamera,
  coverCamera,
  detailMaxZoom,
  openingMaxZoom,
  fitPoints,
  fitWork,
  MIN_ZOOM,
  resizeCamera,
  screenScale,
  tileDetailMaxZoom,
  worldToScreen,
} from "./camera";
import { CardRow, worksForRow } from "./cards";
import { ScopeChip } from "./chip";
import { LabelLayer } from "./labels";
import { MarkerLayer, pointsForWorks, workIdOf } from "./markers";
import { project, WORLD_HEIGHT, WORLD_WIDTH } from "./projection";
import { rasterizeOrFallback, splitMapSvg } from "./raster";
import { inScope } from "./scope";
import { copyFor } from "./strings";
import { attachGestures } from "./gestures";
import { TileLayer, tileFadeOpacity } from "./tiles";
import * as urlState from "./state";
import { hasPoint, type Camera, type LabelsData, type PlacesData, type Place, type Point, type Rect, type Scope, type Viewport } from "./types";

/**
 * The world raster stays sharp up to this many zoom-ones past the cover
 * floor before it is left to go soft under whatever tiles cover that area
 * — baking all the way to the world's own (let alone the tile-raised)
 * ceiling would mean a raster several times the linear size of the
 * viewport sitting in memory for the entire session just to cover a zoom
 * level most views never reach. Named rather than inlined so the actual
 * trade this makes is visible at the call site.
 */
const WORLD_RASTER_ZOOM_CAP = 1.6;
/** Device pixel ratio honoured up to this for the world raster — a 3x phone gains nothing from tripling an already roomy budget. */
const WORLD_RASTER_DPR_CAP = 2;
/** A rebake only fires once the zoom that would drive it has grown past the last bake by this ratio — without a deadband, panning at a steady zoom (which never needs a sharper texture) would still schedule a decode on every settle. */
const WORLD_RASTER_REBAKE_RATIO = 1.3;
/**
 * How long a settle waits with no FURTHER settle before a resolution
 * rebake actually starts — several rapid zoom-in clicks each reset this,
 * so only the final, truly-at-rest settle pays the decode cost, not every
 * one that led to it. Measured load-bearing: three real, back-to-back
 * clicks with no debounce each queued their own rebake, and the filter
 * pipeline behind an SVG image decode and full-canvas relief/lighting pass
 * was slow enough in WebKit that the SECOND click's own rebake was still
 * running when the THIRD click fired, delaying that click past a second —
 * an ablation (removing the rebake call entirely) confirmed it as the
 * cause, holding every other change fixed. Never applies to the very
 * first bake (`scheduleWorldRebake`'s own check) — that one has nothing to
 * debounce against, and the reader is waiting to see anything at all.
 */
const WORLD_REBAKE_DEBOUNCE_MS = 250;

function buildControlButton(control: "zoom-in" | "zoom-out" | "reset", label: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "moss-places-control";
  button.dataset.control = control;
  button.setAttribute("aria-label", label);
  return button;
}

export interface MountOptions {
  worldSvgText: string;
  tilesBaseUrl: string;
  tileCells: Array<[number, number]>;
  /** The factor a tile is drawn at over the world's own scale (`tiles.json`'s own `k`, the build's `TILE_K`) — read off the build, never hardcoded here, so the runtime's own detail ceiling and placement transform can never drift from what a tile was actually rendered at. */
  tileK: number;
  /** Each tile's canvas origin keyed `"x,y"`, in whole canvas units (`tiles.json`'s own `origins`) — read off the build for the same reason `tileK` is. */
  tileOrigins: Record<string, [number, number]>;
  /** The tile grid's size in cells (`tiles.json`'s own `columns` and `rows`) — read off the build so the draw order follows the grid the emitter cut. */
  tileColumns: number;
  tileRows: number;
  places: PlacesData;
  /** `labels.json`'s own parsed body — `null`/`undefined` when the handshake carried no `data-labels` or that fetch failed; the label layer then simply never places anything (progressive enhancement, same posture as a missing locator). */
  labels?: LabelsData | null;
  lang: string;
}

export interface PlacesMapController {
  /** The scope seam: `all` or `place` this landing, the breadcrumb chip's own dig-down included. Re-fits the camera and re-renders. */
  setScope(scope: Scope): void;
  /** The element gestures attach to and `getBoundingClientRect` sizes against — exposed so `embed.ts` can toggle cooperative-gesture mode and observe resizes without map.ts knowing anything about embeds. */
  readonly viewportEl: HTMLElement;
  /** Waits for the initial viewport's actual paint after embed scope and size are applied. */
  waitForInitialPaint(): Promise<boolean>;
  /** Switch `gestures.ts` between its ordinary mode (a lone touch pans the map, the only mode the full explorer page ever uses) and cooperative mode (a lone touch defers to the page's own scroll; two fingers pan as well as pinch) — see `gestures.ts`'s own `GestureCallbacks.cooperative` doc. Also reflects the mode onto the viewport as `data-gesture-mode="cooperative"` for `places-explorer.css`'s own `touch-action` rule. */
  setCooperativeGestures(enabled: boolean): void;
  /** Re-clamp is already automatic on every resize (the `ResizeObserver`/`window.resize` listeners below re-run `applyCamera`, which re-clamps zoom/pan around the UNCHANGED camera centre). What is not automatic: a resize that changes the viewport's aspect ratio sharply enough — the embed's own expand/collapse transition, far larger than an ordinary window resize — can leave the current scope's own points outside the new frame even though the camera centre didn't move. Call after such a transition settles; a no-op when every in-scope point is still in view. */
  refitScopeIfClipped(): void;
  /** Name the article this map is the embed of: it never appears in the card row (the reader is already reading it), its marker carries `data-current` while every article is shown, and the chip offers the "This article | All articles" switch. `null` clears it; defaults to none. */
  setCurrentArticle(workId: string | null): void;
  /** Show only the current article (`true`) or every article (`false`). Leaving "all" remembers its scope and camera, and coming back restores both. A no-op without a current article. */
  setArticleMode(articleOnly: boolean): void;
}

/** Build the interactive layer in place of `figure`'s static `<svg>` child and wire every gesture, selection and scope path together. `null` (leaving the static figure untouched) when the fetched world SVG fails to parse. The world's own first raster decodes in the background (`rebakeWorld`) rather than gating this return — see that call's own comment. */
export function mountPlacesMap(figure: HTMLElement, options: MountOptions): PlacesMapController | null {
  const strings = copyFor(options.lang);
  const worldSplit = splitMapSvg(options.worldSvgText);
  if (!worldSplit) return null;
  // Pulled out of `worldSplit` by name, not read through it, so the closures
  // below (defined once, called later — `rebakeWorld`'s own async body) see
  // a type TypeScript can't widen back to nullable the way it does a field
  // read off a captured `const` of an object type.
  const worldBase = worldSplit.base;
  const worldBaseMarkup = worldSplit.baseMarkup;

  // `--moss-place-figure-top` is the one number places-explorer.css cannot
  // derive on its own (see its own doc on the figure's `block-size`): the
  // header's rendered height, which varies with viewport width (nav
  // wrapping) and content (a folding breadcrumb), so CSS floors the
  // figure's height at `100svh` minus THIS figure's own current top
  // offset rather than at a guessed constant. Read at mount and again on
  // every resize (below) — a stale value is what used to push the card
  // row below the fold at rest.
  function syncFigureOffset(): void {
    figure.style.setProperty("--moss-place-figure-top", `${figure.getBoundingClientRect().top}px`);
  }
  syncFigureOffset();

  const viewportEl = document.createElement("div");
  viewportEl.className = "moss-places-viewport";
  viewportEl.tabIndex = 0;
  viewportEl.setAttribute("role", "application");
  viewportEl.setAttribute("aria-label", strings.map);

  const worldEl = document.createElement("div");
  worldEl.className = "moss-places-world";
  if (worldSplit.rivers) {
    worldSplit.rivers.classList.add("moss-places-rivers");
    worldEl.append(worldSplit.rivers);
  }

  const tilesEl = document.createElement("div");
  tilesEl.className = "moss-places-tiles";
  worldEl.append(tilesEl);

  // ---- world raster -------------------------------------------------------
  // The world's own decoded surface: built once at mount (fired from the
  // first settle below, not awaited — see that call's own comment) and
  // re-decoded at a sharper size on a later settle, up to
  // `WORLD_RASTER_ZOOM_CAP`, debounced by `scheduleWorldRebake` — never
  // during a gesture, and never blocking one. `worldBakePromise` keeps two
  // decodes from overlapping if two scheduled rebakes still somehow land
  // close together, sharing the one in flight instead of racing a second.
  /** CSS px per world unit the current raster was baked for (`unitScale * zoom`), NOT the zoom alone: `zoom` is relative to the viewport's own cover scale, so the same zoom means a 4x larger raster once an embed goes fullscreen. */
  let worldBakedScale = 0;
  let worldBakePromise: Promise<void> | null = null;
  let worldSurfaceEl: HTMLCanvasElement | SVGSVGElement | null = null;
  let worldRelease: () => void = () => {};
  let worldRebakeTimer: ReturnType<typeof setTimeout> | null = null;

  function worldBakeIsSharpEnough(unitScale: number, zoom: number): boolean {
    const targetZoom = Math.min(Math.max(zoom, MIN_ZOOM), WORLD_RASTER_ZOOM_CAP);
    return worldBakedScale > 0 && unitScale * targetZoom <= worldBakedScale * WORLD_RASTER_REBAKE_RATIO;
  }

  /** Resolves once the world raster is sharp enough for `zoom` — a no-op returning the already-resolved past bake once it is. */
  function rebakeWorld(unitScale: number, zoom: number): Promise<void> {
    if (worldBakePromise) return worldBakePromise;
    const targetZoom = Math.min(Math.max(zoom, MIN_ZOOM), WORLD_RASTER_ZOOM_CAP);
    const targetScale = unitScale * targetZoom;
    if (worldBakeIsSharpEnough(unitScale, zoom)) return Promise.resolve();
    worldBakePromise = (async () => {
      try {
        const dpr = Math.min(window.devicePixelRatio || 1, WORLD_RASTER_DPR_CAP);
        const pixelWidth = WORLD_WIDTH * unitScale * targetZoom * dpr;
        const pixelHeight = WORLD_HEIGHT * unitScale * targetZoom * dpr;
        const surface = await rasterizeOrFallback(worldBaseMarkup, worldBase, pixelWidth, pixelHeight);
        surface.el.classList.add("moss-places-world-surface");
        // Always the very first child: whatever has already been inserted
        // (the rivers overlay, the tiles container) stays on top of it,
        // painted-order-wise, exactly like the fetched world SVG used to
        // sit under both before this module existed.
        worldEl.insertBefore(surface.el, worldEl.firstChild);
        worldSurfaceEl?.remove();
        worldRelease();
        worldSurfaceEl = surface.el;
        worldRelease = surface.release;
        worldBakedScale = targetScale;
        // A settle that landed mid-bake got this promise back and has already spent its debounce, so nothing else re-checks the scale it saw.
        scheduleWorldRebake();
      } finally {
        worldBakePromise = null;
      }
    })();
    return worldBakePromise;
  }

  /**
   * Call on every settle instead of `rebakeWorld` directly. The very first
   * bake (nothing to debounce against, and the reader is waiting to see
   * anything at all) fires immediately; every later one waits
   * `WORLD_REBAKE_DEBOUNCE_MS` with no further settle first, restarting the
   * wait on each new one — see that constant's own doc for the rapid-click
   * failure this debounce exists to prevent. `camera`/`getViewport` are
   * read at the moment the timer actually fires, not when it was
   * scheduled, so a camera that kept moving during the wait still bakes
   * for where it ended up, not where it was when the timer was set.
   */
  function scheduleWorldRebake(): void {
    const fire = (): void => {
      worldRebakeTimer = null;
      const viewport = getViewport();
      if (viewport.width <= 0 || viewport.height <= 0) return;
      const unitScale = screenScale({ x: 0, y: 0, zoom: 1 }, viewport);
      void rebakeWorld(unitScale, camera.zoom);
    };
    if (worldBakedScale === 0) {
      fire();
      return;
    }
    if (worldRebakeTimer !== null) clearTimeout(worldRebakeTimer);
    worldRebakeTimer = setTimeout(fire, WORLD_REBAKE_DEBOUNCE_MS);
  }

  // Above the map, below the markers — DOM order alone gives it that
  // stacking (no z-index needed, matching every sibling layer here), and
  // places-explorer.css hides the whole layer while `.moss-places-world`
  // carries `[data-gesture]`, so a stale position is never visible mid-pan.
  const labelsEl = document.createElement("div");
  labelsEl.className = "moss-places-labels";
  labelsEl.setAttribute("aria-hidden", "true");

  const markersEl = document.createElement("div");
  markersEl.className = "moss-places-markers";

  const controlsEl = document.createElement("div");
  controlsEl.className = "moss-places-controls";
  controlsEl.setAttribute("role", "group");
  controlsEl.setAttribute("aria-label", strings.mapControls);
  const capsule = document.createElement("div");
  capsule.className = "moss-places-capsule";
  capsule.setAttribute("role", "group");
  const zoomInBtn = buildControlButton("zoom-in", strings.zoomIn);
  const zoomOutBtn = buildControlButton("zoom-out", strings.zoomOut);
  capsule.append(zoomInBtn, zoomOutBtn);
  const resetBtn = buildControlButton("reset", strings.reset);
  controlsEl.append(capsule, resetBtn);

  const cardsEl = document.createElement("div");
  cardsEl.className = "moss-places-cards";
  cardsEl.setAttribute("role", "list");
  cardsEl.setAttribute("aria-label", strings.worksInView);

  const chipEl = document.createElement("nav");
  chipEl.className = "moss-places-chip";

  const statusEl = document.createElement("p");
  statusEl.className = "moss-places-status";
  statusEl.setAttribute("role", "status");
  statusEl.setAttribute("aria-live", "polite");

  viewportEl.append(worldEl, labelsEl, markersEl, controlsEl, chipEl, cardsEl);
  figure.replaceChildren(viewportEl, statusEl);

  // ---- state -----------------------------------------------------------
  const worksById = new Map(options.places.works.map((work) => [work.id, work]));
  const placesById = new Map(options.places.places.map((place) => [place.id, place]));
  const tileLayer = new TileLayer(tilesEl, {
    tilesBaseUrl: options.tilesBaseUrl,
    availableTiles: options.tileCells,
    k: options.tileK,
    origins: options.tileOrigins,
    columns: options.tileColumns,
    rows: options.tileRows,
  });
  const initial = urlState.readUrlState();

  let scope: Scope = initial.scope;
  let selectedId: string | null = initial.articleId;
  let scopedIds: Set<string> | null = null;
  /** `setCurrentArticle`'s own state — see that method's doc. */
  let currentArticleId: string | null = null;
  /** The "all" side of the article switch, saved when leaving it, with the viewport its camera was framed in (`Camera.zoom` is relative to that viewport's cover scale). */
  let savedAll: { scope: Scope; camera: Camera; viewport: Viewport } | null = null;
  /** Set when the article is re-framed before the host has resized the frame (the embed's collapse): the next real size change re-fits instead of keeping the fullscreen scale. */
  let refitOnResize = false;

  function allPoints(): Point[] {
    return pointsForWorks(options.places.works, options.places.places, { kind: "all" });
  }

  function getViewport(): Viewport {
    const rect = viewportEl.getBoundingClientRect();
    return { width: rect.width, height: rect.height };
  }

  function currentMaxZoom(viewport: Viewport): number {
    return tileLayer.hasVisibleTiles(camera, viewport)
      ? tileDetailMaxZoom(viewport)
      : detailMaxZoom(viewport);
  }

  function announce(message: string): void {
    statusEl.textContent = message;
  }

  const markerLayer = new MarkerLayer(
    markersEl,
    {
      fitPoints(points) {
        const viewport = getViewport();
        camera = fitPoints(points, viewport, currentMaxZoom(viewport), freeFrame(viewport));
        applyCamera(true);
      },
      focusPoint(point, zoom) {
        const viewport = getViewport();
        camera = clampCamera({ x: point.x, y: point.y, zoom }, viewport, currentMaxZoom(viewport));
        applyCamera(true);
      },
      selectWork,
      scopeRow(ids) {
        scopedIds = ids;
        applyCamera(true);
      },
      announce,
      maxZoom: () => currentMaxZoom(getViewport()),
    },
    strings,
    options.lang,
  );

  const cardRow = new CardRow(cardsEl, { selectWork }, strings);
  const labelLayer = new LabelLayer(labelsEl, options.labels, options.lang);

  /** Boxes a label must never cover beyond the markers themselves, in viewport-relative CSS px — the zoom controls, the breadcrumb scope chip, the card row, and the chip's own dig-down menu while it is open. `:empty` card rows collapse to zero size on their own (`places-explorer.css`), so an empty one reserves nothing without a separate check here; a closed menu is simply absent from the DOM, the same "not there, so nothing to measure" shape. The menu is `chip.ts`'s own `<div class="moss-places-chip-menu">`, an absolutely-positioned CHILD of `chipEl` appended outside its own trail (see chip.ts's `openMenu`) — `chipEl`'s own `getBoundingClientRect()` covers only the trail's box, never a child positioned outside it, so the menu needs its own entry here rather than being already included in `relative(chipEl)`. */
  function reservedLabelRects(): Rect[] {
    const origin = viewportEl.getBoundingClientRect();
    const relative = (el: Element | null): Rect | null => {
      if (!el) return null;
      const box = el.getBoundingClientRect();
      if (box.width <= 0 || box.height <= 0) return null;
      return { x: box.x - origin.x, y: box.y - origin.y, width: box.width, height: box.height };
    };
    // The card row's own box is a tall, mostly empty band an opened card
    // grows up into; what it covers is the cards themselves.
    // An opened card is left out while any collapsed one is there: fits are
    // framed against the resting row, not against the card that happens to be open.
    const cards = [...cardsEl.children];
    const resting = cards.filter((card) => card.getAttribute("aria-current") !== "true");
    const cardRects = (resting.length ? resting : cards).map(relative).filter((rect): rect is Rect => rect != null);
    const cardsBox = cardRects.length
      ? {
          x: Math.min(...cardRects.map((r) => r.x)),
          y: Math.min(...cardRects.map((r) => r.y)),
          width: Math.max(...cardRects.map((r) => r.x + r.width)) - Math.min(...cardRects.map((r) => r.x)),
          height: Math.max(...cardRects.map((r) => r.y + r.height)) - Math.min(...cardRects.map((r) => r.y)),
        }
      : null;
    return [
      relative(controlsEl),
      cardsBox,
      relative(chipEl),
      relative(chipEl.querySelector(".moss-places-chip-menu")),
    ].filter((rect): rect is Rect => rect != null);
  }

  /**
   * The inner rectangle every camera fit (the initial cover, a scope
   * change, a selection, a ring opening) frames points INTO, instead of the
   * whole viewport — the same three overlays `reservedLabelRects` already
   * reports, generalized from "don't draw a label here" to "don't frame a
   * marker here". A reserved rect counts against the TOP edge when its own
   * vertical centre sits in the viewport's top half (the chip, the
   * controls), against the BOTTOM edge otherwise (the card row); this is a
   * band subtraction, not a general rectangle-minus-rectangles cutout, which
   * is all three overlays ever need since none of them sits mid-viewport.
   * Falls back to the whole viewport if the bands would invert (a viewport
   * too short for both reserved bands at once — better an occasional
   * marker-under-overlay than a negative-size fit).
   */
  function freeFrame(viewport: Viewport): Rect {
    let top = 0;
    let bottom = viewport.height;
    for (const rect of reservedLabelRects()) {
      const center = rect.y + rect.height / 2;
      if (center < viewport.height / 2) top = Math.max(top, rect.y + rect.height);
      else bottom = Math.min(bottom, rect.y);
    }
    if (bottom <= top) return { x: 0, y: 0, width: viewport.width, height: viewport.height };
    return { x: 0, y: top, width: viewport.width, height: bottom - top };
  }

  /** Every point a work's own places project to — `selectWork`'s, `fitForScope`'s `article` branch's, and the mount-time initial fit's one shared source, so all three land on exactly the same camera for the same work (a multi-place work included: EVERY one of `work.places`, not just the first). */
  function workPoints(id: string | null): Point[] {
    const work = id ? worksById.get(id) : undefined;
    return (work?.places ?? [])
      .map((placeId) => placesById.get(placeId))
      .filter((place): place is Place => place != null)
      .filter(hasPoint)
      .map((place) => project(place.lat, place.lng));
  }

  /** The camera the current `scope` fits to, within `frame` — shared by the initial mount, the re-fit once the card row has real content (both below), and `setScope`'s own fit for every later scope change. An `article` scope (the embed/full-page article locator) fits that one work's own places with `fitWork` — the same REGIONAL framing a marker or card click on that work lands at (`selectWork`, below), never the world's own detail ceiling. A plain `all` scope with a work already selected (the full-page `?article=` locator, which never sets a scope of its own — `state.ts`'s own doc: article is a selection, not a scope) fits that selection the same way, so the only page that ever reaches this branch with a selection still opens framed on it. */
  function fitForScope(viewport: Viewport, frame?: Rect): Camera {
    if (scope.kind === "article" || scope.kind !== "place") {
      const points = workPoints(scope.kind === "article" ? scope.id : selectedId);
      return points.length ? fitWorkCamera(points, viewport, frame && workFrame(viewport)) : fitAllCamera(viewport, frame);
    }
    const points = pointsForWorks(options.places.works, options.places.places, scope);
    return points.length
      ? fitPoints(points, viewport, detailMaxZoom(viewport), frame)
      : fitAllCamera(viewport, frame);
  }

  /** The collapsed embed is too small to give up any of itself to overlays, so a work's fit uses the whole frame there. */
  function workFrame(viewport: Viewport): Rect | undefined {
    return viewportEl.closest('[data-moss-places-embed-mode="collapsed"]') ? undefined : freeFrame(viewport);
  }

  /** A work's fit may go as deep as the tile ceiling only where tiles cover the fit's own target; elsewhere it stops at the world's detail ceiling, since deeper would only upscale the world image. */
  function fitWorkCamera(points: Point[], viewport: Viewport, frame: Rect | undefined): Camera {
    const deep = fitWork(points, viewport, frame, tileDetailMaxZoom(viewport));
    return tileLayer.hasVisibleTiles(deep, viewport) ? deep : fitWork(points, viewport, frame, detailMaxZoom(viewport));
  }

  /** Set once the tiles under the opening view have decoded (or never, if they fail): until then the opening view stays within the world layer's own resolution, since past it the world image is a stretched, low-poly picture. */
  let openingTilesReady = false;

  /** The opening and reset view of every place: fitted to their bounds, so places in one region open on that region. Places spread wider than the viewport fall back to the cover view panned to their densest window. */
  function fitAllCamera(viewport: Viewport, frame?: Rect): Camera {
    const points = allPoints();
    // One frame for both cameras, so comparing their zooms compares like with like (the collapsed embed gives up no part of itself to overlays).
    const fitFrame = frame && workFrame(viewport);
    const cover = coverCamera(points, viewport, fitFrame);
    if (!points.length) return cover;
    // Past the world layer's ceiling only once tiles can draw it, and never past the depth where the bundled data still looks clean.
    const world = fitWork(points, viewport, fitFrame, detailMaxZoom(viewport));
    const deep = openingTilesReady ? fitWork(points, viewport, fitFrame, openingMaxZoom(viewport)) : world;
    const fitted = tileLayer.hasVisibleTiles(deep, viewport) ? deep : world;
    return fitted.zoom > cover.zoom ? fitted : cover;
  }

  function selectWork(id: string | null): void {
    // On an article locator the article's own work is the page's subject: a
    // click on its dot re-fits to it (the same fit it opened on) instead of
    // toggling the selection off with the camera left wherever it was.
    const keepSelected = scope.kind === "article" && scope.id === id;
    const next = selectedId === id && !keepSelected ? null : id;
    selectedId = next;
    if (next) {
      const points = workPoints(next);
      if (points.length) {
        const viewport = getViewport();
        camera = fitWorkCamera(points, viewport, workFrame(viewport));
      }
      const work = worksById.get(next);
      if (work) announce(work.title || strings.untitled);
    }
    urlState.writeSelection(selectedId);
    applyCamera(true);
    cardRow.revealSelected(selectedId);
  }

  /** `restored`: a camera to keep instead of the fit `next` would get. */
  function setScope(next: Scope, restored?: Camera): void {
    scope = next;
    scopedIds = null;
    markerLayer.closeRing();
    urlState.writeScope(next);
    const viewport = getViewport();
    camera = restored ?? fitForScope(viewport, freeFrame(viewport));
    applyCamera(true);
  }

  // The chip's own hover/focus highlight: exempt one child place's works
  // from the ring's dimming attribute without opening a ring — `inScope`
  // (not `pointsForWorks`) because only membership is needed here, never a
  // projected point.
  function highlightPlace(placeId: string | null): void {
    markerLayer.setHighlight(
      placeId == null
        ? null
        : new Set(
            options.places.works
              .filter((work) => inScope(work, options.places.places, { kind: "place", id: placeId }))
              .map((work) => work.id),
          ),
    );
    applyCamera(false);
  }

  const scopeChip = new ScopeChip(
    chipEl,
    // `applyCamera(true)` re-runs the label layer against a freshly-read
    // `reservedLabelRects()` — the one way a label clear of every OTHER
    // reserved box still gets hidden once the menu opens over it, and
    // shown again once it closes; `scopeChip.render()`'s own key guard
    // makes the rest of this a no-op (scope/selection/ring/locale are
    // exactly what opening or closing the menu never changes), so the
    // open menu this is called FROM survives its own trigger untouched.
    { setScope, setArticleMode: (articleOnly) => setArticleMode(articleOnly, false), highlightPlace, menuToggled: () => applyCamera(true) },
    strings,
    options.lang,
  );

  // The saved camera (`?p=patterson&z&x&y`) always wins over a scope fit —
  // a reader who panned/zoomed and copied the link gets exactly that view
  // back, not the scope's own re-fit. Only a `place` scope with no saved
  // camera fits to its own points; `all` and a missing/malformed save both
  // fall back to the plain cover camera. The fit itself uses the plain
  // world ceiling (never `currentMaxZoom`, which reads `camera` — not yet
  // initialized here): no tile has had a chance to matter before the first
  // real `applyCamera(true)` call right below re-clamps against it.
  // No `frame` here: the card row has not rendered anything yet (the first
  // `applyCamera(true)` call, at the bottom of this function, is what
  // populates it), so `freeFrame` would read it as empty and reserve
  // nothing. The re-fit right after that first call corrects this once the
  // row's real height is known.
  let camera: Camera = initial.camera ?? fitForScope(getViewport());
  // An embed mounted while hidden fits against a 0x0 frame (a cover camera);
  // the first real size, from the resize paths, re-fits it.
  /** The viewport size `camera` was last applied at; see `onResize`. */
  let lastViewport: Viewport = { width: 0, height: 0 };
  let fitPending = !initial.camera && (getViewport().width <= 0 || getViewport().height <= 0);

  // ---- camera application -------------------------------------------------
  let gestureFrame: number | null = null;
  function applyCamera(settled: boolean): void {
    if (settled && gestureFrame !== null) {
      cancelAnimationFrame(gestureFrame);
      gestureFrame = null;
    }
    const viewport = getViewport();
    if (viewport.width <= 0 || viewport.height <= 0) return;
    lastViewport = viewport;
    if (fitPending) {
      fitPending = false;
      camera = fitForScope(viewport, freeFrame(viewport));
    }
    // Never force a zoom OUT here: every path that actually zooms IN (wheel,
    // pinch, the capsule buttons, fitPoints/focusPoint) already clamps its
    // own candidate against `currentMaxZoom` at its own call site, before
    // this re-clamp ever runs — so by the time a camera reaches here, its
    // zoom is already within whatever ceiling was correct when it was
    // chosen. A pure pan (drag, keyboard arrows) never changes zoom at all;
    // `currentMaxZoom` depends on `camera` (`tileLayer.hasVisibleTiles`), so
    // recomputing it AFTER the pan, once a reader has dragged off the tile
    // patch that justified a raised ceiling, would otherwise snap the zoom
    // back down mid-drag. `Math.max` keeps this clamp doing its other job
    // (the position/floor clamp below, which still needs a real scale) while
    // never lowering a zoom that was already valid.
    camera = clampCamera(camera, viewport, Math.max(currentMaxZoom(viewport), camera.zoom));
    const unitScale = screenScale({ x: camera.x, y: camera.y, zoom: 1 }, viewport);
    const scale = screenScale(camera, viewport);
    worldEl.style.width = `${WORLD_WIDTH * unitScale}px`;
    worldEl.style.height = `${WORLD_HEIGHT * unitScale}px`;
    const translateX = -(camera.x - WORLD_WIDTH / 2) * scale;
    const translateY = -(camera.y - WORLD_HEIGHT / 2) * scale;
    worldEl.style.transform = `translate(calc(-50% + ${translateX}px), calc(-50% + ${translateY}px)) scale(${camera.zoom})`;

    const ceiling = currentMaxZoom(viewport);
    figure.style.setProperty("--moss-place-river-scale", String(Math.min(1, MIN_ZOOM / camera.zoom)));

    tileLayer.render(camera, viewport, unitScale, settled);

    const visiblePoints = pointsForWorks(options.places.works, options.places.places, scope).filter((point) => {
      const screen = worldToScreen(point, camera, viewport);
      return screen.x >= -22 && screen.x <= viewport.width + 22 && screen.y >= -22 && screen.y <= viewport.height + 22;
    });
    markerLayer.render(visiblePoints, camera, viewport, selectedId, worksById, currentArticleId);

    zoomInBtn.disabled = camera.zoom >= ceiling;
    zoomOutBtn.disabled = camera.zoom <= MIN_ZOOM;

    if (settled) {
      // `data-gesture` no longer drives the world layer's own compositor
      // promotion (`places-explorer.css` promotes it permanently now — see
      // that file's own comment for why), but it still gates the label
      // layer's visibility below and the crispness gate's own assertion
      // that a finished gesture clears it.
      worldEl.removeAttribute("data-gesture");
      // Never during a gesture, and never blocking this settle: a sharper
      // world texture is worth decoding once the camera stops moving, not
      // worth stalling the frame that proves it stopped.
      scheduleWorldRebake();
      const visibleIds = new Set(visiblePoints.map(workIdOf));
      let rows = worksForRow(options.places.works, visibleIds, scopedIds, selectedId);
      if (currentArticleId) rows = rows.filter((work) => work.id !== currentArticleId);
      cardRow.render(rows, options.places.places, selectedId);
      labelLayer.render(visiblePoints, camera, viewport, worksById, placesById, reservedLabelRects());
      urlState.writeCamera(camera);
      scopeChip.render(scope, options.places.places, options.places.works, markerLayer.ringCount(), currentArticleId);
    }
  }

  // ---- gestures ------------------------------------------------------------
  let cooperativeGestures = false;
  attachGestures(viewportEl, {
    getCamera: () => camera,
    getViewport,
    setCamera(next) {
      camera = next;
      // Keep the camera authoritative immediately, but paint at most once per
      // frame when wheel or pointer events arrive faster than the display.
      if (gestureFrame === null) {
        gestureFrame = requestAnimationFrame(() => {
          gestureFrame = null;
          applyCamera(false);
        });
      }
    },
    onSettle: () => applyCamera(true),
    onGestureStart: () => worldEl.setAttribute("data-gesture", ""),
    maxZoom: () => currentMaxZoom(getViewport()),
    cooperative: () => cooperativeGestures,
    // The same deselect a card or marker click on the open work performs; `selectWork(null)` leaves the camera alone.
    onDismiss: () => {
      if (selectedId) selectWork(null);
    },
  });

  // One Escape closes exactly the innermost open thing: a cluster ring, else the
  // breadcrumb's dig-down menu, else the open card. Document-level so it fires
  // wherever focus sits: a clicked marker does not reliably take focus in every
  // engine (WebKit does not focus a button on click). The card, unlike the ring
  // and menu, is only dismissed when the key reached the map or nothing holds
  // focus (closing the ring removes the dot that had it, leaving the body).
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    if (markerLayer.hasOpenRing()) markerLayer.closeRing();
    else if (!scopeChip.dismissMenu()) {
      const inMap = event.target === document.body || (event.target instanceof Node && viewportEl.contains(event.target));
      if (selectedId && inMap) selectWork(null);
      return;
    }
    event.preventDefault();
  });

  function setCooperativeGestures(enabled: boolean): void {
    cooperativeGestures = enabled;
    if (enabled) viewportEl.dataset.gestureMode = "cooperative";
    else delete viewportEl.dataset.gestureMode;
  }

  function refitScopeIfClipped(): void {
    const viewport = getViewport();
    if (viewport.width <= 0 || viewport.height <= 0) return;
    const points = pointsForWorks(options.places.works, options.places.places, scope);
    if (!points.length) return;
    const MARGIN = 22; // matches applyCamera's own visiblePoints margin
    const clipped = points.some((point) => {
      const screen = worldToScreen(point, camera, viewport);
      return screen.x < -MARGIN || screen.x > viewport.width + MARGIN || screen.y < -MARGIN || screen.y > viewport.height + MARGIN;
    });
    if (clipped) {
      camera = fitPoints(points, viewport, currentMaxZoom(viewport));
      applyCamera(true);
    }
  }

  function setCurrentArticle(workId: string | null): void {
    currentArticleId = workId;
    applyCamera(true);
  }

  /** `deferRefit` arms the re-fit for the next resize: only the embed's collapse message wants it (the host resizes the frame right after), a chip click is followed by no resize. */
  function setArticleMode(articleOnly: boolean, deferRefit: boolean): void {
    if (!currentArticleId || articleOnly === (scope.kind === "article")) return;
    if (articleOnly) {
      savedAll = { scope, camera, viewport: getViewport() };
      selectedId = currentArticleId;
      setScope({ kind: "article", id: currentArticleId });
      refitOnResize = deferRefit;
    } else {
      // Every article shows no selection: the current one is marked by its own ring instead.
      selectedId = null;
      setScope(savedAll?.scope ?? { kind: "all" }, savedAll ? resizeCamera(savedAll.camera, savedAll.viewport, getViewport()) : undefined);
    }
    announce(articleOnly ? strings.scopeThisArticle : strings.scopeAllArticles);
  }

  zoomInBtn.addEventListener("click", () => {
    const viewport = getViewport();
    camera = clampCamera({ ...camera, zoom: camera.zoom * 1.25 }, viewport, currentMaxZoom(viewport));
    applyCamera(true);
  });
  zoomOutBtn.addEventListener("click", () => {
    const viewport = getViewport();
    camera = clampCamera({ ...camera, zoom: camera.zoom / 1.25 }, viewport, currentMaxZoom(viewport));
    applyCamera(true);
  });
  resetBtn.addEventListener("click", () => {
    markerLayer.closeRing();
    scopedIds = null;
    const viewport = getViewport();
    camera = fitAllCamera(viewport, freeFrame(viewport));
    applyCamera(true);
  });

  // A resize keeps the scale (px per degree) and centre, so a bigger
  // viewport shows more map rather than the same range magnified
  // (`resizeCamera`'s own doc). `lastViewport` is the size the camera was
  // last applied at — updated by `applyCamera` itself, so a camera fitted
  // for a new size is never "resized" a second time by the observer callback
  // that follows it.
  function onResize(): void {
    const viewport = getViewport();
    const resized = viewport.width !== lastViewport.width || viewport.height !== lastViewport.height;
    if (refitOnResize && resized && viewport.width > 0 && viewport.height > 0) {
      refitOnResize = false;
      camera = fitForScope(viewport, freeFrame(viewport));
    } else {
      camera = resizeCamera(camera, lastViewport, viewport);
    }
    applyCamera(true);
  }
  if (typeof ResizeObserver === "function") {
    new ResizeObserver(onResize).observe(viewportEl);
  }
  // The header's own height can change with the viewport width (nav
  // wrapping to a second row, the breadcrumb fold) — re-measure the
  // figure's offset before every resize's own re-fit, not just at mount.
  window.addEventListener("resize", () => {
    syncFigureOffset();
    onResize();
  });

  // The settle below starts the world's own first bake (`rebakeWorld`) in
  // the background — deliberately NOT awaited here. Gating "ready" (and so
  // the gesture handlers already wired above) on that decode was measured
  // costing WebKit most of a second on top of an otherwise unchanged mount
  // — exactly the interactive-delay the goal this module serves asks to
  // avoid. The reader instead sees the viewport's own background colour
  // for one brief moment (the same gap the static floor's own first-paint
  // comment already covers) before the raster pops in.
  applyCamera(true);
  // The card row has real content now (this call's own `cardRow.render`),
  // so `freeFrame` can finally see its true height. Re-fit once against it
  // — never for a saved camera from the URL, which already won above and
  // must keep winning — so "at rest" never leaves a marker under the row
  // the very first camera guess, made before anything had rendered into
  // it, could not have known to avoid.
  if (!initial.camera) {
    const viewport = getViewport();
    camera = fitForScope(viewport, freeFrame(viewport));
    applyCamera(true);
    // The deeper opening fit waits for the tiles under this first view. If a clear supersedes that wait, retry only while the captured camera and viewport are still current; a failure keeps the shallow view.
    const opening = { ...camera };
    const openingViewport = getViewport();
    void (async () => {
      let result = await tileLayer.waitForVisibleTiles();
      while (result === "superseded") {
        const viewport = getViewport();
        if (camera.x !== opening.x || camera.y !== opening.y || camera.zoom !== opening.zoom || viewport.width !== openingViewport.width || viewport.height !== openingViewport.height) return;
        result = await tileLayer.waitForVisibleTiles();
      }
      if (result !== "ready") return;
      const latest = getViewport();
      if (latest.width !== openingViewport.width || latest.height !== openingViewport.height) return;
      openingTilesReady = true;
      // Only this opening view's completed tile wait unlocks the deeper fit; any user move or resize leaves the current camera alone.
      if (camera.x !== opening.x || camera.y !== opening.y || camera.zoom !== opening.zoom) return;
      camera = fitForScope(latest, freeFrame(latest));
      applyCamera(true);
    })();
  }
  return {
    setScope,
    viewportEl,
    waitForInitialPaint: async () => {
      // Scope changes and ResizeObserver callbacks can land while a surface is
      // decoding. Re-read the actual frame after each await; a stale world
      // bake must never announce readiness for a larger/current viewport.
      // Regional tiles can stand in for the world only when decoded opaque
      // canvases cover the complete frame beyond their faded outer edges.
      // Otherwise the world remains a required first-paint dependency.
      while (true) {
        applyCamera(true);
        const viewport = getViewport();
        const awaitedCamera = { ...camera };
        const unitScale = screenScale({ x: 0, y: 0, zoom: 1 }, viewport);
        // Capture dependencies before awaiting the world surface. A visible
        // tile failure can lower the camera's zoom ceiling and hide its own
        // requirement; the poster must still remain for that original view.
        const requiresVisibleTiles = tileFadeOpacity(camera, viewport) > 0 && tileLayer.hasManifestTiles(camera, viewport);
        const visibleTilesReady = requiresVisibleTiles
          ? tileLayer.waitForVisibleTiles()
          : Promise.resolve("ready" as const);
        let tilesReady: Awaited<typeof visibleTilesReady>;
        try {
          tilesReady = await visibleTilesReady;
        } catch {
          return false;
        }
        if (tilesReady === "failed") return false;
        if (tilesReady === "superseded") {
          // A resize can go A→B→A while the old request is pending. The
          // generation changed regardless of the final geometry, so always
          // capture and await dependencies for the current frame again.
          continue;
        }
        applyCamera(true);
        const afterTilesViewport = getViewport();
        if (
          camera.x !== awaitedCamera.x || camera.y !== awaitedCamera.y || camera.zoom !== awaitedCamera.zoom ||
          afterTilesViewport.width !== viewport.width || afterTilesViewport.height !== viewport.height
        ) continue;
        const tilesCoverFrame = tileLayer.hasOpaqueViewportCoverage(camera, afterTilesViewport);
        try {
          if (!tilesCoverFrame) await rebakeWorld(unitScale, camera.zoom);
        } catch {
          return false;
        }
        applyCamera(true);
        const latest = getViewport();
        if (
          camera.x !== awaitedCamera.x || camera.y !== awaitedCamera.y || camera.zoom !== awaitedCamera.zoom ||
          latest.width !== viewport.width || latest.height !== viewport.height
        ) continue;
        const latestUnitScale = screenScale({ x: 0, y: 0, zoom: 1 }, latest);
        const latestTilesCoverFrame = tileLayer.hasOpaqueViewportCoverage(camera, latest);
        if (tilesCoverFrame && !latestTilesCoverFrame) continue;
        if (!latestTilesCoverFrame && !worldBakeIsSharpEnough(latestUnitScale, camera.zoom)) continue;
        return true;
      }
    },
    setCooperativeGestures,
    refitScopeIfClipped,
    setCurrentArticle,
    setArticleMode: (articleOnly) => setArticleMode(articleOnly, articleOnly),
  };
}
