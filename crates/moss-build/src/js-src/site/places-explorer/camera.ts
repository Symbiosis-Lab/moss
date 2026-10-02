/**
 * camera.ts — the places-explorer's pan/zoom state.
 *
 * `Camera.zoom` is a multiplier over the "cover" baseline (fit the world's
 * longer-relative-to-viewport axis exactly, crop the other), so `zoom === 1`
 * is always the fully-zoomed-out view with no empty band on any side, the
 * same meaning a prototype of this map gave its own `MIN_ZOOM`. `Camera.x`/
 * `Camera.y` are the world-space (projected) point centred in the viewport —
 * unlike that prototype's own camera state, which stored a pixel pan offset
 * that had to be rescaled by hand on every zoom change to keep the same
 * point centred. Storing the centre directly makes that rescale a
 * non-problem: resizing or re-zooming never moves `x`/`y` on its own.
 *
 * The detail ceiling (`DETAIL_MAX_SCALE`) is a screen scale — CSS px covered
 * by one world unit — not a zoom multiplier, so a narrow embed and a full
 * page stop at the same actually-rendered detail instead of the embed's
 * lower zoom number quietly meaning coarser detail.
 */

import { WORLD_WIDTH, WORLD_HEIGHT } from "./projection";
import type { Camera, Point, Rect, Viewport } from "./types";

/** `viewport` as a `Rect` at the origin — the default "free rectangle" for a caller that has no card row, controls or chip to dodge (every test fixture, and any `fitPoints`/`coverCamera` call this module's own tests make directly). */
function wholeViewport(viewport: Viewport): Rect {
  return { x: 0, y: 0, width: viewport.width, height: viewport.height };
}

/**
 * Shift a world-space centre so it lands on `frame`'s own centre on screen
 * rather than the viewport's. `worldToScreen` always centres `camera.x`/
 * `camera.y` in the VIEWPORT, so fitting points into a smaller "free
 * rectangle" (the part of the map the card row, controls and chip leave
 * uncovered — `map.ts`'s own `freeFrame`) needs this one correction applied
 * wherever a camera is built FROM a target world point, rather than
 * threading the frame through `worldToScreen` itself and disturbing every
 * other reader of the camera (markers, gestures, the saved-camera URL).
 */
function centerOnFrame(center: Point, viewport: Viewport, frame: Rect, scale: number): Point {
  const dx = frame.x + frame.width / 2 - viewport.width / 2;
  const dy = frame.y + frame.height / 2 - viewport.height / 2;
  return { x: center.x - dx / scale, y: center.y - dy / scale };
}

/**
 * The span, on one axis, a viewport would need to be — centred on `frame`'s
 * own centre instead of its own — for covering THAT virtual viewport to
 * guarantee covering the real one too. The frame's centre sits some
 * distance from each of the real viewport's two edges; the farther of the
 * two, doubled, is that virtual span, because a symmetric cover around the
 * frame's centre reaches exactly that far either way. Covering a span this
 * size (or larger) is therefore sufficient — proven, not approximated — for
 * covering `viewportSpan` once the camera re-centres on `frame` rather than
 * the viewport, which is what every caller below uses it for.
 */
function virtualSpan(viewportSpan: number, frameStart: number, frameSize: number): number {
  const center = frameStart + frameSize / 2;
  return 2 * Math.max(center, viewportSpan - center);
}

/**
 * The zoom (as a multiplier over `viewport`'s own cover baseline — see this
 * module's own doc) that guarantees the world still covers the WHOLE
 * viewport with no empty band once the camera centres on `frame` instead of
 * the viewport's own centre. Never below `MIN_ZOOM`: `frame` reducing to the
 * whole viewport (`wholeViewport`, every caller's default) always returns
 * exactly the ordinary cover floor, unchanged from before this parameter
 * existed.
 *
 * A blank band OUTSIDE `frame` but still inside the viewport is fine — it
 * sits behind the card row / controls / chip `frame` exists to dodge, so a
 * reader never sees it — but the ordinary "zoom only as high as you must"
 * floor (plain `coverScale`) has no way to know that, and can leave a gap
 * at the viewport's own FAR edge once the centre shifts toward frame. This
 * is the one place that extra zoom is computed, shared by `coverCamera` and
 * `fitPoints` so neither can shift a camera into a visible gap the other
 * was careful to avoid.
 */
