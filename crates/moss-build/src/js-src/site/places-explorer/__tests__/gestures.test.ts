/**
 * Tests for `attachGestures`'s own pointer-tracking: specifically, that a
 * pointer whose `pointerdown` was ignored (a `button`/`a` target — the
 * gesture layer's own exclusion, so a marker or card button can handle its
 * own click) ends NOTHING on `pointerup`. Before this fix, `endPointer` ran
 * unconditionally and called `onSettle()` for that untracked pointer too —
 * `onSettle` re-renders the marker/card layers, which replaced the very
 * button the browser's click synthesis was still resolving against,
 * so a real tap on a marker never produced a `click` event at all. Covered
 * at the unit level (not just the render gates) because this is the exact
 * mechanism, isolated from marker/card DOM entirely.
 *
 * Also covers `attachGestures`'s cooperative mode: the small collapsed embed
 * shares its page with the reader's own scroll, so a lone TOUCH drag must
 * be left alone (the page scrolls) while two fingers pan the camera as well
 * as pinch-zoom it. Mouse dragging and ctrl/⌘+wheel zoom are unaffected —
 * only a solitary touch defers, and only when `cooperative()` says so.
 *
 * jsdom has neither a `PointerEvent` constructor nor
 * `Element.prototype.setPointerCapture`; both are polyfilled here the same
 * way the rest of this suite works around jsdom gaps (immersive-mode's own
 * `createTransitionEndEvent`, for a missing `TransitionEvent`) — real
 * pointer/capture semantics are exercised by the render gates instead.
 */
import { afterEach, beforeAll, beforeEach, describe, test, expect, vi } from "vitest";
import { attachGestures, type GestureCallbacks } from "../gestures";
import type { Camera, Viewport } from "../types";

function makeCallbacks(): GestureCallbacks & { settleCount: number } {
  const camera: Camera = { x: 0, y: 0, zoom: 1 };
  const viewport: Viewport = { width: 800, height: 500 };
  const callbacks = {
    settleCount: 0,
    getCamera: () => camera,
    getViewport: () => viewport,
    setCamera: vi.fn(),
    onSettle: vi.fn(function (this: { settleCount: number }) {
      this.settleCount++;
    }),
    onGestureStart: vi.fn(),
    maxZoom: () => 10,
  };
  callbacks.onSettle = vi.fn(() => {
    callbacks.settleCount++;
  });
  return callbacks;
}

/**
 * jsdom (this project's vitest environment) has no `PointerEvent`
 * constructor — build on `MouseEvent`, which it does support, and attach
 * the pointer-specific fields `gestures.ts` reads (`pointerId`,
 * `pointerType`) directly: `addEventListener("pointerdown", ...)` matches
 * on the event's `type` string, not its constructor, so a plain `MouseEvent`
 * dispatched as type `"pointerdown"` reaches the same listener a real
 * `PointerEvent` would.
 */
function fire(target: EventTarget, type: string, init: { pointerId: number; clientX: number; clientY: number; button?: number }): void {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: init.clientX, clientY: init.clientY, button: init.button ?? 0 });
  Object.defineProperties(event, {
    pointerId: { value: init.pointerId },
    pointerType: { value: "mouse" },
  });
  target.dispatchEvent(event);
}

