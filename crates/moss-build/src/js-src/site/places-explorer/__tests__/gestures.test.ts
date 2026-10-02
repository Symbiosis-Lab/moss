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
 */
import { describe, test, expect, vi } from "vitest";
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
