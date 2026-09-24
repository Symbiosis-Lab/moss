/**
 * Tests for the trackpad swipe-navigation state machine.
 *
 * The detector turns a stream of wheel events (as seen inside the preview
 * iframe) into at most one "back"/"forward" emission per physical gesture.
 * WebKit exposes no wheel phase to JS, so gestures are segmented by quiet
 * gaps between events; momentum events after the fingers lift arrive within
 * the gap and must be swallowed.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  createSwipeGestureDetector,
  canScrollHorizontallyFrom,
  installSwipeNavigation,
  SWIPE_THRESHOLD,
  SWIPE_GESTURE_GAP_MS,
  type SwipeDirection,
} from "../swipe-gesture";

describe("swipe-gesture detector", () => {
  let fired: SwipeDirection[];
  let canScroll: ReturnType<typeof vi.fn>;
  let detector: ReturnType<typeof createSwipeGestureDetector>;
  const target = {} as EventTarget;

  beforeEach(() => {
    fired = [];
    canScroll = vi.fn(() => false);
    detector = createSwipeGestureDetector({
      canScrollHorizontally: canScroll,
      onSwipe: (d) => fired.push(d),
    });
  });

  /** Feed a burst of events with the given per-event deltaX/deltaY, 16ms apart. */
  function burst(
    count: number,
    deltaX: number,
    deltaY = 0,
    startTime = 1000
  ): number {
    let t = startTime;
    for (let i = 0; i < count; i++) {
      detector.handleWheel({ deltaX, deltaY, timeStamp: t, target });
      t += 16;
    }
    return t;
  }

  it("fires 'back' when fingers swipe right (negative deltaX past threshold)", () => {
    burst(10, -(SWIPE_THRESHOLD / 5));
    expect(fired).toEqual(["back"]);
  });

  it("fires 'forward' when fingers swipe left (positive deltaX past threshold)", () => {
    burst(10, SWIPE_THRESHOLD / 5);
    expect(fired).toEqual(["forward"]);
  });

  it("does not fire below the threshold", () => {
    burst(3, -(SWIPE_THRESHOLD / 5));
    expect(fired).toEqual([]);
  });

  it("fires at most once per gesture, swallowing momentum events", () => {
    const t = burst(10, -(SWIPE_THRESHOLD / 5));
    // Momentum tail: same direction, still within the gap.
    burst(30, -(SWIPE_THRESHOLD / 5), 0, t);
    expect(fired).toEqual(["back"]);
  });

  it("fires again for a new gesture after a quiet gap", () => {
    const t = burst(10, -(SWIPE_THRESHOLD / 5));
    burst(10, -(SWIPE_THRESHOLD / 5), 0, t + SWIPE_GESTURE_GAP_MS + 1);
    expect(fired).toEqual(["back", "back"]);
  });

  it("ignores vertical scrolling entirely", () => {
    burst(30, 0, 40);
    expect(fired).toEqual([]);
  });

  it("does not fire when a vertical scroll drifts horizontal mid-gesture", () => {
    // Gesture starts vertical-dominant → whole gesture is a scroll.
    const t = burst(5, 0, 40);
    burst(20, -(SWIPE_THRESHOLD / 5), 0, t);
    expect(fired).toEqual([]);
  });

  it("cancels an armed swipe when a vertical-dominant event interrupts it", () => {
    const t = burst(2, -(SWIPE_THRESHOLD / 5)); // below threshold so far
    detector.handleWheel({ deltaX: 0, deltaY: 40, timeStamp: t, target });
    burst(20, -(SWIPE_THRESHOLD / 5), 0, t + 16);
    expect(fired).toEqual([]);
  });

  it("treats an event with equal deltas as vertical (scroll wins ties)", () => {
    burst(20, 30, 30);
    expect(fired).toEqual([]);
  });

  it("cedes the gesture to a horizontally scrollable container", () => {
    canScroll.mockReturnValue(true);
    burst(20, -(SWIPE_THRESHOLD / 5));
    expect(fired).toEqual([]);
  });

  it("stays ceded even when the container hits its edge mid-gesture", () => {
    // First event: container can still scroll → gesture is consumed.
    canScroll.mockReturnValueOnce(true).mockReturnValue(false);
    burst(20, -(SWIPE_THRESHOLD / 5));
    expect(fired).toEqual([]);
  });

  it("probes scrollability once per gesture, with target and direction", () => {
    burst(10, -(SWIPE_THRESHOLD / 5));
    expect(canScroll).toHaveBeenCalledTimes(1);
    expect(canScroll).toHaveBeenCalledWith(target, "back");
  });

  it("probes with 'forward' for a fingers-left swipe", () => {
    burst(10, SWIPE_THRESHOLD / 5);
    expect(canScroll).toHaveBeenCalledWith(target, "forward");
  });

  it("does not fire when left/right wiggling never accumulates past threshold", () => {
    let t = 1000;
    for (let i = 0; i < 20; i++) {
      const dx = (i % 2 === 0 ? 1 : -1) * (SWIPE_THRESHOLD / 5);
      detector.handleWheel({ deltaX: dx, deltaY: 0, timeStamp: t, target });
      t += 16;
    }
    expect(fired).toEqual([]);
  });
});

