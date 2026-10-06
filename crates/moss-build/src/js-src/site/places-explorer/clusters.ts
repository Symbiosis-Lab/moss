/**
 * clusters.ts — grouping nearby points into one marker, and laying a
 * coincident group's members out on a ring once zooming can never pull them
 * apart.
 *
 * Screen delta between any two points is exactly their world-space delta
 * times zoom, independent of pan or viewport centre — so "would these merge
 * at zoom Z" never needs a real camera. Every function below that takes a
 * `zoom` computes that pan-independent screen position internally;
 * `camera.ts`'s own pan/centre never enters this file, matching the source
 * prototype's own split between a zoom-only `projectedScreen` and the full
 * camera-anchored `screenPoint`. A visible-viewport bounds filter the
 * prototype's own clustering applied before merging is left out here on
 * purpose: it is a DOM/viewport concern (which points are even on screen),
 * not a geometry one, and nothing this port tests depends on it.
 */

import type { Point } from "./types";

/**
 * Screen px within which two markers merge into one, the prototype's own
 * tuned value — unlike `DETAIL_MAX_SCALE`, this needs no re-derivation for
 * the 1000x560-to-720x480 canvas correction: it is a final on-screen CSS-px
 * threshold, compared against positions already multiplied by the full
 * world-unit-to-CSS-px scale (`camera.ts`'s `screenScale`/`coverScale`),
 * and `coverScale` is defined as `viewport / worldSize`, so it cancels
 * whatever world canvas size this module uses. 43 CSS px stays 43 CSS px
 * regardless of the world canvas's own width/height.
 */
export const CLUSTER_DISTANCE = 43;
/** The most members a ring ever lays out individually; a larger group collapses to a count instead. */
export const RING_MAX = 8;
/** Zoom step `separationZoom`/`ringZoom` search by. */
export const RING_ZOOM_STEP = 0.25;

/** A point with a stable identity, in whatever coordinate space the caller documents. */
export interface IdPoint {
  id: string;
  x: number;
  y: number;
}

/** One already-screen-projected item `mergeByProximity` groups. */
export interface ScreenItem {
  id: string;
  screen: Point;
}

/** The result of merging: member ids (insertion order) and the cluster's average screen position. */
export interface ProximityCluster {
  ids: string[];
  screen: Point;
}

/**
 * Greedy nearest-cluster-centroid merge: for each item in a canonical order
 * (sorted by `id`, never the caller's own array order), join the nearest
 * existing cluster whose centroid is within `threshold`, else start a new
 * one. Shared by every caller below — `clusterVisible`'s on-screen grouping
 * and the hypothetical "would this set merge at another zoom" probes
 * (`staysMergedAtMaxZoom`, `separationZoom`, `ringZoom`) — so a group any
 * probe judges coincident is guaranteed to merge the same way for real.
 *
 * Sorting by `id` first (rather than trusting input order) makes the result
 * deterministic for a given point set: `places.<hash>.json`'s own array
 * order is incidental (it falls out of document scan order), so without
 * this, the exact same points could cluster differently from one build to
 * the next, or across a page's own re-fetch, purely because the greedy pass
 * is order-sensitive — the first item in any arrival order claims a
 * cluster's centroid and can pull a borderline third point in or push it
 * out depending on who got there first.
 */
export function mergeByProximity(items: ScreenItem[], threshold: number): ProximityCluster[] {
  const ordered = [...items].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const clusters: { ids: string[]; sum: Point }[] = [];
  for (const item of ordered) {
    let target: { ids: string[]; sum: Point } | null = null;
    let nearest = threshold;
    for (const cluster of clusters) {
      const cx = cluster.sum.x / cluster.ids.length;
      const cy = cluster.sum.y / cluster.ids.length;
      const distance = Math.hypot(item.screen.x - cx, item.screen.y - cy);
      if (distance < nearest) {
        target = cluster;
        nearest = distance;
      }
    }
    if (target) {
      target.ids.push(item.id);
      target.sum.x += item.screen.x;
      target.sum.y += item.screen.y;
    } else {
      clusters.push({ ids: [item.id], sum: { ...item.screen } });
    }
  }
  return clusters.map((cluster) => ({
    ids: cluster.ids,
    screen: { x: cluster.sum.x / cluster.ids.length, y: cluster.sum.y / cluster.ids.length },
  }));
}

/** Zoom-scaled, pan-independent screen position: only relative distances matter for clustering, so the viewport-centring offset the camera would add is left out. */
function projectedScreen(point: Point, zoom: number): Point {
  return { x: point.x * zoom, y: point.y * zoom };
}