describe("attachGestures — endPointer ignores an untracked pointer", () => {
  test("pointerup on a button child, whose pointerdown the gesture layer ignored, never settles", () => {
    const viewportEl = document.createElement("div");
    const button = document.createElement("button");
    viewportEl.append(button);
    document.body.append(viewportEl);
    // jsdom has no real layout; setPointerCapture is a no-op there but must
    // exist on the element for attachGestures's own call to it not to throw
    // on a REAL tracked pointer (the second test below).
    (viewportEl as unknown as { setPointerCapture: (id: number) => void }).setPointerCapture ??= () => {};

    const callbacks = makeCallbacks();
    attachGestures(viewportEl, callbacks);

    fire(button, "pointerdown", { pointerId: 1, clientX: 10, clientY: 10, button: 0 });
    fire(button, "pointerup", { pointerId: 1, clientX: 10, clientY: 10, button: 0 });

    expect(callbacks.onSettle).not.toHaveBeenCalled();
    expect(viewportEl.hasAttribute("data-dragging")).toBe(false);

    document.body.removeChild(viewportEl);
  });

  test("a real drag directly on the viewport still settles on pointerup", () => {
    const viewportEl = document.createElement("div");
    document.body.append(viewportEl);
    (viewportEl as unknown as { setPointerCapture: (id: number) => void }).setPointerCapture ??= () => {};

    const callbacks = makeCallbacks();
    attachGestures(viewportEl, callbacks);

    fire(viewportEl, "pointerdown", { pointerId: 2, clientX: 10, clientY: 10, button: 0 });
    fire(viewportEl, "pointermove", { pointerId: 2, clientX: 20, clientY: 10, button: 0 });
    fire(viewportEl, "pointerup", { pointerId: 2, clientX: 20, clientY: 10, button: 0 });

    expect(callbacks.onSettle).toHaveBeenCalledTimes(1);

    document.body.removeChild(viewportEl);
  });

  test("an untracked pointerup between two real pointers does not end the still-active one", () => {
    // Two real pointers down (a pinch); the FIRST one's pointerdown is
    // tracked normally. A stray pointerup for a THIRD, never-down pointer id
    // (e.g. a duplicate/late event some browsers are known to send) must not
    // touch the still-active pinch state.
    const viewportEl = document.createElement("div");
    document.body.append(viewportEl);
    (viewportEl as unknown as { setPointerCapture: (id: number) => void }).setPointerCapture ??= () => {};

    const callbacks = makeCallbacks();
    attachGestures(viewportEl, callbacks);

    fire(viewportEl, "pointerdown", { pointerId: 3, clientX: 0, clientY: 0, button: 0 });
    fire(viewportEl, "pointerdown", { pointerId: 4, clientX: 50, clientY: 0, button: 0 });
    fire(viewportEl, "pointerup", { pointerId: 99, clientX: 0, clientY: 0, button: 0 });

    // Neither real pointer has ended, so no settle yet.
    expect(callbacks.onSettle).not.toHaveBeenCalled();

    document.body.removeChild(viewportEl);
  });
});

beforeAll(() => {
  if (!("setPointerCapture" in Element.prototype)) {
    (Element.prototype as any).setPointerCapture = () => {};
    (Element.prototype as any).releasePointerCapture = () => {};
    (Element.prototype as any).hasPointerCapture = () => false;
  }
});

interface FakePointerInit {
  pointerId: number;
  pointerType: "mouse" | "touch" | "pen";
  clientX: number;
  clientY: number;
  button?: number;
}

function firePointer(target: EventTarget, type: string, init: FakePointerInit): void {
  const event = new Event(type, { bubbles: true, cancelable: true }) as any;
  Object.assign(event, { button: 0, ...init });
  target.dispatchEvent(event);
}

const VIEWPORT: Viewport = { width: 800, height: 500 };

function buildCallbacks(cooperative: boolean | (() => boolean)) {
  let camera: Camera = { x: 400, y: 250, zoom: 1 };
  const setCamera = vi.fn((next: Camera) => {
    camera = next;
  });
  const onSettle = vi.fn();
  const onGestureStart = vi.fn();
  const cooperativeHint = vi.fn();
  const callbacks: GestureCallbacks = {
    getCamera: () => camera,
    getViewport: () => VIEWPORT,
    setCamera,
    onSettle,
    onGestureStart,
    maxZoom: () => 20,
    cooperative: typeof cooperative === "function" ? cooperative : () => cooperative,
    cooperativeHint,
  };
  return { callbacks, setCamera, onSettle, onGestureStart, cooperativeHint, getCamera: () => camera };
}

