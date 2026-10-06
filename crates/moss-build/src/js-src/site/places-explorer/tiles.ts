/**
 * tiles.ts — regional detail tiles: fetch, overlay, and positioning.
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
 *
 * Each loaded tile is a decoded raster `<img>` plus a small live rivers
 * overlay (`raster.ts`'s `splitMapSvg`/`rasterize`). A raster is baked for the
 * density its tile is shown at (up to a size cap) and re-baked, through the
 * same concurrency limit as loads, when that density moves away from it.
 */
import { detailMaxZoom, screenScale } from "./camera";
import { project } from "./projection";
import { rasterizeOrFallback, splitMapSvg, type MapSvgSplit } from "./raster";
import type { Camera, Viewport } from "./types";

/** Tile rasters are capped at this device pixel ratio — a tile's own build-time resolution already carries the real detail ceiling, so a 3x phone gains nothing from tripling it further. */
const TILE_RASTER_DPR_CAP = 2;
/** No tile raster is ever decoded wider or taller than this many pixels — the cost of a sharp bake grows with the square of the zoom, and a phone's canvas budget does not. */
const TILE_RASTER_MAX_SIDE = 4096;
/** A tile re-rasterises only once the CSS px per canvas px it needs has moved away from what its raster was baked for by this ratio, up or down — the same deadband the world raster uses, so a steady zoom never schedules a decode, and a raster decoded for fullscreen is released once the view is small again. */
const TILE_REBAKE_RATIO = 1.3;
/** How many regional tiles may be mid-fetch/decode at once — see `TileLayer.drainQueue`'s own doc for the stall letting every visible cell start at once was measured causing. */
const MAX_CONCURRENT_TILE_LOADS = 6;

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
 * The CSS transform that overlays a regional tile on the world layer. A
 * tile is rendered server-side at `k` times the world's own scale, in the
 * SAME Patterson projection, and its canvas starts at `origin` — a whole
 * number of canvas units (`1 / k` world units) from the world's own origin,
 * written to `tiles.json`'s `origins` by `TileGrid` in
 * `crates/moss-build/src/build/place_map/geometry/tile_grid.rs`. The canvas
 * is the cell padded by a bleed on every edge, so adjacent tiles overlap at
 * their shared edge instead of meeting it exactly, which absorbs the
 * sub-pixel rounding two independently-transformed elements are rasterized
 * with. Overlaying it is one transform, not a resize: translate its own
 * `(0, 0)` to `origin`, then scale by `1 / k` to bring its magnified
 * content back to the world's unit size. `unitScale` (world units to CSS
 * px, constant across pan/zoom — see `map.ts`'s `applyCamera`) puts both
 * numbers in the px space `worldEl`'s own layout uses, so the tile inherits
 * the world layer's own outer `translate(...) scale(camera.zoom)`. Every
 * tile's vertices were rounded to integers from an origin on that one
 * lattice, so a shared coastline lands on the same pixels in both tiles.
 */
export function tileOverlayTransform(
  origin: readonly [number, number],
  unitScale: number,
  k: number,
): { translateX: number; translateY: number; scale: number } {
  const scale = unitScale / k;
  return { translateX: origin[0] * scale, translateY: origin[1] * scale, scale };
}

/** The z-index tile `(x, y)` is drawn at: north to south, then west to east. Every tile is padded on all sides and none is clipped, so where two overlap one of them draws over the other's edge and no order lets every tile's own cell win. What matters is that the order is fixed, so the overlapping edges render identically on every load, whatever order the fetches finish in. */
export function tileDrawOrder(x: number, y: number, columns: number, rows: number): number {
  return (rows - 1 - y) * columns + x;
}

/** Pad, in world units, added to the viewport's own visible rect before testing a tile for intersection — a tile just past the edge is worth fetching slightly ahead of the reader panning to it. */
const TILE_VIEW_PAD = 20;