/** Group `points` as they would actually cluster at `zoom`. */
export function clusterVisible(points: IdPoint[], zoom: number, threshold: number = CLUSTER_DISTANCE): ProximityCluster[] {
  const items = points.map((point) => ({ id: point.id, screen: projectedScreen(point, zoom) }));
  return mergeByProximity(items, threshold);
}

/**
 * True when zooming as far as the camera allows still can't pull `members`'
 * screen positions more than `threshold` apart — they can never be
 * separated by zooming to fit, so the caller must render them as one
 * coincident group (a ring) instead.
 */
export function staysMergedAtMaxZoom(members: IdPoint[], maxZoom: number, threshold: number = CLUSTER_DISTANCE): boolean {
  if (members.length <= 1) return true;
  return clusterVisible(members, maxZoom, threshold).length === 1;
}

/**
 * Smallest zoom (starting from `floor`, since zooming out never helps
 * separation) at which every one of `points` forms its own cluster, none
 * still within `threshold` of another. Capped at `ceiling`; returns `floor`
 * unchanged when the points already separate there or never separate under
 * the cap, the same "leave them merged, the ring handles it" fallback the
 * cap exists for.
 */
export function separationZoom(
  points: IdPoint[],
  floor: number,
  ceiling: number,
  threshold: number = CLUSTER_DISTANCE,
  step: number = RING_ZOOM_STEP
): number {
  if (points.length <= 1) return floor;
  const searchCeiling = Math.max(floor, ceiling);
  for (let zoom = floor; zoom <= searchCeiling; zoom += step) {
    if (clusterVisible(points, zoom, threshold).length === points.length) return zoom;
  }
  return floor;
}

function clusterKey(ids: string[]): string {
  return [...ids].sort().join("|");
}

/**
 * Smallest zoom (starting from `startZoom`) at which `target`'s own members
 * form a cluster with no outside neighbours from `allPoints`, capped at
 * `ceiling` so a ring never forces the zoom past where the basemap still
 * renders cleanly.
 */
export function ringZoom(target: IdPoint[], allPoints: IdPoint[], startZoom: number, ceiling: number, threshold: number = CLUSTER_DISTANCE, step: number = RING_ZOOM_STEP): number {
  const targetKey = clusterKey(target.map((point) => point.id));
  const searchCeiling = Math.max(startZoom, ceiling);
  for (let zoom = startZoom; zoom <= searchCeiling; zoom += step) {
    const isolated = clusterVisible(allPoints, zoom, threshold).some((cluster) => clusterKey(cluster.ids) === targetKey);
    if (isolated) return zoom;
  }
  return searchCeiling;
}

/** A coincident group's ring layout: `positions` for 2..RING_MAX members, or `null` (count only) for a larger group. */
export interface RingLayout {
  count: number;
  positions: Point[] | null;
}

/**
 * Ring radius for a group of `count` members, the prototype's own tuned
 * curve: constant up to 5, growing to a wider ring at `RING_MAX`. In CSS
 * px, laid out around a cluster's already-screen-space centroid — like
 * `CLUSTER_DISTANCE`, this does not depend on the world canvas's own size.
 */
function ringRadius(count: number): number {
  const MIN_RADIUS = 52;
  const MAX_RADIUS = 64;
  const MIN_SPREAD_COUNT = 5;
  if (count <= MIN_SPREAD_COUNT) return MIN_RADIUS;
  return MIN_RADIUS + ((count - MIN_SPREAD_COUNT) * (MAX_RADIUS - MIN_RADIUS)) / (RING_MAX - MIN_SPREAD_COUNT);
}

/** Lay `count` members evenly on a circle of `radius` around the origin, starting at 12 o'clock and going clockwise — document order, so the ring order is stable across calls. */
function ringOffsets(count: number, radius: number): Point[] {
  return Array.from({ length: count }, (_, index) => {
    const degrees = -90 + (360 * index) / count;
    const radians = (degrees * Math.PI) / 180;
    return { x: radius * Math.cos(radians), y: radius * Math.sin(radians) };
  });
}

/** Lay a coincident group of `count` members on a ring around `center`, or report just the count when the group is too large for one (`count > RING_MAX`). */
export function ringLayout(count: number, center: Point): RingLayout {
  if (count > RING_MAX) return { count, positions: null };
  const radius = ringRadius(count);
  const positions = ringOffsets(count, radius).map((offset) => ({ x: center.x + offset.x, y: center.y + offset.y }));
  return { count, positions };
}