function frameZoomFloor(viewport: Viewport, frame: Rect): number {
  const viewportScale = coverScale(viewport);
  const virtualWidth = virtualSpan(viewport.width, frame.x, frame.width);
  const virtualHeight = virtualSpan(viewport.height, frame.y, frame.height);
  const requiredScale = Math.max(virtualWidth / WORLD_WIDTH, virtualHeight / WORLD_HEIGHT);
  return Math.max(MIN_ZOOM, requiredScale / viewportScale);
}

/** Where `point` (world-space) lands on screen, in CSS px from the viewport's own top-left — the one formula every layer that positions something against the live camera (markers, the ring, tile placement) shares, rather than re-deriving it. */
export function worldToScreen(point: Point, camera: Camera, viewport: Viewport): Point {
  const scale = screenScale(camera, viewport);
  return {
    x: viewport.width / 2 + (point.x - camera.x) * scale,
    y: viewport.height / 2 + (point.y - camera.y) * scale,
  };
}

/** Inverse of `worldToScreen` — a screen point (a click, a drag delta anchor) back to world-space, at the same camera/viewport. */
export function screenToWorld(point: Point, camera: Camera, viewport: Viewport): Point {
  const scale = screenScale(camera, viewport);
  return {
    x: camera.x + (point.x - viewport.width / 2) / scale,
    y: camera.y + (point.y - viewport.height / 2) / scale,
  };
}

export const MIN_ZOOM = 1;
/**
 * The prototype tuned this ceiling (7.21) against its own 1000x560 world
 * canvas. Degrees-per-world-unit is set by canvas height alone
 * (`projection.ts`'s `SCALE = WORLD_HEIGHT / 2 / pattersonY(PI / 2)`), so at
 * this project's real canvas (480 tall, not 560 — and, since the explorer
 * moved to the shared, uncropped world SVG, wider than it used to be, but
 * width never enters `SCALE`) one world unit now spans more degrees, and the
 * prototype's raw number would stop at a coarser final on-screen detail.
 * Scaling by the inverse height ratio (560 / 480) keeps the same
 * CSS-px-per-degree ceiling the prototype's tuned value produced; the width
 * change that derived `WORLD_WIDTH` from `SCALE` (see projection.ts) left
 * this ratio untouched.
 */
export const DETAIL_MAX_SCALE = 7.21 * (560 / 480);

/** Padding fraction/bounds `fitPoints` pads a fit by, in CSS px. Matches the prototype's own fit padding. */
const FIT_PAD_MIN = 36;
const FIT_PAD_MAX = 90;
const FIT_PAD_FRACTION = 0.12;
/** The smallest span `fitPoints` ever fits to, so a single point (zero span) still produces a finite zoom. */
const FIT_MIN_SPAN = 12;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/** The px-per-world-unit scale that makes the world cover `viewport` with no empty band, at `zoom === 1`. */
function coverScale(viewport: Viewport): number {
  return Math.max(viewport.width / WORLD_WIDTH, viewport.height / WORLD_HEIGHT);
}

/** Px-per-world-unit at `camera`'s current zoom. */
export function screenScale(camera: Camera, viewport: Viewport): number {
  return camera.zoom * coverScale(viewport);
}

/** The zoom multiplier at which `screenScale` reaches `DETAIL_MAX_SCALE` for this viewport. */
export function detailMaxZoom(viewport: Viewport): number {
  return DETAIL_MAX_SCALE / coverScale(viewport);
}

