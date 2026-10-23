/**
 * layer-raster.ts — a historical layer's raster tiles, drawn under the
 * markers and clipped to the plates' printed edges.
 *
 * One `<svg>` placed inside the world element: the camera's transform (its
 * zoom, pan and true-aspect scaling) moves it exactly as it moves the land, so
 * a tile and a footprint vertex land where the markers' projection says. The
 * svg has no viewBox of its own: a viewBox maps through the element's laid-out
 * size, which the browser rounds to 1/64 px, and the camera's zoom multiplies
 * that rounding into many screen pixels. A scaling `<g>` carries the one
 * number the layout does depend on, the world unit in px. Each layer is a
 * `<g>` clipped by the union of its footprint polygons; its tiles are
 * `<image>` elements in world units, grouped by level so a coarser tile always
 * draws under a finer one.
 *
 * A layer's tile index is fetched once, the first time its extent is in view,
 * and a bad index leaves the layer undrawn. Tiles are fetched through an
 * `AbortController`, so one that leaves the view stops loading. Until a
 * wanted tile has loaded, its nearest loaded ancestor (or, zooming out, its
 * loaded descendants) stands in.
 */
import { screenScale, viewWorldRect } from "./camera";
import { fetchLocal, worldRect, type DetailLayer } from "./layers";
import { WORLD_WIDTH } from "./projection";
import type { Camera, Point, Rect, Viewport } from "./types";

const SVG_NS = "http://www.w3.org/2000/svg";
const TILE_PX = 256;
/** Tiles are fetched this many at a time; the rest wait for a slot. */
const MAX_CONCURRENT_LOADS = 6;
/** CSS px added to the view before choosing tiles: a tile just past the edge is worth having before a pan reaches it. */
const VIEW_PAD_PX = 96;
/** Loaded tiles kept (shown or not) before the longest-unused are released. */
const MAX_RETAINED_TILES = 160;
const DPR_CAP = 2;

export interface LayerIndex {
  minLevel: number;
  maxLevel: number;
  /** The tiles that exist at each level, as `"i,j"`. */
  levels: Map<number, Set<string>>;
  /** Closed polygons in world coordinates, one per plate. */
  footprints: Point[][];
}

export interface TileRef {
  level: number;
  i: number;
  j: number;
}

const keyOf = ({ level, i, j }: TileRef): string => `${level}/${i}/${j}`;
const isInt = (n: unknown): n is number => typeof n === "number" && Number.isInteger(n) && n >= 0;

/** A layer's `index.json`, validated; anything that is not a well-formed 256 px index yields `null`, and the layer is not drawn. */
export function parseLayerIndex(body: unknown): LayerIndex | null {
  const raw = body as Record<string, unknown> | null;
  if (!raw || !isInt(raw.min_level) || !isInt(raw.max_level) || raw.min_level > raw.max_level || raw.max_level > 24 || raw.tile_size !== TILE_PX) return null;
  if (!raw.levels || typeof raw.levels !== "object" || !Array.isArray(raw.footprints)) return null;
  const levels = new Map<number, Set<string>>();
  for (const [name, cells] of Object.entries(raw.levels as Record<string, unknown>)) {
    const level = Number(name);
    if (!isInt(level) || !Array.isArray(cells)) return null;
    const set = new Set<string>();
    for (const cell of cells) {
      if (!Array.isArray(cell) || !isInt(cell[0]) || !isInt(cell[1])) return null;
      set.add(`${cell[0]},${cell[1]}`);
    }
    levels.set(level, set);
  }
  const footprints: Point[][] = [];
  for (const footprint of raw.footprints as Array<{ polygon?: unknown }>) {
    const polygon = footprint?.polygon;
    if (!Array.isArray(polygon) || polygon.length < 3) return null;
    const points: Point[] = [];
    for (const vertex of polygon) {
      if (!Array.isArray(vertex) || !Number.isFinite(vertex[0]) || !Number.isFinite(vertex[1])) return null;
      points.push({ x: vertex[0], y: vertex[1] });
    }
    footprints.push(points);
  }
  return { minLevel: raw.min_level, maxLevel: raw.max_level, levels, footprints };
}

/** World units a level's tile spans on each side. */
const tileSpan = (level: number): number => WORLD_WIDTH / 2 ** level;

