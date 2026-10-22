/**
 * layers.ts — the detail layers a site declares (`layers.json`).
 *
 * A layer has a geographic extent, the deepest web-map zoom it has data for
 * and a credit line. Inside an extent the explorer may zoom as deep as the
 * layer's `max_zoom`, and the credit of any layer in view is shown in the
 * figure's credit slot.
 */

import { openingMaxZoom } from "./camera";
import { project } from "./projection";
import type { Point, Rect, Viewport } from "./types";

export interface DetailLayer {
  name: string;
  /** `[west, south, east, north]` in degrees: where the layer has pixels. */
  extent: [number, number, number, number];
  /** `[west, south, east, north]` in degrees: the collection's frame, where zoom may reach `maxZoom` whether or not the layer covers the spot. The extent when the file names none. */
  frame?: [number, number, number, number];
  maxZoom: number;
  credit: string;
  /** The raster tile index, relative to the map directory; `null` for a layer with nothing to draw. */
  tiles: string | null;
}

/** `layers.json`'s body, validated: entries that are not well-formed are dropped, and anything that is not a version-1 file yields no layers. */
export function parseLayers(body: unknown): DetailLayer[] {
  const file = body as { version?: unknown; layers?: unknown } | null;
  if (!file || file.version !== 1 || !Array.isArray(file.layers)) return [];
  const layers: DetailLayer[] = [];
  for (const raw of file.layers as Array<Record<string, unknown>>) {
    const extent = raw?.extent;
    if (!isBox(extent)) continue;
    if (typeof raw.max_zoom !== "number" || !Number.isFinite(raw.max_zoom)) continue;
    layers.push({
      name: typeof raw.name === "string" ? raw.name : "",
      extent: extent as DetailLayer["extent"],
      frame: (isBox(raw.frame) ? raw.frame : extent) as DetailLayer["frame"],
      maxZoom: raw.max_zoom,
      credit: typeof raw.credit === "string" ? raw.credit.trim() : "",
      tiles: typeof raw.tiles === "string" && raw.tiles !== "" ? raw.tiles : null,
    });
  }
  return layers;
}

/** Same-origin fetch, matching every other site runtime's asset-loading guard: the handshake's URLs are always this build's own output, never a third party. `force-cache` because every URL sits under a content-hashed map directory, so a cached response is always right to reuse. */
export async function fetchLocal(url: string, init: RequestInit = {}): Promise<Response> {
  const parsed = new URL(url, location.href);
  if (parsed.origin !== location.origin) throw new Error("places explorer assets must be same-origin");
  const response = await fetch(parsed.href, { credentials: "same-origin", cache: "force-cache", ...init });
  if (!response.ok) throw new Error(`asset HTTP ${response.status}`);
  return response;
}

/** Degrees of longitude per CSS px at web-map zoom `zoom` (256 px tiles). */
export function degreesPerPxAtZoom(zoom: number): number {
  return 360 / (256 * 2 ** zoom);
}

function isBox(value: unknown): value is [number, number, number, number] {
  return Array.isArray(value) && value.length === 4 && value.every((n) => typeof n === "number" && Number.isFinite(n));
}

/** A `[west, south, east, north]` box (the extent unless `box` names another) as a world-space rectangle; the projection is monotonic in both axes, so two corners bound it. */
export function worldRect(layer: DetailLayer, box: DetailLayer["extent"] = layer.extent): Rect {
  const [west, south, east, north] = box;
  const topLeft = project(north, west);
  const bottomRight = project(south, east);
  return { x: topLeft.x, y: topLeft.y, width: bottomRight.x - topLeft.x, height: bottomRight.y - topLeft.y };
}

/** Whether two world rectangles share any point, edges included (so a zero-size rectangle is a point test). */
function intersects(a: Rect, b: Rect): boolean {
  return a.x <= b.x + b.width && b.x <= a.x + a.width && a.y <= b.y + b.height && b.y <= a.y + a.height;
}

/** The deepest zoom multiplier a layer allows in `viewport` where its frame contains the world point `anchor`, or overlaps the bounds of several points, or `null` when no layer does. Overlap, not containment, for several points: a collection is rarely covered whole, and the ceiling only caps how deep their own fit may go. */
export function layerCeilingZoom(layers: readonly DetailLayer[], viewport: Viewport, anchor: Point | readonly Point[]): number | null {
  const points = Array.isArray(anchor) ? (anchor as readonly Point[]) : [anchor as Point];
  if (points.length === 0) return null;
  const xs = points.map((p) => p.x);
  const ys = points.map((p) => p.y);
  const bounds: Rect = { x: Math.min(...xs), y: Math.min(...ys), width: Math.max(...xs) - Math.min(...xs), height: Math.max(...ys) - Math.min(...ys) };
  let deepest: number | null = null;
  for (const layer of layers) {
    const r = worldRect(layer, layer.frame);
    if (!intersects(r, bounds)) continue;
    const zoom = openingMaxZoom(viewport, degreesPerPxAtZoom(layer.maxZoom));
    if (deepest === null || zoom > deepest) deepest = zoom;
  }
  return deepest;
}

/** The credit line of every layer whose extent intersects the world-space `view` rectangle, joined with " · "; empty when none does. */
export function creditInView(layers: readonly DetailLayer[], view: Rect): string {
  const credits: string[] = [];
  for (const layer of layers) {
    if (!layer.credit || credits.includes(layer.credit)) continue;
    const r = worldRect(layer);
    if (!intersects(r, view)) continue;
    credits.push(layer.credit);
  }
  return credits.join(" · ");
}