describe("attachGestures — cooperative mode", () => {
  let viewport: HTMLElement;

  beforeEach(() => {
    viewport = document.createElement("div");
    document.body.append(viewport);
    // getBoundingClientRect backs screenToWorld/screenScale math in gestures.ts's own helpers.
    vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
      width: VIEWPORT.width, height: VIEWPORT.height, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, x: 0, y: 0, toJSON() {},
    } as DOMRect);
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  test("a lone touch drag pans the camera when NOT cooperative (unchanged default)", () => {
    const { callbacks, setCamera } = buildCallbacks(false);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 100 });
    firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "touch", clientX: 150, clientY: 100 });
    expect(setCamera).toHaveBeenCalled();
    expect(viewport.hasAttribute("data-dragging")).toBe(true);
  });

  test("a lone touch drag is left for the page's own scroll when cooperative", () => {
    const { callbacks, setCamera, onGestureStart } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 100 });
    firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "touch", clientX: 150, clientY: 100 });
    expect(setCamera).not.toHaveBeenCalled();
    expect(onGestureStart).not.toHaveBeenCalled();
    expect(viewport.hasAttribute("data-dragging")).toBe(false);
  });

  test("a mouse drag still pans the camera when cooperative (only touch defers)", () => {
    const { callbacks, setCamera } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "mouse", clientX: 100, clientY: 100 });
    firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "mouse", clientX: 150, clientY: 100 });
    expect(setCamera).toHaveBeenCalled();
  });

  test("two fingers pan the camera when cooperative, not just pinch-zoom it", () => {
    const { callbacks, setCamera, getCamera } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 250 });
    firePointer(viewport, "pointerdown", { pointerId: 2, pointerType: "touch", clientX: 300, clientY: 250 });
    setCamera.mockClear();
    // Both fingers move right together, in small steps (as real touchmove
    // frames would arrive) so the inter-finger distance — and so the zoom —
    // stays put while the midpoint, and so the pan, visibly moves. Each
    // finger's own pointermove fires separately, so a single large jump
    // would read the OTHER finger's now-stale position mid-step and read as
    // a transient pinch; small repeated steps settle that out, the same way
    // consecutive real touch frames would.
    for (let x1 = 100, x2 = 300; x1 < 160; x1 += 1, x2 += 1) {
      firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "touch", clientX: x1 + 1, clientY: 250 });
      firePointer(viewport, "pointermove", { pointerId: 2, pointerType: "touch", clientX: x2 + 1, clientY: 250 });
    }
    expect(setCamera).toHaveBeenCalled();
    const finalCamera = getCamera();
    expect(finalCamera.zoom).toBeCloseTo(1, 1);
    expect(finalCamera.x).not.toBeCloseTo(400, 1);
  });

  test("two fingers still pinch-zoom when NOT cooperative (regression guard)", () => {
    const { callbacks, setCamera } = buildCallbacks(false);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 300, clientY: 250 });
    firePointer(viewport, "pointerdown", { pointerId: 2, pointerType: "touch", clientX: 500, clientY: 250 });
    setCamera.mockClear();
    firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "touch", clientX: 250, clientY: 250 });
    firePointer(viewport, "pointermove", { pointerId: 2, pointerType: "touch", clientX: 550, clientY: 250 });
    expect(setCamera).toHaveBeenCalled();
    const lastCall = setCamera.mock.calls.at(-1)![0] as Camera;
    expect(lastCall.zoom).toBeGreaterThan(1);
  });

  test("lifting one finger out of a cooperative two-touch gesture does not resume a drag-pan", () => {
    const { callbacks, setCamera } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 250 });
    firePointer(viewport, "pointerdown", { pointerId: 2, pointerType: "touch", clientX: 300, clientY: 250 });
    firePointer(viewport, "pointerup", { pointerId: 2, pointerType: "touch", clientX: 300, clientY: 250 });
    setCamera.mockClear();
    firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "touch", clientX: 160, clientY: 250 });
    expect(setCamera).not.toHaveBeenCalled();
    expect(viewport.hasAttribute("data-dragging")).toBe(false);
  });

  test("ctrl+wheel zoom is unaffected by cooperative mode", () => {
    const { callbacks, setCamera } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    viewport.dispatchEvent(new WheelEvent("wheel", { ctrlKey: true, deltaY: -1, clientX: 400, clientY: 250, bubbles: true, cancelable: true }));
    expect(setCamera).toHaveBeenCalled();
  });

  test("a bare wheel over a cooperative viewport reports the hint instead of zooming", () => {
    const { callbacks, setCamera, cooperativeHint } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    viewport.dispatchEvent(new WheelEvent("wheel", { deltaY: -1, clientX: 400, clientY: 250, bubbles: true, cancelable: true }));
    expect(cooperativeHint).toHaveBeenCalledTimes(1);
    expect(setCamera).not.toHaveBeenCalled();
  });

  test("a bare wheel when NOT cooperative never reports the hint (the full explorer page has no hint to show)", () => {
    const { callbacks, cooperativeHint } = buildCallbacks(false);
    attachGestures(viewport, callbacks);
    viewport.dispatchEvent(new WheelEvent("wheel", { deltaY: -1, clientX: 400, clientY: 250, bubbles: true, cancelable: true }));
    expect(cooperativeHint).not.toHaveBeenCalled();
  });

  test("a ctrl+wheel never reports the hint, cooperative or not — it already zooms", () => {
    const { callbacks, cooperativeHint } = buildCallbacks(true);
    attachGestures(viewport, callbacks);
    viewport.dispatchEvent(new WheelEvent("wheel", { ctrlKey: true, deltaY: -1, clientX: 400, clientY: 250, bubbles: true, cancelable: true }));
    expect(cooperativeHint).not.toHaveBeenCalled();
  });

  test("cooperative() is read live, so toggling mid-session needs no re-attachment", () => {
    let coop = false;
    const { callbacks, setCamera } = buildCallbacks(() => coop);
    attachGestures(viewport, callbacks);
    firePointer(viewport, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 100, clientY: 100 });
    firePointer(viewport, "pointermove", { pointerId: 1, pointerType: "touch", clientX: 150, clientY: 100 });
    expect(setCamera).toHaveBeenCalled(); // not cooperative yet: a plain touch drag pans
    firePointer(viewport, "pointerup", { pointerId: 1, pointerType: "touch", clientX: 150, clientY: 100 });

    coop = true;
    setCamera.mockClear();
    firePointer(viewport, "pointerdown", { pointerId: 2, pointerType: "touch", clientX: 100, clientY: 100 });
    firePointer(viewport, "pointermove", { pointerId: 2, pointerType: "touch", clientX: 150, clientY: 100 });
    expect(setCamera).not.toHaveBeenCalled(); // now cooperative: the lone touch defers
  });
});