/**
 * The level whose world units per pixel is nearest the view's, never past the
 * index's own range; `null` when the view is coarser than the shallowest
 * level by more than a factor of 2. `worldPerPx` is the view's world units per
 * device pixel.
 */
export function levelForView(index: Pick<LayerIndex, "minLevel" | "maxLevel">, worldPerPx: number): number | null {
  const exact = Math.log2(WORLD_WIDTH / (TILE_PX * worldPerPx));
  if (!(exact >= index.minLevel - 1)) return null;
  return Math.min(index.maxLevel, Math.max(index.minLevel, Math.round(exact)));
}

/**
 * The tiles that cover `rect` at `level`. A cell the index does not list (the
 * deepest levels exist only near places) is covered by its finest existing
 * ancestor instead, so the view is never left with a hole at the chosen level.
 */
export function tilesForRect(index: LayerIndex, level: number, rect: Rect): TileRef[] {
  const size = tileSpan(level);
  const last = 2 ** level - 1;
  const i0 = Math.max(0, Math.floor(rect.x / size));
  const i1 = Math.min(last, Math.floor((rect.x + rect.width) / size));
  const j0 = Math.max(0, Math.floor(rect.y / size));
  const j1 = Math.min(last, Math.floor((rect.y + rect.height) / size));
  const wanted = new Map<string, TileRef>();
  for (let j = j0; j <= j1; j++) {
    for (let i = i0; i <= i1; i++) {
      for (let l = level; l >= index.minLevel; l--) {
        const shift = level - l;
        if (!index.levels.get(l)?.has(`${i >> shift},${j >> shift}`)) continue;
        const ref = { level: l, i: i >> shift, j: j >> shift };
        wanted.set(keyOf(ref), ref);
        break;
      }
    }
  }
  return [...wanted.values()];
}

/** For each wanted tile not loaded, the loaded tile that stands in for it: its nearest loaded ancestor, else every loaded descendant. */
export function standIns(wanted: readonly TileRef[], loaded: ReadonlyMap<string, TileRef>): TileRef[] {
  const shown = new Map<string, TileRef>();
  for (const want of wanted) {
    if (loaded.has(keyOf(want))) continue;
    let ancestor: TileRef | undefined;
    for (let shift = 1; shift <= want.level && !ancestor; shift++) {
      ancestor = loaded.get(keyOf({ level: want.level - shift, i: want.i >> shift, j: want.j >> shift }));
    }
    if (ancestor) {
      shown.set(keyOf(ancestor), ancestor);
      continue;
    }
    for (const ref of loaded.values()) {
      const shift = ref.level - want.level;
      if (shift > 0 && ref.i >> shift === want.i && ref.j >> shift === want.j) shown.set(keyOf(ref), ref);
    }
  }
  return [...shown.values()];
}

/** A polygon as an SVG `points` value, in world coordinates. */
export const polygonPoints = (polygon: readonly Point[]): string => polygon.map((p) => `${p.x},${p.y}`).join(" ");

type TileEntry =
  | { kind: "loading"; abort: AbortController }
  | { kind: "ready"; ref: TileRef; el: SVGImageElement; href: string; used: number }
  | { kind: "failed" };

interface LayerState {
  layer: DetailLayer;
  /** Directory of the layer's index; tiles are `<L>/<i>/<j>.jpg` beneath it. */
  dir: string;
  indexUrl: string;
  status: "idle" | "loading" | "ready" | "failed";
  index: LayerIndex | null;
  group: SVGGElement;
  levelGroups: Map<number, SVGGElement>;
  entries: Map<string, TileEntry>;
  /** Whether the last reconcile chose a level and a plate's footprint is in view: the layer is drawing something. */
  drawing: boolean;
}

export class LayerRaster {
  readonly el: SVGSVGElement;
  private readonly states: LayerState[];
  private readonly defs: SVGDefsElement;
  /** Scales world units to the world element's px; every layer's group is inside it. */
  private readonly units: SVGGElement;
  private view: { camera: Camera; viewport: Viewport } | null = null;
  private active = 0;
  private tick = 0;
  /** Called when the set of layers that draw something changes (a layer's index arriving included). */
  onDrawingChange: (() => void) | null = null;

