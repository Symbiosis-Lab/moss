/**
 * gestures.ts — turns raw pointer/wheel/keyboard events on the viewport
 * into camera changes, via the callbacks the caller (`map.ts`) supplies.
 * Owns no camera state of its own: every gesture reads the CURRENT camera
 * through `getCamera()` and reports a new one through `setCamera()`, so
 * `map.ts` stays the one place that clamps, applies and persists it.
 */
import { clampCamera, screenScale, screenToWorld } from "./camera";
import type { Camera, Point, Viewport } from "./types";

export interface GestureCallbacks {
  getCamera(): Camera;
  getViewport(): Viewport;
  /** A camera change mid-gesture — may be called many times per second. */
  setCamera(camera: Camera): void;
  /** The gesture that just produced `setCamera` calls has ended (settle: write the URL, restore static paint). */
  onSettle(): void;
  /** A gesture has just started moving the camera (promote the world layer to its own compositing layer). */
  onGestureStart(): void;
  /** The current detail ceiling (zoom units) — the world ceiling, or the raised tile ceiling once the camera has tiles in view. */
  maxZoom(): number;
  /**
   * True while this map should run cooperative gestures, for a small
   * embed sharing its page with the reader's own scroll: a single-finger
   * TOUCH drag is left alone (the page scrolls natively) instead of
   * panning the map, and two fingers pan the camera in addition to
   * pinch-zooming it. Mouse/pen dragging, and wheel zoom, are
   * unaffected either way — only a lone touch defers to the page.
   * Queried on every pointer event rather than fixed at attach time, so
   * the embed's own expand/collapse transition needs no re-attachment —
   * only the full explorer page omits this (there is no page scroll to
   * share with), the same as every call site below treats an absent
   * callback as "never cooperative".
   */
  cooperative?(): boolean;
  /**
   * A tap or click on the bare map surface (not a marker, cluster, card,
   * breadcrumb chip or control, and not the end of a drag or pinch).
   * map.ts closes an open card with it; the camera is left where it is.
   */
  onDismiss?(): void;
}

/** What a press is NOT on the bare map when it lands on: anything with a job of its own. */
const OWN_JOB = "button, a, .moss-card, .moss-places-chip, .moss-places-controls";
/** Movement under this many CSS px is a tap, not a drag. */
const TAP_SLOP = 5;

const KEYBOARD_STEP = 32;
const KEYBOARD_STEP_FAST = 70;
const WHEEL_ZOOM_PER_PIXEL = Math.log(1.08) / 100;
const WHEEL_SETTLE_MS = 120;

/** The ceiling a PAN (drag, keyboard arrows) clamps against — never below the camera's own current zoom, so dragging off the tile patch that raised `callbacks.maxZoom()` can never snap the zoom back down mid-drag. A zoom-changing gesture (`zoomAt`) deliberately does NOT go through this: `callbacks.maxZoom()` alone is the ceiling that limits zooming IN. */
function panMaxZoom(camera: Camera, callbacks: GestureCallbacks): number {
  return Math.max(callbacks.maxZoom(), camera.zoom);
}

/** Zoom by `factor`, keeping the world point under `(clientX, clientY)` — viewport centre when omitted — fixed on screen: the inverse of `worldToScreen`, solved for the new camera centre at the target zoom, then handed to `clampCamera` for the final word on both the zoom ceiling/floor and the pan bounds. */
function zoomAt(viewport: HTMLElement, callbacks: GestureCallbacks, factor: number, clientX?: number, clientY?: number): void {
  const camera = callbacks.getCamera();
  const viewportSize = callbacks.getViewport();
  const rect = viewport.getBoundingClientRect();
  const anchorScreen: Point = {
    x: (clientX ?? rect.left + rect.width / 2) - rect.left,
    y: (clientY ?? rect.top + rect.height / 2) - rect.top,
  };
  const anchorWorld = screenToWorld(anchorScreen, camera, viewportSize);
  const targetZoom = camera.zoom * factor;
  const scaleAtTarget = screenScale({ x: camera.x, y: camera.y, zoom: targetZoom }, viewportSize);
  const candidate: Camera = {
    x: anchorWorld.x - (anchorScreen.x - viewportSize.width / 2) / scaleAtTarget,
    y: anchorWorld.y - (anchorScreen.y - viewportSize.height / 2) / scaleAtTarget,
    zoom: targetZoom,
  };
  callbacks.setCamera(clampCamera(candidate, viewportSize, callbacks.maxZoom()));
}