/**
 * Tests for the DOM scrollability probe the bridge injects into the
 * detector. jsdom does no layout, so scroll geometry is stubbed per element.
 */
function stubScrollGeometry(
  el: HTMLElement,
  { scrollWidth, clientWidth, scrollLeft }: { scrollWidth: number; clientWidth: number; scrollLeft: number }
): void {
  Object.defineProperty(el, "scrollWidth", { value: scrollWidth, configurable: true });
  Object.defineProperty(el, "clientWidth", { value: clientWidth, configurable: true });
  Object.defineProperty(el, "scrollLeft", { value: scrollLeft, configurable: true, writable: true });
}

describe("canScrollHorizontallyFrom", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("returns false when no ancestor is horizontally scrollable", () => {
    document.body.innerHTML = `<div><p id="t">text</p></div>`;
    const t = document.getElementById("t")!;
    expect(canScrollHorizontallyFrom(t, "back")).toBe(false);
    expect(canScrollHorizontallyFrom(t, "forward")).toBe(false);
  });

  it("returns false for a null target", () => {
    expect(canScrollHorizontallyFrom(null, "back")).toBe(false);
  });

  it("detects a scrolled overflow-x container with room to scroll back", () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: auto"><p id="t">wide</p></div>`;
    const c = document.getElementById("c")!;
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 100 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "back")).toBe(true);
  });

  it("returns false when the container is at its left edge (back swipe)", () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: scroll"><p id="t">wide</p></div>`;
    const c = document.getElementById("c")!;
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 0 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "back")).toBe(false);
  });

  it("detects room to scroll forward, and the right edge", () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: auto"><p id="t">wide</p></div>`;
    const c = document.getElementById("c")!;
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 0 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "forward")).toBe(true);
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 300 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "forward")).toBe(false);
  });

  it("ignores overflow-x visible/hidden containers", () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: hidden"><p id="t">wide</p></div>`;
    const c = document.getElementById("c")!;
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 100 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "back")).toBe(false);
  });

  it("walks past non-scrollable wrappers to a scrollable grandparent", () => {
    document.body.innerHTML = `
      <div id="c" style="overflow-x: auto"><div><span id="t">deep</span></div></div>`;
    const c = document.getElementById("c")!;
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 50 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "back")).toBe(true);
  });

  it("treats a horizontally overflowing documentElement as scrollable regardless of overflow-x", () => {
    document.body.innerHTML = `<p id="t">text</p>`;
    stubScrollGeometry(document.documentElement, { scrollWidth: 900, clientWidth: 400, scrollLeft: 10 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "back")).toBe(true);
    // cleanup: remove stubs from the shared documentElement
    delete (document.documentElement as any).scrollWidth;
    delete (document.documentElement as any).clientWidth;
    delete (document.documentElement as any).scrollLeft;
  });
});

/**
 * Bundle regression: the shipped bridge bundle must contain the swipe
 * wiring, so a rebuild that drops it can't land silently. Same pattern as
 * iframe-bridge-scroll-sync.test.ts.
 */
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const BUNDLE_PATH = resolve(
  __dirname,
  "../../../ops/serve/js/iframe-bridge.js"
);
const bundleExists = existsSync(BUNDLE_PATH);
if (!bundleExists) {
  // eslint-disable-next-line no-console
  console.warn(
    `[swipe-gesture] Skipping bundle regression test: ${BUNDLE_PATH} not found. Run \`node scripts/build-backend-scripts.mjs\` to generate it.`
  );
}

