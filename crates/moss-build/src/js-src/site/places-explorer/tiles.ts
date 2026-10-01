/**
 * tiles.ts — regional detail tiles: fetch, overlay, and the shared map-SVG
 * sanitiser.
 *
 * Mirrors `markers.ts`'s `MarkerLayer`: `TileLayer` owns exactly the tile
 * DOM/fetch lifecycle (which cells are already in flight, loaded, or
 * failed; where each loaded element sits; whether the layer should exist at
 * all yet — see `tileFadeOpacity`) and nothing else, constructed once by
 * `map.ts`'s `mountPlacesMap` and driven by its own `render` call on every
 * `applyCamera`. `render` is a no-op below the fade band short of the
 * world's own detail ceiling: the world layer alone carries every zoom up
 * to that ceiling, so a tile exists only to supply detail past it, never as
 * a second copy of what the world is already drawing. The pure geometry
 * (`tileCellBounds`, `tileOverlayTransform`, `tilesForView`,
 * `tileFadeOpacity`) stays exported as plain functions, the same split
 * `markers.ts` draws between `MarkerLayer` (DOM) and
 * `pointsForWorks`/`clusters.ts` (pure) — so a test can exercise the
 * geometry without a camera or a DOM ever existing.
 */
import { detailMaxZoom, screenScale } from "./camera";
import { project } from "./projection";
import type { Camera, Viewport } from "./types";

/** The last fraction of the world layer's own zoom range (`0`..`detailMaxZoom`, NOT the raised ceiling `hasVisibleTiles` unlocks) where the tile layer fades in. */
const TILE_FADE_BAND = 0.2;

/**
 * How visible the regional-tile layer should be at `camera`, 0..1 — the
 * cross-fade `TileLayer.render` drives (its own `--moss-place-tile-opacity`)
 * so the handover past the world layer's own detail ceiling is a fade, not
 * tiles popping onto the world view at rest. 0 at and below 80% of
 * `detailMaxZoom`, ramping linearly to 1 exactly at it — past that, the
 * camera is already into the zoom range only a raised, tile-covered ceiling
 * (`hasVisibleTiles`) permits. Pure and independent of whether any cell
 * actually has a tile to show: a camera over open ocean reaches full
 * opacity on the same schedule, it just has nothing to apply it to.
 */
export function tileFadeOpacity(camera: Camera, viewport: Viewport): number {
  const ceiling = detailMaxZoom(viewport);
  const fadeStart = ceiling * (1 - TILE_FADE_BAND);
  if (ceiling <= fadeStart) return camera.zoom >= ceiling ? 1 : 0;
  return Math.max(0, Math.min(1, (camera.zoom - fadeStart) / (ceiling - fadeStart)));
}

/** 10-degree tile cell `(x, y)` world bounds — `x*10-180`..`+10`, `y*10-90`..`+10`, mirroring `tile_frame()` in `crates/moss-build/src/build/place_map/explorer.rs` exactly (x 0..=35 west to east, y 0..=17 south to north). Longitude is linear in `project()`, so an edge's `x` never depends on which latitude it's projected at, and the reverse for `y`/longitude — each edge is projected at longitude/latitude 0 for that reason, not at the cell's own opposite edge. */
export function tileCellBounds(x: number, y: number): { minX: number; maxX: number; minY: number; maxY: number } {
  const lngMin = x * 10 - 180;
  const lngMax = lngMin + 10;
  const latMin = y * 10 - 90;
  const latMax = latMin + 10;
  // Higher latitude (north) projects to a SMALLER y (north is up), so the
  // cell's own maxY is its SOUTH (latMin) edge.
  return {
    minX: project(0, lngMin).x,
    maxX: project(0, lngMax).x,
    minY: project(latMax, 0).y,
    maxY: project(latMin, 0).y,
  };
}