/** Which of `cells` intersect the current view, world-space. Pure — no fetch, no DOM, so a view can be tested without a camera ever existing (the vitest suite passes a plain `Camera`/`Viewport` pair). */
export function tilesForView(cells: Array<[number, number]>, camera: Camera, viewport: Viewport, pad: number = TILE_VIEW_PAD): Array<[number, number]> {
  const scale = screenScale(camera, viewport);
  const halfW = viewport.width / 2 / scale + pad;
  const halfH = viewport.height / 2 / scale + pad;
  const left = camera.x - halfW;
  const right = camera.x + halfW;
  const top = camera.y - halfH;
  const bottom = camera.y + halfH;
  return cells.filter(([x, y]) => {
    const bounds = tileCellBounds(x, y);
    return bounds.maxX >= left && bounds.minX <= right && bounds.maxY >= top && bounds.minY <= bottom;
  });
}

/** How far, in canvas units (world units times `k`), the outer edge of a tile's own covered region fades toward the bare world layer past it — a cell about 23 world units wide, so this is a short distance against it, not a redraw of the whole tile. */
const OUTER_FADE_WORLD_UNITS = 1.5;

/**
 * The `mask-image` that fades a tile's own OUTER edges only — the sides
 * with no real neighbour cell — toward transparent,
 * leaving every edge shared with a real neighbour fully opaque: past the
 * site's own detail coverage, the world layer carries a frame at this
 * tile's own geometry but no further detail past it, so a tile that
 * stopped dead at its own nominal cell edge met it with a hard tone step,
 * the same shape as a seam, but deliberately placed rather than
 * measured away — there is no second tile's content to
 * disagree with past a coverage edge, only the world's own coarser one.
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
  /** Each emitted cell's canvas origin, keyed `"x,y"`, in whole canvas units (`tiles.json`'s own `origins`) — see `tileOverlayTransform`. */
  origins: Record<string, [number, number]>;
  /** The grid's size in cells (`tiles.json`'s own `columns` and `rows`), which `tileDrawOrder` numbers tiles by. */
  columns: number;
  rows: number;
}

/** A fetched, decoded tile: the positioned wrapper div and the raster's own release callback (a no-op on the progressive-enhancement fallback — see `raster.ts`'s `rasterizeOrFallback`). */
interface LoadedTile {
  el: HTMLElement;
  release: () => void;
  /** Kept so a sharper raster is a decode, not a second fetch. */
  split: MapSvgSplit;
  /** CSS px per canvas px the current raster was actually baked for (after the size cap). */
  bakedDensity: number;
  /** A re-bake of this tile is queued or decoding. */
  rebaking: boolean;
}

/** The regional-tile DOM layer: fetches each cell's SVG once, caches the result (including a failure, so a 404 is never retried every frame), and keeps every loaded element positioned over its own cell as the camera moves. */
export class TileLayer {
  private readonly container: HTMLElement;
  private readonly options: TileLayerOptions;
  private readonly elements = new Map<string, LoadedTile | "loading" | "failed">();
  /** Cells queued behind `MAX_CONCURRENT_TILE_LOADS`, each already marked "loading" in `elements` — a `clear()` landing while one waits here drops it from `elements` but not from this array, so `drainQueue` re-checks "still wanted?" before spending a slot on it, the same staleness guard `load` itself applies at its own two await points. */
  private readonly queue: string[] = [];
  /** Loaded tiles waiting for a slot to re-bake; loads go first, since an empty cell costs the reader more than a soft one. */
  private readonly rebakeQueue: string[] = [];
  private activeLoads = 0;
  /** Bumped by `clear()`, captured by `load()` at its own start and re-checked after each of its two awaits — the per-cell `"loading"` sentinel alone cannot tell a load apart from a LATER load of the SAME cell: a `clear()` landing mid-fetch and a re-queue of the same key before the first `load()` resolves both see `"loading"`, so without this a stale load can still finish, append its own wrapper, and overwrite the fresh one's bookkeeping. A generation mismatch means "a clear() happened since I started" regardless of what the per-cell map currently says. */
  private generation = 0;
  /** CSS px per canvas px the tiles are shown at right now (`unitScale / k * zoom`) — what a raster must be baked for to be sharp. */
  private density = 1;
  /** World units to CSS px at the latest `render()`. A tile finishing its fetch after the viewport changed size (an embed switching layout right after mount) must be placed with THIS, not the value in force when it was queued. */
  private unitScale = 1;
  /** Keys of the tiles actually on screen right now (the loaded set also holds a margin of neighbours). Only these are baked sharp: a raster at full density is far larger than the screen it serves, so spending it on every neighbour would cost memory for nothing. */
  private onScreen = new Set<string>();
  /** Whether the latest `render()` was a settled one: re-bakes are only discovered then, so a layer is not idle before its first settled render. */
  private settledRender = false;
  /** Whether the latest `render()` was inside the fade band, i.e. tiles are wanted at all. */
  private shown = false;