describe("attachGestures — dismiss", () => {
  function setup(): { viewport: HTMLElement; callbacks: GestureCallbacks & { onDismiss: ReturnType<typeof vi.fn> } } {
    const viewport = document.createElement("div");
    document.body.append(viewport);
    (viewport as unknown as { setPointerCapture: (id: number) => void }).setPointerCapture ??= () => {};
    const callbacks = { ...makeCallbacks(), onDismiss: vi.fn() };
    attachGestures(viewport, callbacks);
    return { viewport, callbacks };
  }
  afterEach(() => {
    document.body.replaceChildren();
  });

  test("a press and release in place on the bare map dismisses", () => {
    const { viewport, callbacks } = setup();
    fire(viewport, "pointerdown", { pointerId: 1, clientX: 10, clientY: 10 });
    fire(viewport, "pointerup", { pointerId: 1, clientX: 12, clientY: 11 });
    expect(callbacks.onDismiss).toHaveBeenCalledTimes(1);
  });

  test("the end of a drag does not", () => {
    const { viewport, callbacks } = setup();
    fire(viewport, "pointerdown", { pointerId: 1, clientX: 10, clientY: 10 });
    fire(viewport, "pointermove", { pointerId: 1, clientX: 60, clientY: 10 });
    fire(viewport, "pointerup", { pointerId: 1, clientX: 60, clientY: 10 });
    expect(callbacks.onDismiss).not.toHaveBeenCalled();
  });

  test("a press on a card or a control does not", () => {
    const { viewport, callbacks } = setup();
    const card = document.createElement("article");
    card.className = "moss-card";
    const text = document.createElement("p");
    card.append(text);
    viewport.append(card);
    fire(text, "pointerdown", { pointerId: 1, clientX: 10, clientY: 10 });
    fire(text, "pointerup", { pointerId: 1, clientX: 10, clientY: 10 });
    expect(callbacks.onDismiss).not.toHaveBeenCalled();
  });
});