/**
 * The CSS transform that overlays regional tile `(x, y)` on the world
 * layer: a tile is rendered server-side at `k` times the world's own scale,
 * in the SAME Patterson projection, clipped to its own cell padded by
 * `bleed` world units on every edge (`PattersonProjection::for_tile` in
 * `crates/moss-build/src/build/place_map/geometry.rs` — `bleed` is
 * `tiles.json`'s own `bleed` field, `TILE_BLEED` there), so its viewBox
 * already equals that padded rectangle times `k` — overlaying it is
 * therefore one transform, not a resize: translate its own `(0, 0)` to the
 * padded rectangle's own top-left corner (the cell's own corner, minus
 * `bleed`), then scale down by `1 / k` to bring its magnified content back
 * to the world's own unit size. `unitScale` (world units to CSS px,
 * constant across pan/zoom — see `map.ts`'s `applyCamera`) converts both
 * numbers into the SAME px space `worldEl`'s own internal layout uses, so
 * the tile inherits the world layer's own outer `translate(...)
 * scale(camera.zoom)` for free and adjacent tiles OVERLAP by `2 * bleed`
 * world units at their shared edge instead of meeting it exactly: neither
 * one's rendered size depends on its own SVG being stretched to a
 * DIFFERENT aspect ratio (the bug this replaces), and the deliberate
 * overlap absorbs the sub-pixel rounding that an exact meet is vulnerable
 * to when two independently-transformed elements are rasterized.
 */
export function tileOverlayTransform(
  x: number,
  y: number,
  unitScale: number,
  k: number,
  bleed: number,
): { translateX: number; translateY: number; scale: number } {
  const bounds = tileCellBounds(x, y);
  return {
    translateX: (bounds.minX - bleed) * unitScale,
    translateY: (bounds.minY - bleed) * unitScale,
    scale: unitScale / k,
  };
}

/** Pad, in world units, added to the viewport's own visible rect before testing a tile for intersection — a tile just past the edge is worth fetching slightly ahead of the reader panning to it. */
const TILE_VIEW_PAD = 20;

/** Which of `cells` intersect the current view, world-space. Pure — no fetch, no DOM, so a view can be tested without a camera ever existing (the vitest suite passes a plain `Camera`/`Viewport` pair). */
export function tilesForView(cells: Array<[number, number]>, camera: Camera, viewport: Viewport): Array<[number, number]> {
  const scale = screenScale(camera, viewport);
  const halfW = viewport.width / 2 / scale + TILE_VIEW_PAD;
  const halfH = viewport.height / 2 / scale + TILE_VIEW_PAD;
  const left = camera.x - halfW;
  const right = camera.x + halfW;
  const top = camera.y - halfH;
  const bottom = camera.y + halfH;
  return cells.filter(([x, y]) => {
    const bounds = tileCellBounds(x, y);
    return bounds.maxX >= left && bounds.minX <= right && bounds.maxY >= top && bounds.minY <= bottom;
  });
}

/** Give every `[data-map-layer="rivers"] path` its own `--river-w` custom property, copied once from its baked `stroke-width` attribute — the base `places-explorer.css` multiplies by `--moss-place-river-scale` on every camera settle, so a river's taper survives while its ON-SCREEN width stays constant as the reader zooms. */
function prepareRiverWidths(svg: SVGElement): void {
  svg.querySelectorAll<SVGElement>('[data-map-layer="rivers"] path[stroke-width]').forEach((path) => {
    const width = path.getAttribute("stroke-width");
    if (width) path.style.setProperty("--river-w", width);
  });
}

/** Parse and sanitise a fetched map SVG (the world map or a regional tile): must be a real `<svg>` with no parser error; strips `<script>`/`<foreignObject>` and any `on*`/non-local `href` the source should never carry, the same defense-in-depth a fetched asset gets regardless of same-origin trust. `null` on anything that fails those checks. */
export function parseMapSvg(markup: string): SVGElement | null {
  const parsed = new DOMParser().parseFromString(markup, "image/svg+xml");
  const svg = parsed.documentElement;
  if (svg.localName !== "svg" || parsed.querySelector("parsererror")) return null;
  parsed.querySelectorAll("script, foreignObject").forEach((node) => node.remove());
  parsed.querySelectorAll("*").forEach((node) => {
    for (const attribute of [...node.attributes]) {
      const local = attribute.value.startsWith("#");
      const dataImage = /^data:image\/(?:png|jpe?g|webp|svg\+xml);base64,[a-z\d+/=]+$/i.test(attribute.value);
      if (/^on/i.test(attribute.name) || (["href", "xlink:href"].includes(attribute.name) && !local && !dataImage)) {
        node.removeAttribute(attribute.name);
      }
    }
  });
  prepareRiverWidths(svg as unknown as SVGElement);
  const imported = document.importNode(svg, true) as unknown as SVGElement;
  imported.removeAttribute("role");
  imported.removeAttribute("aria-label");
  imported.setAttribute("focusable", "false");
  imported.setAttribute("aria-hidden", "true");
  return imported;
}

