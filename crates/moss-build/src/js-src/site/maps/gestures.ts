/**
 * Pointer, wheel and keyboard gestures for a map's viewport, in screen terms.
 * Owns no camera: each map says what a pan or a zoom does to its own view
 * through the callbacks, so one set of gestures serves every map.
 */

export interface Point { x: number; y: number }

export interface GestureCallbacks {
  /** A pan is about to move the view from where it is now; remember that view. */
  panStart(): void;
  /** Put the view where it would be had the content followed the pointer by (dx, dy) screen px since `panStart`. */
  panTo(dx: number, dy: number): void;
  /** Zoom by `factor` (> 1 zooms in), keeping the point `at` (px from the viewport's top-left) fixed on screen. */
  zoomBy(factor: number, at: Point): void;
  /** The 0 key. A map without a reset leaves the key alone. */
  reset?(): void;
  /** The gesture that just moved the view has ended (settle: write the URL, restore static paint). */
  onSettle?(): void;
  /** A gesture is about to move the view. */
  onGestureStart(): void;
  /**
   * True while this map shares its page with the reader's own scroll (a small
   * embed): a lone touch is left to the page, two fingers pan as well as
   * pinch, and a wheel zooms only with ctrl or ⌘. Read on every event, so a
   * map can switch mode without re-attaching. Absent means never cooperative.
   */
  cooperative?(): boolean;
  /** A tap or click on the bare surface, not the end of a drag or pinch. */
  onDismiss?(): void;
}

export interface GestureOptions {
  /** Presses and keys that land inside this selector belong to what it matches, not to the map. */
  ownJob: string;
  /** A plain wheel over this selector scrolls it instead of zooming. */
  wheelPassThrough?: string;
  /** The factor of one + or − key press. */
  keyZoomStep: number;
  /** True when only ctrl or ⌘ + wheel zooms and a plain wheel scrolls the page. Read on every wheel event. */
  wheelNeedsModifier?(): boolean;
}

/** Movement under this many CSS px is a tap, not a drag. */
const TAP_SLOP = 5;
const KEYBOARD_STEP = 32;
const KEYBOARD_STEP_FAST = 70;
const WHEEL_ZOOM_PER_PIXEL = Math.log(1.08) / 100;
const WHEEL_SETTLE_MS = 120;
/** How long after a drag ends a click is still the drag's own. */
const CLICK_AFTER_DRAG_MS = 50;