/**
 * The screen scale at which a regional tile is shown at native, 1:1
 * resolution: `k` times the world map's own ceiling, since a tile is drawn
 * in the SAME Patterson projection as the world map at `k` times its scale
 * and clipped to its own cell (`PattersonProjection::for_tile` in
 * `crates/moss-build/src/build/place_map/geometry.rs`) — past this, the
 * explorer would be upscaling a tile past its own detail, the same "don't
 * render past what the geometry actually resolves" reasoning
 * `DETAIL_MAX_SCALE` applies to the world map. `k` is read from
 * `tiles.json` (the build's own `TILE_K`), never hardcoded here, so the
 * two can never drift apart — see that file's own module doc.
 */
export function tileDetailMaxScale(k: number): number {
  return DETAIL_MAX_SCALE * k;
}

/** The zoom multiplier at which `screenScale` reaches `tileDetailMaxScale(k)` for this viewport — the raised ceiling once the camera has tiles to show. */
export function tileDetailMaxZoom(viewport: Viewport, k: number): number {
  return tileDetailMaxScale(k) / coverScale(viewport);
}

/** How far, in world units, the camera centre may sit from the world's own centre on one axis before an empty band would show, at the given scale. */
function overflow(worldSpan: number, viewportSpan: number, scale: number): number {
  return Math.max(0, (worldSpan - viewportSpan / scale) / 2);
}

/**
 * Clamp a camera to this viewport: zoom never below the cover floor nor past
 * `maxZoom` (the world ceiling by default; a caller with tiles in view
 * passes `tileDetailMaxZoom` instead), and the centre never panned far
 * enough to open an empty band on either axis. Keeps `x`/`y` unchanged
 * whenever they are already within bounds — the geographic centre survives
 * a viewport resize (or a zoom-only change) with no special-case rescale.
 */
export function clampCamera(camera: Camera, viewport: Viewport, maxZoom: number = detailMaxZoom(viewport)): Camera {
  // A container with no laid-out size yet (e.g. before its first
  // ResizeObserver callback) reports a 0x0 (or one-axis-zero) viewport.
  // `coverScale` would divide by that zero further down (`overflow`'s
  // `viewportSpan / scale`), propagating NaN into `x`/`y`. There is no
  // meaningful "no empty band" camera for a viewport with no area, so keep
  // the current centre and the cover zoom rather than computing one.
  if (viewport.width <= 0 || viewport.height <= 0) {
    return { x: camera.x, y: camera.y, zoom: MIN_ZOOM };
  }
  const zoom = clamp(camera.zoom, MIN_ZOOM, maxZoom);
  const scale = zoom * coverScale(viewport);
  const overflowX = overflow(WORLD_WIDTH, viewport.width, scale);
  const overflowY = overflow(WORLD_HEIGHT, viewport.height, scale);
  const x = clamp(camera.x, WORLD_WIDTH / 2 - overflowX, WORLD_WIDTH / 2 + overflowX);
  const y = clamp(camera.y, WORLD_HEIGHT / 2 - overflowY, WORLD_HEIGHT / 2 + overflowY);
  return { x, y, zoom };
}

/**
 * The centre of the fixed-width window (along one axis) covering the most
 * points. `values` may list the same coordinate more than once (one entry
 * per thing located there) so a denser coordinate naturally outweighs a
 * sparser one. One sorted two-pointer pass: for each candidate left edge
 * (anchored at a point, since an optimal fixed-width window always starts at
 * one), the right edge advances monotonically. Ties go to the window
 * closest to `fallbackMid`.
 */
export function windowCenter(values: number[], width: number, fallbackMid: number): number {
  if (values.length === 0) return fallbackMid;
  const sorted = [...values].sort((a, b) => a - b);
  let right = 0;
  let best: { count: number; center: number } | null = null;
  for (let left = 0; left < sorted.length; left++) {
    if (right < left) right = left;
    while (right < sorted.length && sorted[right] - sorted[left] <= width) right++;
    const count = right - left;
    const center = sorted[left] + width / 2;
    if (!best || count > best.count || (count === best.count && Math.abs(center - fallbackMid) < Math.abs(best.center - fallbackMid))) {
      best = { count, center };
    }
  }
  return best!.center;
}

