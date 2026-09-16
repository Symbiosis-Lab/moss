/**
 * Trackpad swipe-navigation state machine for the preview iframe.
 *
 * Turns a stream of wheel events into at most one "back"/"forward" emission
 * per physical gesture. Pure logic — no DOM access — so it can be unit-tested
 * directly; the bridge wires it to real wheel events and postMessage.
 *
 * WebKit exposes no wheel phase to JS and keeps delivering momentum events
 * after the fingers lift, so a "gesture" is defined as a run of events with
 * gaps under SWIPE_GESTURE_GAP_MS. Each gesture is classified once, by its
 * first meaningful event:
 *   - zero-delta events (WebKit gesture-phase markers) are ignored entirely
 *   - vertical-dominant first event → the gesture is a scroll; never fires
 *   - horizontal-dominant but a container can consume the scroll → ceded to
 *     the container for the gesture's whole life (matching Safari: reaching
 *     the container's edge mid-gesture does not start navigation)
 *   - otherwise → accumulate deltaX and fire once past SWIPE_THRESHOLD
 * A vertical-dominant or site-cancelled (defaultPrevented) event interrupting
 * an armed gesture cancels it, as does reversing direction — firing opposite
 * to the probed direction could navigate while a container is scrolling.
 *
 * `initialTimeStamp` starts the machine as if a consumed gesture just ended
 * at that moment: each document gets a fresh detector, and the momentum tail
 * of the swipe that navigated here must not chain another navigation. (The
 * shell adds a post-navigation cooldown for momentum that arrives later than
 * the gap window — see navigation-manager.ts.)
 *
 * Known limitation (no phase API): a swipe attempted while a previous
 * vertical scroll's momentum is still trickling merges into that consumed
 * gesture and is swallowed — conservative by design (misses a swipe rather
 * than mis-navigating); retrying after a beat works.
 *
 * Direction follows the macOS convention under natural scrolling: fingers
 * moving right produce negative deltaX and mean "back".
 *
 * Design: docs/archive/2026-07-10-preview-swipe-navigation-design.md
 */

export type SwipeDirection = "back" | "forward";

/** Accumulated |deltaX| (pixels) at which a gesture triggers navigation. */
export const SWIPE_THRESHOLD = 80;

/** Quiet gap (ms) between wheel events that separates two gestures. */
export const SWIPE_GESTURE_GAP_MS = 300;

/** px per line for DOM_DELTA_LINE normalization (mirrors scrollbar.ts). */
const LINE_HEIGHT_PX = 16;

export interface SwipeWheelInput {
  deltaX: number;
  deltaY: number;
  timeStamp: number;
  target: EventTarget | null;
  /** True when a site handler cancelled the event — the site owns it. */
  defaultPrevented?: boolean;
}

export interface SwipeGestureOptions {
  /**
   * Whether some element from `target` upward can still scroll horizontally
   * in the direction this swipe would scroll it. Probed once, on the
   * gesture's first horizontal event.
   */
  canScrollHorizontally: (
    target: EventTarget | null,
    direction: SwipeDirection
  ) => boolean;
  onSwipe: (direction: SwipeDirection) => void;
  /**
   * Start the machine as if a consumed gesture ended at this timeStamp —
   * swallows the momentum tail of a swipe that navigated to this document.
   */
  initialTimeStamp?: number;
}

/**
 * DOM implementation of the scrollability probe: can some element from
 * `target` upward still scroll horizontally in the direction this swipe
 * would scroll it? (deltaX < 0 → "back" → scrollLeft decreasing;
 * deltaX > 0 → "forward" → scrollLeft increasing.)
 *
 * The documentElement scrolls the viewport even with the default
 * `overflow-x: visible`, so the root only needs overflow-x ≠ hidden/clip.
 * RTL containers range scrollLeft over [-(scrollWidth-clientWidth), 0] in
 * WebKit; both bounds carry a 1px tolerance for fractional scroll positions
 * (preview zoom / Retina scaling report values like 0.49999).
 */
export function canScrollHorizontallyFrom(
  target: EventTarget | null,
  direction: SwipeDirection
): boolean {
  let el: Element | null =
    target instanceof Element ? target : ((target as Node | null)?.parentElement ?? null);

  for (; el; el = el.parentElement) {
    if (el.scrollWidth <= el.clientWidth) continue;
    const style = getComputedStyle(el);
    const overflowX = style.overflowX;
    if (el === el.ownerDocument.documentElement) {
      if (overflowX === "hidden" || overflowX === "clip") continue;
    } else if (overflowX !== "auto" && overflowX !== "scroll") {
      continue;
    }
    const maxScroll = el.scrollWidth - el.clientWidth;
    const minLeft = style.direction === "rtl" ? -maxScroll : 0;
    const maxLeft = style.direction === "rtl" ? 0 : maxScroll;
    const canScroll =
      direction === "back"
        ? el.scrollLeft > minLeft + 1
        : el.scrollLeft < maxLeft - 1;
    if (canScroll) return true;
  }
  return false;
}

