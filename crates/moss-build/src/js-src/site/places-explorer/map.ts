/**
 * map.ts — the controller: world SVG, DOM scaffold, and the camera
 * transform.
 *
 * Owns the one mutable `Camera` the whole explorer reads and writes
 * (`gestures.ts`, `markers.ts`'s callbacks, card selection, the URL layer)
 * and the single `applyCamera` that turns it into the `.moss-places-world`
 * CSS transform, the relief/river custom properties, and a re-render of
 * markers, tiles and cards. Regional tiles are `tiles.ts`'s own
 * `TileLayer`, constructed once here and driven by `applyCamera` the same
 * way `markers.ts`'s `MarkerLayer` is — everything else in this directory
 * is a module `mountPlacesMap` wires together, not a second place that
 * touches the DOM this one owns.
 */
import {
  clampCamera,
  coverCamera,
  detailMaxZoom,
  fitPoints,
  MIN_ZOOM,
  screenScale,
  tileDetailMaxZoom,
  worldToScreen,
} from "./camera";
import { CardRow, worksForRow } from "./cards";
import { ScopeChip } from "./chip";
import { LabelLayer } from "./labels";
import { MarkerLayer, pointsForWorks } from "./markers";
import { project, WORLD_HEIGHT, WORLD_WIDTH } from "./projection";
import { inScope } from "./scope";
import { copyFor } from "./strings";
import { attachGestures } from "./gestures";
import { parseMapSvg, TileLayer } from "./tiles";
import * as urlState from "./state";
import type { Camera, LabelsData, PlacesData, Place, Point, Rect, Scope, Viewport } from "./types";

/** Relief fades to this floor at the detail ceiling — the design's own tuned value, ported from the prototype's `RELIEF_STRENGTH_FLOOR`. */
const RELIEF_STRENGTH_FLOOR = 0.2;

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

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
  /** How far, in world units, a tile's own canvas was padded past its nominal cell on every edge (`tiles.json`'s own `bleed`, the build's `TILE_BLEED`) — read off the build for the same reason `tileK` is. */
  tileBleed: number;
  places: PlacesData;
  /** `labels.json`'s own parsed body — `null`/`undefined` when the handshake carried no `data-labels` or that fetch failed; the label layer then simply never places anything (progressive enhancement, same posture as a missing locator). */
  labels?: LabelsData | null;
  lang: string;
}

export interface PlacesMapController {
  /** The scope seam: `all` or `place` this landing, the breadcrumb chip's own dig-down included. Re-fits the camera and re-renders. */
  setScope(scope: Scope): void;
}