  /** `baseUrl` is the map directory the layers' relative `tiles` URLs resolve against. */
  constructor(layers: readonly DetailLayer[], baseUrl: string) {
    this.el = document.createElementNS(SVG_NS, "svg");
    this.el.setAttribute("aria-hidden", "true");
    this.el.setAttribute("focusable", "false");
    this.el.dataset.mossPlacesLayers = "";
    this.el.style.cssText = "position:absolute;inset:0;width:100%;height:100%;pointer-events:none";
    this.defs = document.createElementNS(SVG_NS, "defs");
    this.units = document.createElementNS(SVG_NS, "g");
    this.el.append(this.defs, this.units);
    const base = new URL(baseUrl, location.href);
    this.states = layers
      .filter((layer) => layer.tiles)
      .map((layer) => {
        const indexUrl = new URL(layer.tiles as string, base).href;
        const group = document.createElementNS(SVG_NS, "g");
        group.dataset.layer = layer.name;
        this.units.append(group);
        return { layer, indexUrl, dir: indexUrl.slice(0, indexUrl.lastIndexOf("/") + 1), status: "idle", index: null, group, levelGroups: new Map(), entries: new Map(), drawing: false };
      });
  }

  /** `unitScale` is the world element's px per world unit at zoom 1. */
  render(camera: Camera, viewport: Viewport, unitScale: number): void {
    this.view = { camera, viewport };
    this.units.setAttribute("transform", `scale(${unitScale})`);
    this.reconcile();
  }

  private reconcile(): void {
    if (!this.view) return;
    const { camera, viewport } = this.view;
    this.tick++;
    const view = viewWorldRect(camera, viewport);
    const padded = viewWorldRect(camera, viewport, VIEW_PAD_PX);
    const worldPerPx = 1 / (screenScale(camera, viewport) * Math.min(window.devicePixelRatio || 1, DPR_CAP));
    let busy = false;
    let drawingChanged = false;
    for (const state of this.states) {
      if (state.status === "idle" && overlaps(worldRect(state.layer), view)) void this.loadIndex(state);
      if (state.status === "loading") busy = true;
      if (state.status !== "ready" || !state.index) continue;
      const level = overlaps(worldRect(state.layer), view) ? levelForView(state.index, worldPerPx) : null;
      const drawing = level !== null && state.index.footprints.some((footprint) => overlaps(boundsOf(footprint), view));
      if (drawing !== state.drawing) {
        state.drawing = drawing;
        drawingChanged = true;
      }
      const wanted = level === null ? [] : tilesForRect(state.index, level, padded);
      this.syncTiles(state, wanted, camera);
      busy ||= [...state.entries.values()].some((entry) => entry.kind === "loading") || wanted.some((ref) => !state.entries.has(keyOf(ref)));
    }
    this.el.dataset.mossPlacesLayersState = busy ? "busy" : "idle";
    if (drawingChanged) this.onDrawingChange?.();
  }

  /** Whether `layer` draws something at the camera last rendered: a level is chosen and a plate's footprint is in view. */
  draws(layer: DetailLayer): boolean {
    return this.states.some((state) => state.layer === layer && state.drawing);
  }

  private async loadIndex(state: LayerState): Promise<void> {
    state.status = "loading";
    try {
      const index = parseLayerIndex(await (await fetchLocal(state.indexUrl)).json());
      if (!index) throw new Error("not a tile index");
      state.index = index;
      state.status = "ready";
      const id = `moss-layer-clip-${this.states.indexOf(state)}`;
      const clip = document.createElementNS(SVG_NS, "clipPath");
      clip.id = id;
      // One element per plate: a clip path is the union of its children, whatever their winding.
      for (const footprint of index.footprints) {
        const polygon = document.createElementNS(SVG_NS, "polygon");
        polygon.setAttribute("points", polygonPoints(footprint));
        clip.append(polygon);
      }
      this.defs.append(clip);
      state.group.setAttribute("clip-path", `url(#${id})`);
    } catch (error) {
      state.status = "failed";
      console.warn("[moss] places explorer layer index could not load", error);
    }
    this.reconcile();
  }

