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

/**
 * Which of a tile's own two bled-toward-a-LOWER-index-neighbour edges (its
 * own west and south) must be clipped away because a real neighbour cell
 * sits there — the fix for a faint but real line measured along both axes
 * of the `2 * bleed` overlap two adjacent tiles deliberately leave at their
 * shared edge (`tileOverlayTransform`): sampling pixel rows/columns across a
 * real tile pair showed BOTH a west- and a south-bleeding slice reading a
 * few colour units off its own tile's flat surroundings — reproducible with
 * the neighbour and the world layer each hidden in turn, so neither a
 * two-copy disagreement nor the world layer showing through explains it —
 * while the SAME tile's own east/north-bleeding slice (the opposite
 * direction) never did. Whatever in the per-cell source data or its
 * rendering makes a west/south bleed less trustworthy than an east/north
 * one, never drawing it is strictly safer than drawing it on top: this
 * function always keeps a tile's bleed toward a HIGHER-index neighbour
 * (east, north) and clips the slice it bled toward a LOWER-index one (west,
 * south) whenever that neighbour cell actually exists, so of any two
 * adjacent tiles exactly one — always the lower-index one, bleeding
 * forward — draws their shared strip. A cell with no neighbour in a given
 * direction (the outer perimeter of this site's tile coverage) keeps that
 * edge's own bleed unclipped, same as before: nothing to prefer away from
 * there, and `tileFadeOpacity`'s cross-fade still needs it to close the
 * sub-pixel seam against the bare world layer past it.
 */
export function tileClipInset(
  x: number,
  y: number,
  availableTiles: Array<[number, number]>,
  k: number,
  bleed: number,
): { bottom: number; left: number } {
  const has = (cx: number, cy: number): boolean => availableTiles.some(([ax, ay]) => ax === cx && ay === cy);
  const insetUnits = bleed * k;
  return {
    // Larger y is further north (`tile_frame`'s own `center_latitude`), so
    // the south (lower-index) neighbour is (x, y - 1).
    bottom: has(x, y - 1) ? insetUnits : 0,
    left: has(x - 1, y) ? insetUnits : 0,
  };
}

/** How far, in canvas units (world units times `k`, the SAME unit `tileClipInset` returns), the outer edge of a tile's own covered region fades toward the bare world layer past it — a cell about 23 world units wide, so this is a short distance against it, not a redraw of the whole tile. */
const OUTER_FADE_WORLD_UNITS = 1.5;

/**
 * The `mask-image` that fades a tile's own OUTER edges only — the sides
 * `tileClipInset` finds no real neighbour cell on — toward transparent,
 * leaving every edge shared with a real neighbour fully opaque: past the
 * site's own detail coverage, the world layer carries a frame at a coarser,
 * faded rendering of the same terrain (`map.ts`'s own relief-strength
 * fade), so a tile that stopped dead at its own nominal cell edge met it
 * with a hard tone step, same shape as the seam `tileClipInset` fixes but
 * deliberately placed rather than measured away — there is no second
 * tile's content to disagree with past a coverage edge, only the world's
 * own coarser one.
 *
 * One `linear-gradient` layer per outer edge — ordinarily at most two
 * (a corner of the site's own tile coverage, missing one of east/west and
 * one of north/south; `relevant_tiles`' own 3x3-per-place neighbourhood
 * means a cell is never missing BOTH neighbours on the same axis unless
 * two separate places' own coverage areas happen not to touch there
 * either) — composited with
 * `intersect` (Porter-Duff source-in, chained) rather than the `add`
 * default: `add` would UNION each gradient's own opaque region, so a
 * corner tile's south-fading layer would paint its own (un-faded) east
 * edge back to full opacity — the opposite of what two independent fades
 * meeting at a corner should do. `intersect` instead keeps a pixel only as
 * opaque as the DARKEST (most-faded) layer covering it, so a corner's two
 * fades compose the way two independent dimmers would. `null` when the
 * tile has no outer edge at all (every side has a real neighbour) — the
 * common case past a site's own first ring of tiles, which keeps its own
 * `mask-image` unset rather than carrying a no-op one.
 */