/** Attaches every pan/zoom gesture to `viewport`. Listeners live as long as the element does. */
export function attachGestures(viewport: HTMLElement, callbacks: GestureCallbacks, options: GestureOptions): void {
  const pointers = new Map<number, { x: number; y: number; type: string }>();
  let drag: { id: number; startX: number; startY: number; captured: boolean } | null = null;
  let pinch: { distance: number; midpoint: Point } | null = null;
  /** The one press that may still end as a tap: set on a lone press, voided by movement past the slop or a second pointer. */
  let tap: { id: number; x: number; y: number } | null = null;
  /** The gesture in progress has moved the view, so the click that follows its release is not a choice. */
  let moved = false;

  const isCooperative = (): boolean => callbacks.cooperative?.() ?? false;
  /** A lone touch, in cooperative mode, is left for the page's own scroll rather than claimed as a map drag. */
  const isDeferredTouch = (pointerType: string): boolean => pointerType === "touch" && isCooperative();
  const centre = (): Point => ({ x: viewport.clientWidth / 2, y: viewport.clientHeight / 2 });
  const local = (clientX: number, clientY: number): Point => {
    const rect = viewport.getBoundingClientRect();
    return { x: clientX - rect.left, y: clientY - rect.top };
  };
  const pan = (dx: number, dy: number): void => {
    callbacks.panStart();
    callbacks.panTo(dx, dy);
  };
  const beginDrag = (id: number, x: number, y: number, captured: boolean): void => {
    callbacks.panStart();
    drag = { id, startX: x, startY: y, captured };
    viewport.setAttribute("data-dragging", "");
  };

  viewport.addEventListener("pointerdown", (event) => {
    if (event.target instanceof Element && event.target.closest(options.ownJob)) return;
    if (event.pointerType === "mouse" && event.button !== 0) return;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY, type: event.pointerType });
    tap = pointers.size === 1 ? { id: event.pointerId, x: event.clientX, y: event.clientY } : null;
    if (pointers.size === 1) {
      moved = false;
      if (isDeferredTouch(event.pointerType)) return;
      beginDrag(event.pointerId, event.clientX, event.clientY, false);
    } else if (pointers.size === 2) {
      viewport.setPointerCapture(event.pointerId);
      drag = null;
      moved = true;
      viewport.removeAttribute("data-dragging");
      const [a, b] = [...pointers.values()];
      pinch = { distance: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)), midpoint: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } };
    }
  });

  viewport.addEventListener("pointermove", (event) => {
    if (!pointers.has(event.pointerId)) return;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY, type: event.pointerType });
    if (tap && Math.hypot(event.clientX - tap.x, event.clientY - tap.y) > TAP_SLOP) {
      tap = null;
      moved = true;
      // Capture waits for a real drag: capturing at the press would retarget the click that ends a tap away from what was pressed.
      if (drag && !drag.captured) {
        viewport.setPointerCapture(drag.id);
        drag.captured = true;
      }
    }
    if (pointers.size >= 2 && pinch) {
      const [a, b] = [...pointers.values()];
      const distance = Math.max(1, Math.hypot(a.x - b.x, a.y - b.y));
      const midpoint: Point = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      callbacks.onGestureStart();
      if (isCooperative()) {
        // Two fingers pan and pinch together, so neither motion is lost to the other within a frame.
        event.preventDefault();
        pan(midpoint.x - pinch.midpoint.x, midpoint.y - pinch.midpoint.y);
        callbacks.zoomBy(distance / pinch.distance, local(midpoint.x, midpoint.y));
        pinch.distance = distance;
        pinch.midpoint = midpoint;
        return;
      }
      callbacks.zoomBy(distance / pinch.distance, local(pinch.midpoint.x, pinch.midpoint.y));
      pinch.distance = distance;
      return;
    }
    if (!drag || drag.id !== event.pointerId) return;
    callbacks.onGestureStart();
    callbacks.panTo(event.clientX - drag.startX, event.clientY - drag.startY);
  });

  function endPointer(event: PointerEvent): void {
    // A pointer whose press was ignored never entered `pointers`; ending it must not settle a gesture that never began (settling re-renders, which would replace a button mid-click).
    if (!pointers.has(event.pointerId)) return;
    const isTap = event.type === "pointerup" && tap?.id === event.pointerId && pointers.size === 1;
    if (tap?.id === event.pointerId) tap = null;
    pointers.delete(event.pointerId);
    if (drag?.id === event.pointerId) drag = null;
    if (pointers.size < 2) pinch = null;
    if (pointers.size === 1) {
      // One finger lifted out of a pinch resumes panning with the other, unless that one is a lone touch the page owns.
      const [[id, point]] = pointers;
      if (isDeferredTouch(point.type)) viewport.removeAttribute("data-dragging");
      else beginDrag(id, point.x, point.y, true);
    } else {
      viewport.removeAttribute("data-dragging");
    }
    if (pointers.size === 0) {
      callbacks.onSettle?.();
      if (moved) {
        // Some browsers still click after a touch drag shorter than their own slop; the drag is not a choice.
        const swallow = (click: Event): void => click.stopPropagation();
        viewport.addEventListener("click", swallow, { capture: true });
        setTimeout(() => viewport.removeEventListener("click", swallow, { capture: true }), CLICK_AFTER_DRAG_MS);
      }
    }
    if (isTap) callbacks.onDismiss?.();
  }
  viewport.addEventListener("pointerup", endPointer);
  viewport.addEventListener("pointercancel", endPointer);
  viewport.addEventListener("lostpointercapture", endPointer);

  // Wheel deltas arrive in pixels, lines, or pages. Use distance rather than event count so a trackpad's many small events do not race to the zoom cap.
  let wheelSettleTimer: ReturnType<typeof setTimeout> | undefined;
  viewport.addEventListener(
    "wheel",
    (event) => {
      const modified = event.ctrlKey || event.metaKey;
      if (!event.deltaY || (options.wheelNeedsModifier?.() && !modified)) return;
      if (!modified && options.wheelPassThrough && event.target instanceof Element && event.target.closest(options.wheelPassThrough)) return;
      event.preventDefault();
      callbacks.onGestureStart();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? viewport.clientHeight : 1;
      const delta = Math.max(-400, Math.min(400, event.deltaY * unit));
      callbacks.zoomBy(Math.exp(-delta * WHEEL_ZOOM_PER_PIXEL), local(event.clientX, event.clientY));
      clearTimeout(wheelSettleTimer);
      // Cards, labels and URL persistence belong to the end of a wheel burst, just as they belong to pointerup rather than every drag movement.
      wheelSettleTimer = setTimeout(() => {
        wheelSettleTimer = undefined;
        if (!pointers.size) callbacks.onSettle?.();
      }, WHEEL_SETTLE_MS);
    },
    { passive: false },
  );

  viewport.addEventListener("keydown", (event) => {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.target instanceof Element && event.target.closest(`button, a, ${options.ownJob}`)) return;
    const zoom = event.key === "+" || event.key === "=" ? options.keyZoomStep : event.key === "-" || event.key === "_" ? 1 / options.keyZoomStep : 0;
    if (zoom) {
      event.preventDefault();
      callbacks.onGestureStart();
      callbacks.zoomBy(zoom, centre());
      callbacks.onSettle?.();
      return;
    }
    if (event.key === "0" && callbacks.reset) {
      event.preventDefault();
      callbacks.reset();
      return;
    }
    const step = event.shiftKey ? KEYBOARD_STEP_FAST : KEYBOARD_STEP;
    const arrows: Record<string, [number, number]> = { ArrowLeft: [step, 0], ArrowRight: [-step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] };
    const by = arrows[event.key];
    if (!by) return;
    event.preventDefault();
    callbacks.onGestureStart();
    pan(by[0], by[1]);
    callbacks.onSettle?.();
  });
}