export interface TileLayerOptions {
  tilesBaseUrl: string;
  availableTiles: Array<[number, number]>;
  /** The factor a tile is drawn at over the world's own scale (`tiles.json`'s own `k`, the build's `TILE_K`). */
  k: number;
  /** How far, in world units, a tile's own canvas was padded past its nominal cell on every edge (`tiles.json`'s own `bleed`, the build's `TILE_BLEED`) — see `tileOverlayTransform`. */
  bleed: number;
}

/** The regional-tile DOM layer: fetches each cell's SVG once, caches the result (including a failure, so a 404 is never retried every frame), and keeps every loaded element positioned over its own cell as the camera moves. */
export class TileLayer {
  private readonly container: HTMLElement;
  private readonly options: TileLayerOptions;
  private readonly elements = new Map<string, SVGElement | "loading" | "failed">();

  constructor(container: HTMLElement, options: TileLayerOptions) {
    this.container = container;
    this.options = options;
  }

  /** Whether any of this layer's own cells are in view at `camera`/`viewport` — `map.ts`'s own detail-ceiling switch (`currentMaxZoom`) reads this rather than reaching into `availableTiles` itself. */
  hasVisibleTiles(camera: Camera, viewport: Viewport): boolean {
    return tilesForView(this.options.availableTiles, camera, viewport).length > 0;
  }

  /**
   * Fetch and position every tile visible at `camera`/`viewport` — but only
   * once the camera's own screen scale has entered the fade band short of
   * the world layer's detail ceiling (`tileFadeOpacity`); below it, this
   * clears the layer instead (no fetch, no DOM), which is what keeps the
   * world view at rest from ever showing a tile rectangle sitting on top of
   * it. `unitScale` is world units to CSS px (constant across pan/zoom —
   * see `map.ts`'s `applyCamera`), the SAME px space the world layer's own
   * internal layout uses.
   */
  render(camera: Camera, viewport: Viewport, unitScale: number): void {
    const opacity = tileFadeOpacity(camera, viewport);
    this.container.style.setProperty("--moss-place-tile-opacity", String(opacity));
    if (opacity <= 0) {
      this.clear();
      return;
    }
    const visible = tilesForView(this.options.availableTiles, camera, viewport);
    for (const [x, y] of visible) {
      const key = `${x},${y}`;
      const existing = this.elements.get(key);
      if (existing === "loading" || existing === "failed") continue;
      if (existing) {
        this.position(existing, x, y, unitScale);
        continue;
      }
      this.elements.set(key, "loading");
      void fetch(`${this.options.tilesBaseUrl}tile-${x}-${y}.svg`)
        .then((response) => (response.ok ? response.text() : Promise.reject(new Error(String(response.status)))))
        .then((text) => {
          // A `clear()` between this fetch starting and now (the camera
          // dropped back out of the fade band mid-flight) already dropped
          // this key — finishing the append anyway would be exactly the DOM
          // `clear()` exists to rule out below the band.
          if (this.elements.get(key) !== "loading") return;
          const svg = parseMapSvg(text);
          if (!svg) throw new Error("invalid tile svg");
          svg.dataset.mossPlacesTile = key;
          svg.style.position = "absolute";
          this.container.append(svg);
          this.elements.set(key, svg);
          this.position(svg, x, y, unitScale);
        })
        .catch(() => {
          this.elements.set(key, "failed");
        });
    }
  }

  /** Remove every tile element this layer has added and forget its own fetch/load state, so a later `render()` re-fetches from scratch — the "no DOM" half of the fade-band contract above. */
  private clear(): void {
    if (this.elements.size === 0) return;
    for (const value of this.elements.values()) {
      if (value !== "loading" && value !== "failed") value.remove();
    }
    this.elements.clear();
  }

  private position(svg: SVGElement, x: number, y: number, unitScale: number): void {
    const { translateX, translateY, scale } = tileOverlayTransform(x, y, unitScale, this.options.k, this.options.bleed);
    const el = svg as unknown as HTMLElement;
    el.style.left = "0";
    el.style.top = "0";
    el.style.transformOrigin = "0 0";
    el.style.transform = `translate(${translateX}px, ${translateY}px) scale(${scale})`;
  }
}