export function tileEdgeMask(
  x: number,
  y: number,
  availableTiles: Array<[number, number]>,
  k: number,
): string | null {
  const has = (cx: number, cy: number): boolean => availableTiles.some(([ax, ay]) => ax === cx && ay === cy);
  const fade = `${OUTER_FADE_WORLD_UNITS * k}px`;
  const opaque = "black";
  const clear = "transparent";
  const layers: string[] = [];
  if (!has(x - 1, y)) layers.push(`linear-gradient(to right, ${clear}, ${opaque} ${fade})`);
  if (!has(x + 1, y)) layers.push(`linear-gradient(to left, ${clear}, ${opaque} ${fade})`);
  // Larger y is further north (`tile_frame`'s own `center_latitude`): the
  // top of a tile's own canvas (CSS "to bottom" fades FROM the top edge)
  // is its north side, so a missing NORTH neighbour is `(x, y + 1)`.
  if (!has(x, y + 1)) layers.push(`linear-gradient(to bottom, ${clear}, ${opaque} ${fade})`);
  if (!has(x, y - 1)) layers.push(`linear-gradient(to top, ${clear}, ${opaque} ${fade})`);
  return layers.length > 0 ? layers.join(", ") : null;
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
          // This tile just became a real, DOM-present neighbour for up to
          // four others — including, possibly, itself relative to ones
          // already loaded — so both its own clip/mask and theirs need
          // recomputing now, not at whatever later camera move happens to
          // call `render()` again next.
          this.applyClipAndMask(svg, x, y);
          this.refreshNeighbourClipAndMask(x, y);
        })
        .catch(() => {
          this.elements.set(key, "failed");
          // A failed fetch is cached and never retried (this file's own
          // doc above), so any loaded neighbour that was treating this
          // cell as still-pending must now fade that shared edge instead
          // of waiting forever for a tile that will never arrive.
          this.refreshNeighbourClipAndMask(x, y);
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

  /** Only the transform — called on every `render()`, including a plain pan where nothing has loaded, failed, or been removed, so this must never touch `clip-path`/`mask-image` (see `applyClipAndMask`, called only on those load-state changes). */
  private position(svg: SVGElement, x: number, y: number, unitScale: number): void {
    const { translateX, translateY, scale } = tileOverlayTransform(x, y, unitScale, this.options.k, this.options.bleed);
    const el = svg as unknown as HTMLElement;
    el.style.left = "0";
    el.style.top = "0";
    el.style.transformOrigin = "0 0";
    el.style.transform = `translate(${translateX}px, ${translateY}px) scale(${scale})`;
  }

  /** Cell keys of this layer's own tiles that are actually loaded and in the DOM right now — the "neighbour exists" `tileClipInset` needs: clipping toward a neighbour is only ever correct once that neighbour is itself drawing the shared strip, never merely listed in the static manifest (a neighbour still `"loading"`, or forever `"failed"` since a failure is never retried, would otherwise leave the shared strip undrawn by either tile). */
  private loadedNeighbourCells(): Array<[number, number]> {
    const loaded: Array<[number, number]> = [];
    for (const [key, value] of this.elements) {
      if (value === "loading" || value === "failed") continue;
      const [cx, cy] = key.split(",").map(Number);
      loaded.push([cx, cy]);
    }
    return loaded;
  }

  /** The manifest's own cells, minus any that have already `"failed"` to load — the "neighbour exists at all" `tileEdgeMask` needs for its outer-edge fade: a cell the manifest never listed and a cell whose fetch permanently failed read the same way here, both absent, so either one fades the shared edge instead of ending it hard. A cell still `"loading"` (or not yet attempted) stays present — it may yet load, so its edge is not treated as outer. */
  private nonFailedManifestCells(): Array<[number, number]> {
    return this.options.availableTiles.filter(([cx, cy]) => this.elements.get(`${cx},${cy}`) !== "failed");
  }

  /** Recompute and (re)apply `clip-path`/`mask-image` for `svg`, the tile at `(x, y)` — the only place either style is ever written, so the only way they change is a call here, never a plain `render()` pan. */
  private applyClipAndMask(svg: SVGElement, x: number, y: number): void {
    const el = svg as unknown as HTMLElement;
    // `clip-path` on an SVG root with intrinsic width/height (no CSS
    // width/height override here) operates in that SAME local/viewBox unit
    // space `position()`'s own `scale(...)` then shrinks as a whole — so an
    // inset in canvas units clips exactly the bled slice `tileClipInset`
    // names, regardless of `unitScale`/zoom.
    const { bottom, left } = tileClipInset(x, y, this.loadedNeighbourCells(), this.options.k, this.options.bleed);
    el.style.clipPath = bottom > 0 || left > 0 ? `inset(0 0 ${bottom}px ${left}px)` : "";
    // Same local/viewBox unit space as the `clip-path` above — an edge this
    // tile shares with a real, LOADED neighbour is never also one of its
    // own OUTER edges (manifest-present and not failed), so the two never
    // fight over the same side.
    const mask = tileEdgeMask(x, y, this.nonFailedManifestCells(), this.options.k);
    el.style.maskImage = mask ?? "";
    el.style.webkitMaskImage = mask ?? "";
    if (mask) {
      el.style.maskComposite = "intersect";
      el.style.webkitMaskComposite = "source-in";
    }
  }

  /** Re-run `applyClipAndMask` for each of `(x, y)`'s own up-to-four loaded neighbours — called whenever `(x, y)` itself finishes loading or fails, the two moments that can change what a NEIGHBOUR should draw at the shared edge, independent of any camera move. */
  private refreshNeighbourClipAndMask(x: number, y: number): void {
    for (const [nx, ny] of [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]] as const) {
      const neighbour = this.elements.get(`${nx},${ny}`);
      if (neighbour && neighbour !== "loading" && neighbour !== "failed") this.applyClipAndMask(neighbour, nx, ny);
    }
  }
}