/**
 * Bridge entry point: wire the detector to real wheel events on `win` and
 * post `{ type: "moss-swipe-nav", direction }` to its parent. The shell
 * routes the message through NavigationManager.goBack()/goForward() —
 * exactly as if the titlebar buttons were clicked. Bubble phase + passive:
 * we never preventDefault, and a site handler that cancels the event is
 * honored via defaultPrevented (Safari's cede-on-cancel semantics).
 * Returns an uninstall function.
 */
export function installSwipeNavigation(win: Window): () => void {
  const detector = createSwipeGestureDetector({
    canScrollHorizontally: canScrollHorizontallyFrom,
    onSwipe: (direction) => {
      win.parent.postMessage({ type: "moss-swipe-nav", direction }, "*");
    },
    initialTimeStamp: win.performance?.now?.() ?? 0,
  });
  const onWheel = (event: WheelEvent) => {
    const scale =
      event.deltaMode === 1 ? LINE_HEIGHT_PX : event.deltaMode === 2 ? win.innerHeight : 1;
    const deltaX = event.deltaX * scale;
    const deltaY = event.deltaY * scale;

    // Stateless per-event report for the shell's native-phase tracker:
    // proves the wheel happened over the preview and says whether content
    // claims it. Deliberately NOT gesture-segmented — the bridge has no
    // phase info, and any segmentation here races the native stream (the
    // once-per-gesture candidate starved forward-after-back gestures when
    // momentum chaining kept the detector consumed).
    if (Math.abs(deltaX) > Math.abs(deltaY)) {
      const direction: SwipeDirection = deltaX < 0 ? "back" : "forward";
      const ceded =
        event.defaultPrevented || canScrollHorizontallyFrom(event.target, direction);
      win.parent.postMessage({ type: "moss-swipe-wheel", direction, ceded }, "*");
    }

    detector.handleWheel({
      deltaX,
      deltaY,
      timeStamp: event.timeStamp,
      target: event.target,
      defaultPrevented: event.defaultPrevented,
    });
  };
  win.addEventListener("wheel", onWheel, { passive: true });
  return () => win.removeEventListener("wheel", onWheel);
}

export function createSwipeGestureDetector(options: SwipeGestureOptions): {
  handleWheel(event: SwipeWheelInput): void;
} {
  let lastEventTime = options.initialTimeStamp ?? -Infinity;
  let accumulatedX = 0;
  /** Once true, nothing fires until a quiet gap starts a new gesture. */
  let consumed = options.initialTimeStamp !== undefined;
  /** Direction the scrollability probe ran for (null = not yet classified). */
  let probedDirection: SwipeDirection | null = null;

  function handleWheel(event: SwipeWheelInput): void {
    // WebKit dispatches zero-delta wheel events at gesture phase boundaries;
    // they carry no scroll intent and must not classify or extend a gesture.
    if (event.deltaX === 0 && event.deltaY === 0) return;

    const isNewGesture = event.timeStamp - lastEventTime > SWIPE_GESTURE_GAP_MS;
    lastEventTime = event.timeStamp;

    if (isNewGesture) {
      accumulatedX = 0;
      consumed = false;
      probedDirection = null;
    }
    if (consumed) return;

    // The site cancelled this event — it owns the interaction. Cede.
    if (event.defaultPrevented) {
      consumed = true;
      return;
    }

    const horizontal = Math.abs(event.deltaX) > Math.abs(event.deltaY);
    if (!horizontal) {
      // A scroll — either the gesture is one from the start, or it
      // interrupts an armed swipe. Both end this gesture's chance to fire.
      consumed = true;
      return;
    }

    if (probedDirection === null) {
      probedDirection = event.deltaX < 0 ? "back" : "forward";
      if (options.canScrollHorizontally(event.target, probedDirection)) {
        consumed = true;
        return;
      }
    }

    accumulatedX += event.deltaX;
    if (Math.abs(accumulatedX) >= SWIPE_THRESHOLD) {
      consumed = true;
      const direction: SwipeDirection = accumulatedX < 0 ? "back" : "forward";
      // A gesture that reversed past the threshold fires opposite to the
      // direction it was probed for — any container at the probed edge may
      // now be scrolling. Cede instead of navigating.
      if (direction !== probedDirection) return;
      options.onSwipe(direction);
    }
  }

  return { handleWheel };
}
