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

/**
 * The camera for `to` that shows the same scale (CSS px per world unit) and
 * centre `camera` showed at `from`. `Camera.zoom` is relative to the
 * viewport's own cover scale, so a bare resize with the zoom untouched
 * silently re-frames the SAME geographic range at the new size: an embed
 * entering fullscreen got a magnified copy of its small view, not more map.
 * Keeping the scale instead lets a bigger viewport simply show more. A
 * camera already at the cover floor stays there (the whole-world view must
 * not turn into a zoomed-in one when the viewport shrinks).
 */
export function resizeCamera(camera: Camera, from: Viewport, to: Viewport): Camera {
  if (from.width <= 0 || from.height <= 0 || to.width <= 0 || to.height <= 0) return camera;
  if (camera.zoom <= MIN_ZOOM) return camera;
  return { ...camera, zoom: (camera.zoom * coverScale(from)) / coverScale(to) };
}

/** The zoom multiplier at which `screenScale` reaches `DETAIL_MAX_SCALE` for this viewport. */
export function detailMaxZoom(viewport: Viewport): number {
  return DETAIL_MAX_SCALE / coverScale(viewport);
}

/** Degrees of longitude one CSS px covers at the deepest zoom: about 500 m at the equator. The tiles hold about 0.01 degree per coordinate unit and the pack's own geometry stops near there, so zooming past this would only enlarge data that is no longer there. */
export const TILE_DETAIL_DEGREES_PER_PX = 0.005;

/**
 * The screen scale (CSS px per world unit) at the deepest zoom a reader can
 * reach once regional tiles are in view: one CSS px covers
 * `TILE_DETAIL_DEGREES_PER_PX` of longitude. A world unit is
 * `360 / WORLD_WIDTH` degrees of longitude, so this reads off the
 * projection's own width and no longer depends on how finely the build
 * quantises a tile (`tiles.json`'s `k`).
 */
export function tileDetailMaxScale(): number {
  return 360 / (TILE_DETAIL_DEGREES_PER_PX * WORLD_WIDTH);
}

/**
 * The deepest the opening view of all places (and Fit all places) zooms: one CSS px covers 0.015 degrees of longitude, so a 1280 px map opens about 19 degrees across. The bundled map data is simplified for frames about 10 degrees across, and below this the land turns into 10 to 30 px polygon steps and blocky relief, so the opening view stops where it still looks clean. A reader can still zoom in by hand up to the tile ceiling (`TILE_DETAIL_DEGREES_PER_PX`).
 */
export const OPENING_DEGREES_PER_PX = 0.015;

/** The zoom multiplier at which `screenScale` reaches the opening view's depth limit for this viewport. */
export function openingMaxZoom(viewport: Viewport): number {
  return 360 / (OPENING_DEGREES_PER_PX * WORLD_WIDTH * coverScale(viewport));
}

/** The zoom multiplier at which `screenScale` reaches `tileDetailMaxScale()` for this viewport — the raised ceiling once the camera has tiles to show. The zoom controls disable here. */
export function tileDetailMaxZoom(viewport: Viewport): number {
  return tileDetailMaxScale() / coverScale(viewport);
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
    // Centred on the points the window covers, not anchored at the first of them: a lone point (or a tight group) then sits mid-window instead of on its edge.
    const center = (sorted[left] + sorted[right - 1]) / 2;
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
  // No area, no scale: the world's centre at the floor zoom (see `clampCamera`).
  if (viewport.width <= 0 || viewport.height <= 0) return { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: MIN_ZOOM };
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

/**
 * The smallest geographic span a work's fit shows, and the margin it pads the
 * places' bounding box by — mirrored from the build's locator poster so the
 * live map and the static poster it replaces open on the same framing
 * (`geometry.rs`: `MIN_FRAME_DEGREES`, `FRAME_PADDING`). The minimum applies to longitude only: the poster's latitude share assumes a 3:2 canvas, and a wide live frame would otherwise stay height-bound and show far more than the poster.
 */
const WORK_MIN_SPAN_DEGREES = 10;
const WORK_BOX_PADDING = 1.25;
/** Breathing room per edge, as a share of the frame's smaller side. */
const WORK_EDGE_PAD_FRACTION = 0.06;

/**
 * Fit one work's own places — the initial article scope, a marker click and
 * a card click all share this one function. The bounding box is padded like
 * the poster's frame and widened to a minimum span so a single place frames
 * regionally instead of at the ceiling.
 *
 * `frame` (the card row/controls-free rectangle) is honoured only when it is
 * at least half the viewport in each dimension; a smaller one would shrink
 * the fit to a sliver, so the whole viewport is used. Callers that must
 * ignore overlays entirely (the collapsed embed) pass no frame.
 *
 * Clamped only by `maxZoom` (callers pass the tile ceiling: tiles exist
 * around every place), never raised to the cover floor beyond what
 * `clampCamera` itself requires.
 */
export function fitWork(
  points: Point[],
  viewport: Viewport,
  frame: Rect = wholeViewport(viewport),
  maxZoom: number = tileDetailMaxZoom(viewport),
): Camera {
  // A viewport with no area (an embed mounted while hidden) has no scale to
  // fit against: 0/0 would reach `clamp` as NaN and poison x/y. The caller
  // re-fits once the frame gets a real size.
  if (points.length === 0 || viewport.width <= 0 || viewport.height <= 0) return coverCamera(points, viewport, frame);
  const usable =
    frame.width >= viewport.width / 2 && frame.height >= viewport.height / 2 ? frame : wholeViewport(viewport);
  const xs = points.map((point) => point.x);
  const ys = points.map((point) => point.y);
  const minSpanX = (WORK_MIN_SPAN_DEGREES * WORLD_WIDTH) / 360;
  const spanX = Math.max((Math.max(...xs) - Math.min(...xs)) * WORK_BOX_PADDING, minSpanX);
  const spanY = (Math.max(...ys) - Math.min(...ys)) * WORK_BOX_PADDING;
  const pad = Math.min(usable.width, usable.height) * WORK_EDGE_PAD_FRACTION;
  const scale = Math.min((usable.width - pad * 2) / spanX, (usable.height - pad * 2) / spanY);
  const zoom = clamp(Number.isFinite(scale) ? scale / coverScale(viewport) : MIN_ZOOM, MIN_ZOOM, maxZoom);
  const centered = centerOnFrame(
    { x: (Math.min(...xs) + Math.max(...xs)) / 2, y: (Math.min(...ys) + Math.max(...ys)) / 2 },
    viewport,
    usable,
    zoom * coverScale(viewport),
  );
  return clampCamera({ x: centered.x, y: centered.y, zoom }, viewport, maxZoom);
}