/**
 * The initial/reset camera: cover zoom, panned toward the densest window of
 * `points` on whichever axis cover crops, clamped so no empty band ever
 * appears. With no points, centres on the world.
 *
 * `frame` is the "free rectangle" (`map.ts`'s own `freeFrame`) to centre the
 * densest window within — defaults to the whole viewport for a caller (a
 * bare embed, this module's own tests) with no card row, controls or chip
 * reserving part of it. The window itself is sized to `frame`, not the
 * viewport: a shorter free rectangle (the card row eating a quarter of the
 * map) admits fewer world units top-to-bottom, exactly as a shorter real
 * viewport would.
 */
export function coverCamera(points: Point[], viewport: Viewport, frame: Rect = wholeViewport(viewport)): Camera {
  const zoom = frameZoomFloor(viewport, frame);
  const scale = zoom * coverScale(viewport);
  const overflowX = overflow(WORLD_WIDTH, frame.width, scale);
  const overflowY = overflow(WORLD_HEIGHT, frame.height, scale);
  const xs = points.map((point) => point.x);
  const ys = points.map((point) => point.y);
  const x = overflowX > 0 ? windowCenter(xs, frame.width / scale, WORLD_WIDTH / 2) : WORLD_WIDTH / 2;
  const y = overflowY > 0 ? windowCenter(ys, frame.height / scale, WORLD_HEIGHT / 2) : WORLD_HEIGHT / 2;
  const centered = centerOnFrame({ x, y }, viewport, frame, scale);
  return clampCamera({ x: centered.x, y: centered.y, zoom }, viewport);
}

/**
 * Fit `points` inside `frame` (default: the whole viewport — see
 * `coverCamera`'s own doc) with padding sized to `frame`, zoomed in only as
 * far as needed (never out past the cover floor, never in past the detail
 * ceiling). A single point (zero span) still produces a finite result via
 * `FIT_MIN_SPAN`, which is what drives it all the way to the ceiling rather
 * than an unbounded zoom.
 *
 * Pulling a coincident or near-coincident pair of points apart beyond this
 * fit is the ring's own job (`clusters.ts`'s `separationZoom`), not this
 * function's: `fitPoints` only has to land every point inside the frame,
 * not guarantee each one reads as its own marker.
 */
export function fitPoints(
  points: Point[],
  viewport: Viewport,
  maxZoom: number = detailMaxZoom(viewport),
  frame: Rect = wholeViewport(viewport),
): Camera {
  if (points.length === 0) return coverCamera(points, viewport, frame);
  const viewportScale = coverScale(viewport);
  const screenXs = points.map((point) => point.x * viewportScale);
  const screenYs = points.map((point) => point.y * viewportScale);
  const minX = Math.min(...screenXs);
  const maxX = Math.max(...screenXs);
  const minY = Math.min(...screenYs);
  const maxY = Math.max(...screenYs);
  const pad = clamp(Math.min(frame.width, frame.height) * FIT_PAD_FRACTION, FIT_PAD_MIN, FIT_PAD_MAX);
  const dx = Math.max(FIT_MIN_SPAN, maxX - minX);
  const dy = Math.max(FIT_MIN_SPAN, maxY - minY);
  const fitZoom = Math.min((frame.width - pad * 2) / dx, (frame.height - pad * 2) / dy);
  // At least as much zoom as `coverCamera` would need for this same frame —
  // re-centring on `frame` instead of the viewport can only ever need MORE
  // zoom than fitting the points alone did, never less, or the shift below
  // would open a gap at the viewport's own far edge (`frameZoomFloor`'s own
  // doc).
  const zoom = Math.max(fitZoom, frameZoomFloor(viewport, frame));
  const scale = zoom * viewportScale;
  const centerX = (minX + maxX) / 2 / viewportScale;
  const centerY = (minY + maxY) / 2 / viewportScale;
  const centered = centerOnFrame({ x: centerX, y: centerY }, viewport, frame, scale);
  return clampCamera({ x: centered.x, y: centered.y, zoom }, viewport, maxZoom);
}