/** Attaches every pan/zoom gesture this module owns to `viewport`. Listeners live as long as the element does — the same boot-and-forget wiring every other site runtime uses. */
export function attachGestures(viewport: HTMLElement, callbacks: GestureCallbacks): void {
  const pointers = new Map<number, { x: number; y: number; type: string }>();
  let drag: { id: number; startX: number; startY: number; cameraX: number; cameraY: number } | null = null;
  let pinch: { distance: number; midpoint: Point } | null = null;
  /** The one press that may still end as a tap: set on a lone press on the bare map, voided by movement past the slop or a second pointer. */
  let tap: { id: number; x: number; y: number } | null = null;

  const isCooperative = (): boolean => callbacks.cooperative?.() ?? false;
  /** A lone touch, in cooperative mode, is the one pointer left for the page's own scroll rather than claimed as a map drag. */
  const isDeferredTouch = (pointerType: string): boolean => pointerType === "touch" && isCooperative();

  viewport.addEventListener("pointerdown", (event) => {
    if (event.target instanceof Element && event.target.closest(OWN_JOB)) return;
    if (event.pointerType === "mouse" && event.button !== 0) return;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY, type: event.pointerType });
    tap = pointers.size === 1 ? { id: event.pointerId, x: event.clientX, y: event.clientY } : null;
    if (pointers.size === 1) {
      if (isDeferredTouch(event.pointerType)) return;
      viewport.setPointerCapture(event.pointerId);
      const camera = callbacks.getCamera();
      drag = { id: event.pointerId, startX: event.clientX, startY: event.clientY, cameraX: camera.x, cameraY: camera.y };
      viewport.setAttribute("data-dragging", "");
    } else if (pointers.size === 2) {
      viewport.setPointerCapture(event.pointerId);
      drag = null;
      viewport.removeAttribute("data-dragging");
      const [a, b] = [...pointers.values()];
      pinch = { distance: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)), midpoint: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } };
    }
  });

  viewport.addEventListener("pointermove", (event) => {
    if (!pointers.has(event.pointerId)) return;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY, type: event.pointerType });
    if (tap && Math.hypot(event.clientX - tap.x, event.clientY - tap.y) > TAP_SLOP) tap = null;
    if (pointers.size >= 2 && pinch) {
      const [a, b] = [...pointers.values()];
      const distance = Math.max(1, Math.hypot(a.x - b.x, a.y - b.y));
      const midpoint: Point = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      callbacks.onGestureStart();
      if (isCooperative()) {
        // Two fingers pan AND pinch together: translate by the midpoint's
        // own movement (same math as the single-pointer drag below,
        // anchored at the pinch midpoint instead of one finger) before the
        // existing distance-based zoom runs, so neither motion is lost to
        // the other inside one frame. Only the cooperative path tracks a
        // moving midpoint; the plain pinch below still zooms anchored at
        // its ORIGINAL midpoint, unchanged.
        event.preventDefault();
        const camera = callbacks.getCamera();
        const viewportSize = callbacks.getViewport();
        const scale = screenScale(camera, viewportSize);
        const dx = (midpoint.x - pinch.midpoint.x) / scale;
        const dy = (midpoint.y - pinch.midpoint.y) / scale;
        if (dx !== 0 || dy !== 0) {
          callbacks.setCamera(clampCamera({ x: camera.x - dx, y: camera.y - dy, zoom: camera.zoom }, viewportSize, panMaxZoom(camera, callbacks)));
        }
        zoomAt(viewport, callbacks, distance / pinch.distance, midpoint.x, midpoint.y);
        pinch.distance = distance;
        pinch.midpoint = midpoint;
        return;
      }
      zoomAt(viewport, callbacks, distance / pinch.distance, pinch.midpoint.x, pinch.midpoint.y);
      pinch.distance = distance;
      return;
    }
    if (!drag || drag.id !== event.pointerId) return;
    const camera = callbacks.getCamera();
    const viewportSize = callbacks.getViewport();
    const scale = screenScale(camera, viewportSize);
    const dx = (event.clientX - drag.startX) / scale;
    const dy = (event.clientY - drag.startY) / scale;
    callbacks.onGestureStart();
    callbacks.setCamera(clampCamera({ x: drag.cameraX - dx, y: drag.cameraY - dy, zoom: camera.zoom }, viewportSize, panMaxZoom(camera, callbacks)));
  });

  function endPointer(event: PointerEvent): void {
    // A pointer `pointerdown` ignored (a `button`/`a` target, above) was
    // never added to `pointers` — ending it here must do nothing, not
    // settle a gesture that never started. Settling calls `onSettle()`,
    // which re-renders the marker/card layers; doing that between a real
    // `pointerdown` and `pointerup` on, say, a cluster marker button
    // replaced the button out from under the browser's own click
    // synthesis, so no `click` event ever fired — a tap that visibly
    // pressed a marker and did nothing. `pointerup`/`pointercancel`/
    // `lostpointercapture` all reach this same guard.
    if (!pointers.has(event.pointerId)) return;
    const isTap = event.type === "pointerup" && tap?.id === event.pointerId && pointers.size === 1;
    if (tap?.id === event.pointerId) tap = null;
    pointers.delete(event.pointerId);
    if (drag?.id === event.pointerId) drag = null;
    if (pointers.size < 2) pinch = null;
    if (pointers.size === 1) {
      // One finger lifted out of a pinch: resume panning with the other,
      // anchored at its current screen position rather than restarting the
      // whole gesture from here — unless that remaining finger is itself a
      // cooperative-mode touch, which defers to the page's own scroll the
      // same as a lone touch always does in that mode.
      const [[id, point]] = pointers;
      if (isDeferredTouch(point.type)) {
        viewport.removeAttribute("data-dragging");
      } else {
        const camera = callbacks.getCamera();
        drag = { id, startX: point.x, startY: point.y, cameraX: camera.x, cameraY: camera.y };
        viewport.setAttribute("data-dragging", "");
      }
    } else {
      viewport.removeAttribute("data-dragging");
    }
    if (pointers.size === 0) callbacks.onSettle();
    if (isTap) callbacks.onDismiss?.();
  }
  viewport.addEventListener("pointerup", endPointer);
  viewport.addEventListener("pointercancel", endPointer);
  viewport.addEventListener("lostpointercapture", endPointer);

  // Wheel deltas arrive in pixels, lines, or pages. Use distance rather than
  // event count so a trackpad's many small events do not race to the zoom cap.
  let wheelSettleTimer: ReturnType<typeof setTimeout> | undefined;
  viewport.addEventListener(
    "wheel",
    (event) => {
      if (!event.deltaY) return;
      event.preventDefault();
      callbacks.onGestureStart();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? callbacks.getViewport().height : 1;
      const delta = Math.max(-400, Math.min(400, event.deltaY * unit));
      zoomAt(viewport, callbacks, Math.exp(-delta * WHEEL_ZOOM_PER_PIXEL), event.clientX, event.clientY);
      clearTimeout(wheelSettleTimer);
      // Cards, labels and URL persistence belong to the end of a wheel burst,
      // just as they belong to pointerup rather than every drag movement.
      wheelSettleTimer = setTimeout(() => {
        wheelSettleTimer = undefined;
        if (!pointers.size) callbacks.onSettle();
      }, WHEEL_SETTLE_MS);
    },
    { passive: false },
  );

  viewport.addEventListener("keydown", (event) => {
    if (event.target instanceof Element && event.target.closest("button, a")) return;
    if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      callbacks.onGestureStart();
      zoomAt(viewport, callbacks, 1.2);
      callbacks.onSettle();
      return;
    }
    if (event.key === "-" || event.key === "_") {
      event.preventDefault();
      callbacks.onGestureStart();
      zoomAt(viewport, callbacks, 1 / 1.2);
      callbacks.onSettle();
      return;
    }
    const step = event.shiftKey ? KEYBOARD_STEP_FAST : KEYBOARD_STEP;
    let dx = 0;
    let dy = 0;
    if (event.key === "ArrowLeft") dx = -step;
    else if (event.key === "ArrowRight") dx = step;
    else if (event.key === "ArrowUp") dy = -step;
    else if (event.key === "ArrowDown") dy = step;
    else return;
    event.preventDefault();
    const camera = callbacks.getCamera();
    const viewportSize = callbacks.getViewport();
    const scale = screenScale(camera, viewportSize);
    callbacks.onGestureStart();
    callbacks.setCamera(clampCamera({ x: camera.x + dx / scale, y: camera.y + dy / scale, zoom: camera.zoom }, viewportSize, panMaxZoom(camera, callbacks)));
    callbacks.onSettle();
  });
}