describe("iframe-bridge bundle contains swipe navigation", () => {
  it.skipIf(!bundleExists)("posts moss-swipe-nav", () => {
    const bundle = readFileSync(BUNDLE_PATH, "utf-8");
    expect(bundle).toContain("moss-swipe-nav");
  });
});

/**
 * Tests for installSwipeNavigation — the bridge entry point that wires the
 * detector to real wheel events and posts moss-swipe-nav to the parent.
 * jsdom's window.parent === window, so the parent postMessage is spied
 * directly on window.
 */
describe("installSwipeNavigation", () => {
  let posted: Array<{ type: string; direction: string }>;

  beforeEach(() => {
    posted = [];
    vi.spyOn(window.parent, "postMessage").mockImplementation((msg: unknown) => {
      posted.push(msg as { type: string; direction: string });
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("posts one moss-swipe-nav per strong horizontal wheel burst", () => {
    const uninstall = installSwipeNavigation(window);
    for (let i = 0; i < 10; i++) {
      window.dispatchEvent(
        new WheelEvent("wheel", { deltaX: -SWIPE_THRESHOLD / 5, deltaY: 0 })
      );
    }
    expect(posted.filter((m) => m.type === "moss-swipe-nav")).toEqual([
      { type: "moss-swipe-nav", direction: "back" },
    ]);
    uninstall();
  });

  it("posts nothing for vertical scrolling", () => {
    const uninstall = installSwipeNavigation(window);
    for (let i = 0; i < 10; i++) {
      window.dispatchEvent(new WheelEvent("wheel", { deltaX: 0, deltaY: 40 }));
    }
    expect(posted).toEqual([]);
    uninstall();
  });

  it("stops listening after uninstall", () => {
    const uninstall = installSwipeNavigation(window);
    uninstall();
    for (let i = 0; i < 10; i++) {
      window.dispatchEvent(
        new WheelEvent("wheel", { deltaX: -SWIPE_THRESHOLD / 5, deltaY: 0 })
      );
    }
    expect(posted).toEqual([]);
  });
});

/**
 * Review-driven hardening cases: zero-delta phase events, site-cancelled
 * wheels, direction reversal, install-time momentum swallowing, RTL and
 * overflow-hidden scrollability, fractional scroll positions.
 */
describe("swipe-gesture detector hardening", () => {
  let fired: SwipeDirection[];
  let canScroll: ReturnType<typeof vi.fn>;
  const target = {} as EventTarget;

  function makeDetector(initialTimeStamp?: number) {
    fired = [];
    canScroll = vi.fn(() => false);
    return createSwipeGestureDetector({
      canScrollHorizontally: canScroll,
      onSwipe: (d) => fired.push(d),
      initialTimeStamp,
    });
  }

  it("ignores zero-delta events (WebKit gesture-phase markers) without consuming the gesture", () => {
    const detector = makeDetector();
    // Phase-boundary events carry no scroll intent.
    detector.handleWheel({ deltaX: 0, deltaY: 0, timeStamp: 1000, target });
    let t = 1016;
    for (let i = 0; i < 10; i++) {
      detector.handleWheel({ deltaX: -(SWIPE_THRESHOLD / 5), deltaY: 0, timeStamp: t, target });
      t += 16;
    }
    expect(fired).toEqual(["back"]);
  });

  it("cedes the gesture when the site cancelled the wheel event (defaultPrevented)", () => {
    const detector = makeDetector();
    let t = 1000;
    for (let i = 0; i < 20; i++) {
      detector.handleWheel({
        deltaX: -(SWIPE_THRESHOLD / 5),
        deltaY: 0,
        timeStamp: t,
        target,
        defaultPrevented: true,
      });
      t += 16;
    }
    expect(fired).toEqual([]);
  });

  it("cedes when a gesture reverses direction instead of firing the opposite way", () => {
    const detector = makeDetector();
    // First event classifies the gesture as 'forward' (probe runs for forward)…
    detector.handleWheel({ deltaX: 2, deltaY: 0, timeStamp: 1000, target });
    // …then the user reverses hard. Firing 'back' here would navigate while
    // any forward-edge container is actively scrolling; the gesture must cede.
    let t = 1016;
    for (let i = 0; i < 20; i++) {
      detector.handleWheel({ deltaX: -(SWIPE_THRESHOLD / 2), deltaY: 0, timeStamp: t, target });
      t += 16;
    }
    expect(fired).toEqual([]);
    expect(canScroll).toHaveBeenCalledWith(target, "forward");
  });

  it("swallows momentum arriving right after install (initialTimeStamp) but fires after a quiet gap", () => {
    // The bridge installs a fresh detector in each document; a swipe that
    // just navigated here still has momentum flowing. Those events must not
    // chain another navigation.
    const detector = makeDetector(1000);
    let t = 1050;
    for (let i = 0; i < 30; i++) {
      detector.handleWheel({ deltaX: -(SWIPE_THRESHOLD / 5), deltaY: 0, timeStamp: t, target });
      t += 16;
    }
    expect(fired).toEqual([]);

    // A deliberate swipe after the momentum dies down works.
    t += SWIPE_GESTURE_GAP_MS + 1;
    for (let i = 0; i < 10; i++) {
      detector.handleWheel({ deltaX: -(SWIPE_THRESHOLD / 5), deltaY: 0, timeStamp: t, target });
      t += 16;
    }
    expect(fired).toEqual(["back"]);
  });
});

describe("canScrollHorizontallyFrom hardening", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("handles RTL containers (negative scrollLeft range)", () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: auto; direction: rtl"><p id="t">wide</p></div>`;
    const c = document.getElementById("c")!;
    const t = document.getElementById("t")!;
    // WebKit RTL: scrollLeft ranges from -(scrollWidth-clientWidth) to 0.
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: -200 });
    expect(canScrollHorizontallyFrom(t, "back")).toBe(true);
    expect(canScrollHorizontallyFrom(t, "forward")).toBe(true);
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: -300 });
    expect(canScrollHorizontallyFrom(t, "back")).toBe(false);
    stubScrollGeometry(c, { scrollWidth: 500, clientWidth: 200, scrollLeft: 0 });
    expect(canScrollHorizontallyFrom(t, "forward")).toBe(false);
  });

  it("treats an overflowing documentElement with overflow-x hidden as NOT scrollable", () => {
    document.body.innerHTML = `<p id="t">text</p>`;
    document.documentElement.style.overflowX = "hidden";
    stubScrollGeometry(document.documentElement, { scrollWidth: 900, clientWidth: 400, scrollLeft: 10 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "forward")).toBe(false);
    document.documentElement.style.overflowX = "";
    delete (document.documentElement as any).scrollWidth;
    delete (document.documentElement as any).clientWidth;
    delete (document.documentElement as any).scrollLeft;
  });

  it("ignores fractional scroll residue at the left edge (back swipe)", () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: auto"><p id="t">wide</p></div>`;
    stubScrollGeometry(document.getElementById("c")!, { scrollWidth: 500, clientWidth: 200, scrollLeft: 0.5 });
    expect(canScrollHorizontallyFrom(document.getElementById("t")!, "back")).toBe(false);
  });
});