  /** Make the tiles shown, loading and kept match the `wanted` set for the current view. */
  private syncTiles(state: LayerState, wanted: readonly TileRef[], camera: Camera): void {
    const wantedKeys = new Set(wanted.map(keyOf));
    const loaded = new Map<string, TileRef>();
    for (const [key, entry] of state.entries) {
      if (entry.kind === "ready") loaded.set(key, entry.ref);
      // A tile that left the view stops loading.
      else if (entry.kind === "loading" && !wantedKeys.has(key)) {
        entry.abort.abort();
        state.entries.delete(key);
      }
    }
    const shown = new Set([...wanted.filter((ref) => loaded.has(keyOf(ref))), ...standIns(wanted, loaded)].map(keyOf));
    for (const [key, entry] of state.entries) {
      if (entry.kind !== "ready") continue;
      if (shown.has(key)) {
        entry.used = this.tick;
        this.attach(state, entry);
      } else entry.el.remove();
    }
    // Nearest the centre first.
    const byDistance = (ref: TileRef): number => {
      const size = tileSpan(ref.level);
      return Math.hypot((ref.i + 0.5) * size - camera.x, (ref.j + 0.5) * size - camera.y);
    };
    for (const ref of [...wanted].sort((a, b) => byDistance(a) - byDistance(b))) {
      if (this.active >= MAX_CONCURRENT_LOADS) break;
      if (!state.entries.has(keyOf(ref))) void this.loadTile(state, ref);
    }
    this.release(state);
  }

  private attach(state: LayerState, entry: Extract<TileEntry, { kind: "ready" }>): void {
    const { level } = entry.ref;
    let group = state.levelGroups.get(level);
    if (!group) {
      group = document.createElementNS(SVG_NS, "g");
      group.dataset.level = String(level);
      const finer = [...state.levelGroups].filter(([l]) => l > level).sort(([a], [b]) => a - b)[0];
      state.group.insertBefore(group, finer ? finer[1] : null);
      state.levelGroups.set(level, group);
    }
    if (entry.el.parentNode !== group) group.append(entry.el);
  }

  /** Drop the longest-unused loaded tiles beyond the retention cap. */
  private release(state: LayerState): void {
    const ready = [...state.entries].filter((pair): pair is [string, Extract<TileEntry, { kind: "ready" }>] => pair[1].kind === "ready");
    if (ready.length <= MAX_RETAINED_TILES) return;
    ready.sort(([, a], [, b]) => a.used - b.used);
    for (const [key, entry] of ready.slice(0, ready.length - MAX_RETAINED_TILES)) {
      if (entry.used === this.tick) break;
      entry.el.remove();
      URL.revokeObjectURL(entry.href);
      state.entries.delete(key);
    }
  }

  private async loadTile(state: LayerState, ref: TileRef): Promise<void> {
    const key = keyOf(ref);
    const abort = new AbortController();
    const mine: TileEntry = { kind: "loading", abort };
    state.entries.set(key, mine);
    this.active++;
    let href: string | null = null;
    try {
      const response = await fetchLocal(`${state.dir}${ref.level}/${ref.i}/${ref.j}.jpg`, { signal: abort.signal });
      const blob = await response.blob();
      href = URL.createObjectURL(blob);
      const probe = new Image();
      probe.src = href;
      await probe.decode?.();
      if (state.entries.get(key) !== mine) throw new Error("superseded");
      const size = tileSpan(ref.level);
      const el = document.createElementNS(SVG_NS, "image");
      // Half a tile pixel of overlap hides the seam two abutting tiles would show at a fractional position.
      const bleed = size / TILE_PX / 2;
      el.setAttribute("x", String(ref.i * size));
      el.setAttribute("y", String(ref.j * size));
      el.setAttribute("width", String(size + bleed));
      el.setAttribute("height", String(size + bleed));
      el.setAttribute("preserveAspectRatio", "none");
      el.setAttribute("href", href);
      el.dataset.tile = key;
      state.entries.set(key, { kind: "ready", ref, el, href, used: this.tick });
      href = null;
    } catch {
      if (href) URL.revokeObjectURL(href);
      // An abort or a replaced entry already removed this request's bookkeeping; a real failure is cached and not retried.
      if (state.entries.get(key) === mine) state.entries.set(key, { kind: "failed" });
    } finally {
      this.active--;
      this.reconcile();
    }
  }
}

function boundsOf(polygon: readonly Point[]): Rect {
  const xs = polygon.map((p) => p.x);
  const ys = polygon.map((p) => p.y);
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}

function overlaps(a: Rect, b: Rect): boolean {
  return a.x <= b.x + b.width && a.x + a.width >= b.x && a.y <= b.y + b.height && a.y + a.height >= b.y;
}