/** Build the interactive layer in place of `figure`'s static `<svg>` child and wire every gesture, selection and scope path together. `null` (leaving the static figure untouched) when the fetched world SVG fails to parse. */
export function mountPlacesMap(figure: HTMLElement, options: MountOptions): PlacesMapController | null {
  const strings = copyFor(options.lang);
  const worldSvg = parseMapSvg(options.worldSvgText);
  if (!worldSvg) return null;

  const viewportEl = document.createElement("div");
  viewportEl.className = "moss-places-viewport";
  viewportEl.tabIndex = 0;
  viewportEl.setAttribute("role", "application");
  viewportEl.setAttribute("aria-label", strings.map);

  const worldEl = document.createElement("div");
  worldEl.className = "moss-places-world";
  worldEl.append(worldSvg);

  const tilesEl = document.createElement("div");
  tilesEl.className = "moss-places-tiles";
  worldEl.append(tilesEl);

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
    bleed: options.tileBleed,
  });
  const initial = urlState.readUrlState();

  let scope: Scope = initial.scope;
  let selectedId: string | null = initial.articleId;
  let scopedIds: Set<string> | null = null;

  function allPoints(): Point[] {
    return pointsForWorks(options.places.works, options.places.places, { kind: "all" });
  }

  function getViewport(): Viewport {
    const rect = viewportEl.getBoundingClientRect();
    return { width: rect.width, height: rect.height };
  }

  function currentMaxZoom(viewport: Viewport): number {
    return tileLayer.hasVisibleTiles(camera, viewport)
      ? tileDetailMaxZoom(viewport, options.tileK)
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
        camera = fitPoints(points, viewport, currentMaxZoom(viewport));
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
    return [
      relative(controlsEl),
      relative(cardsEl),
      relative(chipEl),
      relative(chipEl.querySelector(".moss-places-chip-menu")),
    ].filter((rect): rect is Rect => rect != null);
  }

  function selectWork(id: string | null): void {
    const next = selectedId === id ? null : id;
    selectedId = next;
    if (next) {
      const work = worksById.get(next);
      const points = (work?.places ?? [])
        .map((placeId) => placesById.get(placeId))
        .filter((place): place is Place => place != null)
        .map((place) => project(place.lat, place.lng));
      if (points.length) {
        const viewport = getViewport();
        camera = fitPoints(points, viewport, currentMaxZoom(viewport));
      }
      if (work) announce(work.title || strings.untitled);
    }
    urlState.writeSelection(selectedId);
    applyCamera(true);
    cardRow.revealSelected(selectedId);
  }

  function setScope(next: Scope): void {
    scope = next;
    scopedIds = null;
    markerLayer.closeRing();
    urlState.writeScope(next);
    const points = pointsForWorks(options.places.works, options.places.places, next);
    const viewport = getViewport();
    camera = points.length ? fitPoints(points, viewport, currentMaxZoom(viewport)) : coverCamera(allPoints(), viewport);
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
    { setScope, selectWork, highlightPlace, menuToggled: () => applyCamera(true) },
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
  let camera: Camera = initial.camera ?? (() => {
    if (scope.kind !== "place") return coverCamera(allPoints(), getViewport());
    const points = pointsForWorks(options.places.works, options.places.places, scope);
    return points.length ? fitPoints(points, getViewport()) : coverCamera(allPoints(), getViewport());
  })();

  // ---- camera application -------------------------------------------------
  function applyCamera(settled: boolean): void {
    const viewport = getViewport();
    if (viewport.width <= 0 || viewport.height <= 0) return;
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
    const fade = ceiling > MIN_ZOOM ? clamp01((camera.zoom - MIN_ZOOM) / (ceiling - MIN_ZOOM)) : 0;
    figure.style.setProperty("--moss-place-relief-strength", String(1 - fade * (1 - RELIEF_STRENGTH_FLOOR)));
    figure.style.setProperty("--moss-place-river-scale", String(Math.min(1, MIN_ZOOM / camera.zoom)));

    tileLayer.render(camera, viewport, unitScale);

    const visiblePoints = pointsForWorks(options.places.works, options.places.places, scope).filter((point) => {
      const screen = worldToScreen(point, camera, viewport);
      return screen.x >= -22 && screen.x <= viewport.width + 22 && screen.y >= -22 && screen.y <= viewport.height + 22;
    });
    markerLayer.render(visiblePoints, camera, viewport, selectedId, worksById);

    zoomInBtn.disabled = camera.zoom >= ceiling;
    zoomOutBtn.disabled = camera.zoom <= MIN_ZOOM;

    if (settled) {
      // The world layer is promoted to its own compositing layer only
      // between `onGestureStart` and here — demoting it back on every
      // settle forces a fresh rasterisation at the resting size/scale
      // instead of resampling a composited texture rasterised for a
      // different one, which is what reads blurry after a zoom out-then-in.
      worldEl.removeAttribute("data-gesture");
      const visibleIds = new Set(visiblePoints.map((point) => point.id));
      const rows = worksForRow(options.places.works, visibleIds, scopedIds, selectedId);
      cardRow.render(rows, options.places.places, selectedId);
      labelLayer.render(visiblePoints, camera, viewport, worksById, placesById, reservedLabelRects());
      urlState.writeCamera(camera);
      const selectedWork = selectedId ? (worksById.get(selectedId) ?? null) : null;
      scopeChip.render(scope, options.places.places, options.places.works, selectedWork, markerLayer.ringCount());
    }
  }

  // ---- gestures ------------------------------------------------------------
  attachGestures(viewportEl, {
    getCamera: () => camera,
    getViewport,
    setCamera(next) {
      camera = next;
      applyCamera(false);
    },
    onSettle: () => applyCamera(true),
    onGestureStart: () => worldEl.setAttribute("data-gesture", ""),
    maxZoom: () => currentMaxZoom(getViewport()),
  });

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
    camera = coverCamera(allPoints(), getViewport());
    applyCamera(true);
  });

  if (typeof ResizeObserver === "function") {
    new ResizeObserver(() => applyCamera(true)).observe(viewportEl);
  }
  window.addEventListener("resize", () => applyCamera(true));

  applyCamera(true);
  return { setScope };
}