describe("installSwipeNavigation hardening", () => {
  let posted: Array<{ type: string; direction: string }>;

  beforeEach(() => {
    posted = [];
    vi.spyOn(window.parent, "postMessage").mockImplementation((msg: unknown) => {
      posted.push(msg as { type: string; direction: string });
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** Real event timeStamps sit within the install-time momentum window;
   *  wait it out so bursts count as a fresh gesture. */
  function afterMomentumWindow(): Promise<void> {
    return new Promise((r) => setTimeout(r, SWIPE_GESTURE_GAP_MS + 20));
  }

  it("normalizes line-mode wheel deltas (deltaMode=1) like scrollbar.ts", async () => {
    const uninstall = installSwipeNavigation(window);
    await afterMomentumWindow();
    // A tilt-wheel emits ~2 lines per notch; 16px/line makes 4 notches cross
    // the 80px threshold. Raw accumulation (-8) would never fire.
    for (let i = 0; i < 4; i++) {
      window.dispatchEvent(
        new WheelEvent("wheel", { deltaX: -2, deltaY: 0, deltaMode: 1 })
      );
    }
    expect(posted.filter((m) => m.type === "moss-swipe-nav")).toEqual([
      { type: "moss-swipe-nav", direction: "back" },
    ]);
    uninstall();
  });

  it("does not fire when the site cancelled the wheel events", async () => {
    const cancel = (e: Event) => e.preventDefault();
    window.addEventListener("wheel", cancel, { passive: false, capture: true });
    const uninstall = installSwipeNavigation(window);
    await afterMomentumWindow();
    for (let i = 0; i < 10; i++) {
      window.dispatchEvent(
        new WheelEvent("wheel", {
          deltaX: -SWIPE_THRESHOLD / 5,
          deltaY: 0,
          cancelable: true,
        })
      );
    }
    // No navigation — the per-event wheel reports still flow (marked ceded).
    expect(posted.filter((m) => m.type === "moss-swipe-nav")).toEqual([]);
    expect(
      posted.filter((m) => m.type === "moss-swipe-wheel" && m.ceded !== true)
    ).toEqual([]);
    uninstall();
    window.removeEventListener("wheel", cancel, { capture: true });
  });
});

/**
 * v2: the bridge posts a STATELESS moss-swipe-wheel report for every
 * horizontal-dominant wheel event (direction + whether content claims it),
 * so the shell-side native-phase tracker can corroborate that the gesture
 * happened over the preview and may navigate. Deliberately not
 * gesture-segmented — segmentation here raced the native stream and
 * starved forward-after-back gestures. The v1 threshold-fired
 * moss-swipe-nav stays as the non-macOS fallback.
 */
describe("installSwipeNavigation wheel reports", () => {
  let posted: Array<Record<string, unknown>>;

  beforeEach(() => {
    posted = [];
    vi.spyOn(window.parent, "postMessage").mockImplementation((msg: unknown) => {
      posted.push(msg as Record<string, unknown>);
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function afterMomentumWindow(): Promise<void> {
    return new Promise((r) => setTimeout(r, SWIPE_GESTURE_GAP_MS + 20));
  }

  it("posts a moss-swipe-wheel report per horizontal event, regardless of thresholds", async () => {
    const uninstall = installSwipeNavigation(window);
    await afterMomentumWindow();
    // Small horizontal events — far below the nav threshold.
    window.dispatchEvent(new WheelEvent("wheel", { deltaX: -8, deltaY: 0 }));
    window.dispatchEvent(new WheelEvent("wheel", { deltaX: -8, deltaY: 0 }));
    const reports = posted.filter((m) => m.type === "moss-swipe-wheel");
    expect(reports).toEqual([
      { type: "moss-swipe-wheel", direction: "back", ceded: false },
      { type: "moss-swipe-wheel", direction: "back", ceded: false },
    ]);
    // Vertical events post nothing.
    window.dispatchEvent(new WheelEvent("wheel", { deltaX: 0, deltaY: 40 }));
    expect(posted.filter((m) => m.type === "moss-swipe-wheel").length).toBe(2);
    uninstall();
  });

  it("reports ceded=true when a container can consume the scroll", async () => {
    document.body.innerHTML = `<div id="c" style="overflow-x: auto"><p id="t">wide</p></div>`;
    const c = document.getElementById("c")!;
    Object.defineProperty(c, "scrollWidth", { value: 500, configurable: true });
    Object.defineProperty(c, "clientWidth", { value: 200, configurable: true });
    Object.defineProperty(c, "scrollLeft", { value: 100, configurable: true });
    const uninstall = installSwipeNavigation(window);
    await afterMomentumWindow();
    const target = document.getElementById("t")!;
    target.dispatchEvent(
      new WheelEvent("wheel", { deltaX: -8, deltaY: 0, bubbles: true })
    );
    const reports = posted.filter((m) => m.type === "moss-swipe-wheel");
    expect(reports).toEqual([
      { type: "moss-swipe-wheel", direction: "back", ceded: true },
    ]);
    document.body.innerHTML = "";
    uninstall();
  });
});