  constructor(container: HTMLElement, options: TileLayerOptions) {
    this.container = container;
    this.options = options;
  }

  /** Whether any of this layer's own cells (manifest minus failed fetches) are in view at `camera`/`viewport` — `map.ts`'s own detail-ceiling switch (`currentMaxZoom`) reads this rather than reaching into `availableTiles` itself. */
  hasVisibleTiles(camera: Camera, viewport: Viewport): boolean {
    return tilesForView(this.nonFailedManifestCells(), camera, viewport).length > 0;
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
  render(camera: Camera, viewport: Viewport, unitScale: number, settled = false): void {
    this.unitScale = unitScale;
    this.density = (unitScale / this.options.k) * camera.zoom;
    const opacity = tileFadeOpacity(camera, viewport);
    this.container.style.setProperty("--moss-place-tile-opacity", String(opacity));
    this.settledRender = settled;
    this.shown = opacity > 0;
    if (opacity <= 0) {
      this.clear();
      this.publishState();
      return;
    }
    const visible = tilesForView(this.options.availableTiles, camera, viewport);
    this.onScreen = new Set(tilesForView(visible, camera, viewport, 0).map(([x, y]) => `${x},${y}`));
    for (const [x, y] of visible) {
      const key = `${x},${y}`;
      const existing = this.elements.get(key);
      if (existing === "loading" || existing === "failed") continue;
      if (existing) {
        this.position(existing.el, x, y, unitScale);
        continue;
      }
      this.elements.set(key, "loading");
      this.queue.push(key);
    }
    // Every loaded tile, not only the visible ones: a tile left outside the view when the frame shrinks must give its large raster back too.
    if (settled) {
      for (const [key, entry] of this.elements) {
        if (entry === "loading" || entry === "failed" || entry.rebaking || !this.needsRebake(key, entry)) continue;
        entry.rebaking = true;
        this.rebakeQueue.push(key);
      }
    }
    this.drainQueue();
    this.publishState();
  }

  /** `data-moss-places-tiles-state` on the container: "busy" while a bake or re-bake is pending or the view has not settled, "idle" once nothing is left to change — the signal a gate waits on before it reads pixels. */
  private publishState(): void {
    const working = this.activeLoads > 0 || this.queue.length > 0 || this.rebakeQueue.length > 0;
    const idle = !this.shown || (this.settledRender && !working);
    this.container.dataset.mossPlacesTilesState = idle ? "idle" : "busy";
  }

  /**
   * Start loading queued cells up to `MAX_CONCURRENT_TILE_LOADS` at once —
   * navigating straight to a deep zoom can put every one of ~38 tiles into
   * the queue in the same `render()` call, and letting all of them fetch,
   * decode and insert at once was measured landing their combined DOM work
   * in a single frame, a stall this layer's whole point is to avoid. Each
   * finished `load` (success or failure) re-calls this to pull the next
   * one, so the pool stays full without this layer polling for work.
   */
  private drainQueue(): void {
    while (this.activeLoads < MAX_CONCURRENT_TILE_LOADS) {
      const key = this.queue.shift();
      if (key === undefined) break;
      if (this.elements.get(key) !== "loading") continue; // a clear() already dropped it
      const [x, y] = key.split(",").map(Number);
      this.run(this.load(key, x, y));
    }
    // Re-bakes decode as large as loads do, so they take slots from the same pool.
    while (this.activeLoads < MAX_CONCURRENT_TILE_LOADS) {
      const key = this.rebakeQueue.shift();
      if (key === undefined) return;
      const entry = this.elements.get(key);
      if (!entry || entry === "loading" || entry === "failed") continue; // a clear() already dropped it
      this.run(this.rebake(key, entry));
    }
  }

  private run(job: Promise<void>): void {
    this.activeLoads++;
    void job.finally(() => {
      this.activeLoads--;
      this.drainQueue();
      this.publishState();
    });
  }

  /** Fetch, split, rasterise and position one cell — a method rather than an inline `.then()` chain so the "still wanted?" staleness check (a `clear()` landing mid-flight) reads the same way at both of its two await points. */
  private async load(key: string, x: number, y: number): Promise<void> {
    const generation = this.generation;
    try {
      const response = await fetch(`${this.options.tilesBaseUrl}tile-${x}-${y}.svg`);
      if (!response.ok) throw new Error(String(response.status));
      const text = await response.text();
      // The generation check catches what the per-cell sentinel alone
      // cannot: a clear() + re-queue of this SAME cell while this call was
      // awaiting, which resets the sentinel back to "loading" too.
      if (generation !== this.generation || this.elements.get(key) !== "loading") return; // dropped out of the fade band mid-fetch
      const split = splitMapSvg(text);
      if (!split || !this.options.origins[key]) throw new Error("invalid tile svg");
      const { surface, baked } = await this.bake(split, this.onScreen.has(key) ? this.density : 1);
      if (generation !== this.generation || this.elements.get(key) !== "loading") {
        surface.release();
        return; // dropped out of the fade band while the raster decoded, or superseded by a clear() + re-queue
      }
      const wrapper = document.createElement("div");
      wrapper.className = "moss-places-tile";
      wrapper.dataset.mossPlacesTile = key;
      wrapper.style.position = "absolute";
      wrapper.style.width = `${split.width}px`;
      wrapper.style.height = `${split.height}px`;
      wrapper.append(surface.el);
      if (split.rivers) {
        split.rivers.classList.add("moss-places-rivers");
        wrapper.append(split.rivers);
      }
      wrapper.style.zIndex = String(tileDrawOrder(x, y, this.options.columns, this.options.rows));
      this.container.append(wrapper);
      this.elements.set(key, { el: wrapper, release: surface.release, split, bakedDensity: baked, rebaking: false });
      this.position(wrapper, x, y, this.unitScale);
      // A cell the manifest names counts as present while it loads, so its
      // own load changes no neighbour's mask; only its own edges need one.
      this.applyEdgeMask(wrapper, x, y);
    } catch {
      // A stale generation's own failure must not stomp the CURRENT load's
      // state — it may already be "loading" again under the new generation.
      if (generation !== this.generation) return;
      this.elements.set(key, "failed");
      // A failed fetch is cached and never retried (this file's own doc
      // above), so any loaded neighbour that was treating this cell as
      // still-pending must now fade that shared edge instead of waiting
      // forever for a tile that will never arrive.
      this.refreshNeighbourEdgeMask(x, y);
    }
  }

  /** CSS px per canvas px a raster of `split` can actually be baked for when `density` is wanted: never below 1, never past the size cap. */
  private bakeableDensity(split: MapSvgSplit, density: number): number {
    const dpr = Math.min(window.devicePixelRatio || 1, TILE_RASTER_DPR_CAP);
    return Math.min(Math.max(1, density), TILE_RASTER_MAX_SIDE / (Math.max(split.width, split.height) * dpr));
  }

  /** Decode `split` at the pixel size a tile shown at `density` CSS px per canvas px needs, and return the density that size actually gives. */
  private async bake(split: MapSvgSplit, density: number) {
    const dpr = Math.min(window.devicePixelRatio || 1, TILE_RASTER_DPR_CAP);
    const baked = this.bakeableDensity(split, density);
    const surface = await rasterizeOrFallback(split.baseMarkup, split.base, split.width * baked * dpr, split.height * baked * dpr);
    return { surface, baked };
  }

  /** A tile is swapped for a smaller raster only once the density the view needs has fallen, so one that merely left the view keeps its raster for the pan back; an off-screen one is never baked sharper, since nothing shows it. */
  private needsRebake(key: string, entry: LoadedTile): boolean {
    const wanted = this.bakeableDensity(entry.split, this.density);
    return (this.onScreen.has(key) && wanted > entry.bakedDensity * TILE_REBAKE_RATIO) || wanted < entry.bakedDensity / TILE_REBAKE_RATIO;
  }

  /** Swap a loaded tile's raster for one baked for the density needed now, without refetching. A `clear()` landing mid-decode drops this one. */
  private async rebake(key: string, entry: LoadedTile): Promise<void> {
    const generation = this.generation;
    let result;
    try {
      result = await this.bake(entry.split, this.density);
    } catch {
      entry.rebaking = false;
      return;
    }
    entry.rebaking = false;
    if (generation !== this.generation || this.elements.get(key) !== entry) {
      result.surface.release();
      return;
    }
    entry.el.firstElementChild?.replaceWith(result.surface.el);
    entry.release();
    entry.release = result.surface.release;
    entry.bakedDensity = result.baked;
  }

  /** Remove every tile element this layer has added and forget its own fetch/load state, so a later `render()` re-fetches from scratch — the "no DOM" half of the fade-band contract above. Bumps `generation` unconditionally, even with nothing to remove, so a `load()` already in flight (always tracked in `elements` by the time it runs — see `render()`) is invalidated regardless of this call's own early return. */
  private clear(): void {
    this.generation++;
    if (this.elements.size === 0) return;
    for (const value of this.elements.values()) {
      if (value !== "loading" && value !== "failed") {
        value.el.remove();
        value.release();
      }
    }
    this.elements.clear();
    // Drops every still-queued key too — `drainQueue`'s own staleness check
    // would skip them anyway (gone from `elements`), this just stops the
    // array growing across repeated in/out-of-band-fade crossings over a
    // long session.
    this.queue.length = 0;
    this.rebakeQueue.length = 0;
  }

  /** Only the transform — called on every `render()`, including a plain pan where nothing has loaded, failed, or been removed, so this must never touch `mask-image` (see `applyEdgeMask`, called only on those load-state changes). */
  private position(el: HTMLElement, x: number, y: number, unitScale: number): void {
    const { translateX, translateY, scale } = tileOverlayTransform(this.options.origins[`${x},${y}`], unitScale, this.options.k);
    el.style.left = "0";
    el.style.top = "0";
    el.style.transformOrigin = "0 0";
    el.style.transform = `translate(${translateX}px, ${translateY}px) scale(${scale})`;
  }

  /** The manifest's own cells, minus any that have already `"failed"` to load — the "neighbour exists at all" `tileEdgeMask` needs for its outer-edge fade: a cell the manifest never listed and a cell whose fetch permanently failed read the same way here, both absent, so either one fades the shared edge instead of ending it hard. A cell still `"loading"` (or not yet attempted) stays present — it may yet load, so its edge is not treated as outer. */
  private nonFailedManifestCells(): Array<[number, number]> {
    return this.options.availableTiles.filter(([cx, cy]) => this.elements.get(`${cx},${cy}`) !== "failed");
  }

  /** Recompute and (re)apply `mask-image` for `el`, the tile wrapper at `(x, y)` — the only place it is ever written, so it changes only by a call here, never on a plain `render()` pan. The mask works on the wrapper's own CSS box, which `load()` sizes to the tile's native canvas dimensions directly, so a fade distance in canvas units (`tileEdgeMask`) lands in the right place regardless of `unitScale`/zoom, and fades the wrapper's raster AND its rivers overlay together as one composited unit. A tile is never clipped toward a neighbour: a fractional clip edge antialiases into a one-pixel light line, and the tiles carry no filter edge effects that would need hiding. */
  private applyEdgeMask(el: HTMLElement, x: number, y: number): void {
    const mask = tileEdgeMask(x, y, this.nonFailedManifestCells(), this.options.k);
    el.style.maskImage = mask ?? "";
    el.style.webkitMaskImage = mask ?? "";
    if (mask) {
      el.style.maskComposite = "intersect";
      el.style.webkitMaskComposite = "source-in";
    }
  }

  /** Re-run `applyEdgeMask` for each of `(x, y)`'s own up-to-four loaded neighbours — called when `(x, y)` fails to load, the one moment that changes what a NEIGHBOUR should fade at the shared edge (a failed cell is no longer present), independent of any camera move. */
  private refreshNeighbourEdgeMask(x: number, y: number): void {
    for (const [nx, ny] of [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]] as const) {
      const neighbour = this.elements.get(`${nx},${ny}`);
      if (neighbour && neighbour !== "loading" && neighbour !== "failed") this.applyEdgeMask(neighbour.el, nx, ny);
    }
  }
}
