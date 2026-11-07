/**
 * The gestures every map shares, against a stand-in view: pointer tracking
 * (an ignored press ends nothing, a tap versus a drag, a drag is no click),
 * cooperative mode for embeds, and the wheel and keyboard rules.
 *
 * jsdom has no `PointerEvent` and no pointer capture; events are plain
 * `Event`s of the right type and capture is stubbed. Real pointer semantics
 * are exercised by the render gates.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import { attachGestures, type GestureCallbacks, type GestureOptions, type Point } from "../gestures";

beforeAll(() => {
  Object.assign(Element.prototype, { setPointerCapture() {}, releasePointerCapture() {}, hasPointerCapture: () => false });
});

const SIZE = { width: 800, height: 500 };
const OPTIONS: GestureOptions = { ownJob: "button, a, .own", keyZoomStep: 1.5 };

/** A view that is a point and a zoom: panning moves the point against the pointer, zooming scales it. */
function harness(extra: Partial<GestureCallbacks> = {}, options: Partial<GestureOptions> = {}) {
  const view = { x: 0, y: 0, zoom: 1 };
  let from = { ...view };
  const zooms: Array<{ factor: number; at: Point }> = [];
  const callbacks = {
    panStart: vi.fn(() => {
      from = { ...view };
    }),
    panTo: vi.fn((dx: number, dy: number) => {
      view.x = from.x - dx;
      view.y = from.y - dy;
    }),
    zoomBy: vi.fn((factor: number, at: Point) => {
      zooms.push({ factor, at });
      view.zoom *= factor;
    }),
    reset: vi.fn(),
    onSettle: vi.fn(),
    onGestureStart: vi.fn(),
  };
  const el = document.createElement("div");
  document.body.append(el);
  vi.spyOn(el, "getBoundingClientRect").mockReturnValue({ ...SIZE, top: 0, left: 0, right: SIZE.width, bottom: SIZE.height, x: 0, y: 0, toJSON() {} } as DOMRect);
  Object.defineProperties(el, { clientWidth: { value: SIZE.width }, clientHeight: { value: SIZE.height } });
  attachGestures(el, { ...callbacks, ...extra }, { ...OPTIONS, ...options });
  return { el, callbacks, view, zooms };
}

type Init = { pointerId?: number; pointerType?: string; clientX: number; clientY: number };
function pointer(target: EventTarget, type: string, init: Init): void {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.assign(event, { button: 0, pointerId: 1, pointerType: "mouse", ...init });
  target.dispatchEvent(event);
}
const wheel = (el: Element, init: WheelEventInit) => {
  const event = new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: 400, clientY: 250, ...init });
  el.dispatchEvent(event);
  return event;
};
const key = (el: Element, k: string, init: KeyboardEventInit = {}) => {
  const event = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
  el.dispatchEvent(event);
  return event;
};

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("pointer tracking", () => {
  test("a press on a button, which the gesture layer ignores, ends nothing on release", () => {
    const { el, callbacks } = harness();
    const button = el.appendChild(document.createElement("button"));
    pointer(button, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(button, "pointerup", { clientX: 10, clientY: 10 });
    expect(callbacks.onSettle).not.toHaveBeenCalled();
    expect(el.hasAttribute("data-dragging")).toBe(false);
  });

  test("a drag pans by the pointer's whole travel since the press, then settles once", () => {
    const { el, callbacks, view } = harness();
    pointer(el, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(el, "pointermove", { clientX: 30, clientY: 14 });
    pointer(el, "pointermove", { clientX: 60, clientY: 10 });
    expect(view).toMatchObject({ x: -50, y: 0 });
    pointer(el, "pointerup", { clientX: 60, clientY: 10 });
    expect(callbacks.onSettle).toHaveBeenCalledTimes(1);
  });

  test("an untracked pointerup between two real pointers does not end the pinch", () => {
    const { el, callbacks } = harness();
    pointer(el, "pointerdown", { pointerId: 3, clientX: 0, clientY: 0 });
    pointer(el, "pointerdown", { pointerId: 4, clientX: 50, clientY: 0 });
    pointer(el, "pointerup", { pointerId: 99, clientX: 0, clientY: 0 });
    expect(callbacks.onSettle).not.toHaveBeenCalled();
  });

  test("a pinch zooms by the change in finger distance, at the midpoint it began at", () => {
    const { el, zooms } = harness();
    pointer(el, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 300, clientY: 250 });
    pointer(el, "pointerdown", { pointerId: 2, pointerType: "touch", clientX: 500, clientY: 250 });
    pointer(el, "pointermove", { pointerId: 2, pointerType: "touch", clientX: 600, clientY: 250 });
    expect(zooms.at(-1)).toEqual({ factor: 1.5, at: { x: 400, y: 250 } });
  });

  test("a press and release in place dismisses; the end of a drag does not; a press on an own-job element does not", () => {
    const onDismiss = vi.fn();
    const { el } = harness({ onDismiss });
    pointer(el, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(el, "pointerup", { clientX: 12, clientY: 11 });
    expect(onDismiss).toHaveBeenCalledTimes(1);
    pointer(el, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(el, "pointermove", { clientX: 60, clientY: 10 });
    pointer(el, "pointerup", { clientX: 60, clientY: 10 });
    const own = el.appendChild(document.createElement("p"));
    own.className = "own";
    pointer(own, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(own, "pointerup", { clientX: 10, clientY: 10 });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  test("a click right after a drag is swallowed; a click after a tap is not", () => {
    vi.useFakeTimers();
    const { el } = harness();
    const seen = vi.fn();
    el.addEventListener("click", seen);
    pointer(el, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(el, "pointermove", { clientX: 60, clientY: 10 });
    pointer(el, "pointerup", { clientX: 60, clientY: 10 });
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(seen).not.toHaveBeenCalled();
    vi.advanceTimersByTime(60);
    pointer(el, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(el, "pointerup", { clientX: 10, clientY: 10 });
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(seen).toHaveBeenCalledTimes(1);
  });

  test("the pointer is captured only once the press has become a drag", () => {
    const { el } = harness();
    const capture = vi.spyOn(el, "setPointerCapture");
    pointer(el, "pointerdown", { clientX: 10, clientY: 10 });
    pointer(el, "pointermove", { clientX: 12, clientY: 10 });
    expect(capture).not.toHaveBeenCalled();
    pointer(el, "pointermove", { clientX: 40, clientY: 10 });
    expect(capture).toHaveBeenCalledWith(1);
  });
});

describe("cooperative mode", () => {
  const touch = (id: number, x: number, y = 250): Init => ({ pointerId: id, pointerType: "touch", clientX: x, clientY: y });

  test("a lone touch drag pans when not cooperative, and is left for the page when cooperative", () => {
    const plain = harness({ cooperative: () => false });
    pointer(plain.el, "pointerdown", touch(1, 100));
    pointer(plain.el, "pointermove", touch(1, 150));
    expect(plain.view.x).toBe(-50);
    const coop = harness({ cooperative: () => true });
    pointer(coop.el, "pointerdown", touch(1, 100));
    pointer(coop.el, "pointermove", touch(1, 150));
    expect(coop.callbacks.panTo).not.toHaveBeenCalled();
    expect(coop.callbacks.onGestureStart).not.toHaveBeenCalled();
    expect(coop.el.hasAttribute("data-dragging")).toBe(false);
  });

  test("a mouse drag still pans when cooperative", () => {
    const { el, view } = harness({ cooperative: () => true });
    pointer(el, "pointerdown", { clientX: 100, clientY: 100 });
    pointer(el, "pointermove", { clientX: 150, clientY: 100 });
    expect(view.x).toBe(-50);
  });

  test("two fingers pan as well as pinch when cooperative", () => {
    const { el, view } = harness({ cooperative: () => true });
    pointer(el, "pointerdown", touch(1, 100));
    pointer(el, "pointerdown", touch(2, 300));
    for (let x = 100; x < 160; x++) {
      pointer(el, "pointermove", touch(1, x + 1));
      pointer(el, "pointermove", touch(2, x + 201));
    }
    expect(view.zoom).toBeCloseTo(1, 1);
    expect(view.x).toBeLessThan(-30);
  });

  test("lifting one finger out of a cooperative pinch does not resume a drag", () => {
    const { el, callbacks } = harness({ cooperative: () => true });
    pointer(el, "pointerdown", touch(1, 100));
    pointer(el, "pointerdown", touch(2, 300));
    pointer(el, "pointerup", touch(2, 300));
    callbacks.panTo.mockClear();
    pointer(el, "pointermove", touch(1, 160));
    expect(callbacks.panTo).not.toHaveBeenCalled();
  });

  test("cooperative() is read live", () => {
    let coop = false;
    const { el, callbacks } = harness({ cooperative: () => coop });
    pointer(el, "pointerdown", touch(1, 100));
    pointer(el, "pointermove", touch(1, 150));
    expect(callbacks.panTo).toHaveBeenCalled();
    pointer(el, "pointerup", touch(1, 150));
    coop = true;
    callbacks.panTo.mockClear();
    pointer(el, "pointerdown", touch(2, 100));
    pointer(el, "pointermove", touch(2, 150));
    expect(callbacks.panTo).not.toHaveBeenCalled();
  });
});

describe("wheel", () => {
  test("a plain wheel zooms at the pointer and claims the scroll", () => {
    const { el, zooms } = harness();
    const event = wheel(el, { deltaY: -100, clientX: 120, clientY: 80 });
    expect(event.defaultPrevented).toBe(true);
    expect(zooms[0].factor).toBeCloseTo(1.08);
    expect(zooms[0].at).toEqual({ x: 120, y: 80 });
  });

  test.each([{ ctrlKey: true }, { metaKey: true }])("when a modifier is required, a plain wheel is left for the page and %o zooms", (modifier) => {
    const { el, zooms } = harness({}, { wheelNeedsModifier: () => true });
    const plain = wheel(el, { deltaY: -100 });
    expect(plain.defaultPrevented).toBe(false);
    expect(zooms).toHaveLength(0);
    const modified = wheel(el, { deltaY: -100, ...modifier });
    expect(modified.defaultPrevented).toBe(true);
    expect(zooms).toHaveLength(1);
  });

  test("the modifier requirement is read live", () => {
    let needed = false;
    const { el, zooms } = harness({}, { wheelNeedsModifier: () => needed });
    wheel(el, { deltaY: -100 });
    needed = true;
    wheel(el, { deltaY: -100 });
    expect(zooms).toHaveLength(1);
  });

  test("a wheel over a pass-through element is left to it, but ctrl + wheel there still zooms", () => {
    const { el, zooms } = harness({}, { wheelPassThrough: ".row" });
    const row = el.appendChild(document.createElement("div"));
    row.className = "row";
    const plain = wheel(row, { deltaX: 40, deltaY: 3 });
    expect(plain.defaultPrevented).toBe(false);
    expect(zooms).toHaveLength(0);
    wheel(row, { ctrlKey: true, deltaY: -50 });
    expect(zooms).toHaveLength(1);
  });

  test("a burst zooms by distance and settles once after idle", () => {
    vi.useFakeTimers();
    const { el, view, callbacks } = harness();
    for (let i = 0; i < 10; i++) {
      wheel(el, { deltaY: -10 });
      vi.advanceTimersByTime(8);
    }
    expect(view.zoom).toBeCloseTo(1.08);
    expect(callbacks.onSettle).not.toHaveBeenCalled();
    vi.advanceTimersByTime(120);
    expect(callbacks.onSettle).toHaveBeenCalledTimes(1);
  });

  test("wheel idle does not settle an active pointer pan", () => {
    vi.useFakeTimers();
    const { el, callbacks } = harness();
    wheel(el, { deltaY: -100 });
    pointer(el, "pointerdown", { clientX: 100, clientY: 250 });
    vi.advanceTimersByTime(120);
    expect(callbacks.onSettle).not.toHaveBeenCalled();
    pointer(el, "pointerup", { clientX: 100, clientY: 250 });
    expect(callbacks.onSettle).toHaveBeenCalledTimes(1);
  });

  test("line and page deltas normalise to the distance of pixel deltas; a zero delta does nothing", () => {
    const zoomFor = (deltaY: number, deltaMode: number) => {
      const { el, view } = harness();
      wheel(el, { deltaY, deltaMode });
      return view.zoom;
    };
    expect(zoomFor(-1, 1)).toBeCloseTo(zoomFor(-16, 0));
    expect(zoomFor(-0.2, 2)).toBeCloseTo(zoomFor(-100, 0));
    expect(zoomFor(0, 0)).toBe(1);
  });
});

describe("keyboard", () => {
  test("+ and = zoom in and − zooms out by the step, about the centre, and each settles", () => {
    const { el, zooms, callbacks } = harness();
    key(el, "+");
    key(el, "=");
    key(el, "-");
    expect(zooms.map((z) => z.factor)).toEqual([1.5, 1.5, 1 / 1.5]);
    expect(zooms[0].at).toEqual({ x: 400, y: 250 });
    expect(callbacks.onSettle).toHaveBeenCalledTimes(3);
  });

  test("an arrow moves the view that way and Shift moves it further", () => {
    const { el, view } = harness();
    key(el, "ArrowRight");
    expect(view.x).toBe(32);
    key(el, "ArrowLeft", { shiftKey: true });
    expect(view.x).toBe(32 - 70);
    key(el, "ArrowUp");
    key(el, "ArrowDown");
    expect(view.y).toBe(0);
  });

  test("0 resets when the map has a reset, and is left alone when it does not", () => {
    const { el, callbacks } = harness();
    expect(key(el, "0").defaultPrevented).toBe(true);
    expect(callbacks.reset).toHaveBeenCalledTimes(1);
    const bare = harness({ reset: undefined });
    expect(key(bare.el, "0").defaultPrevented).toBe(false);
  });

  test("a key pressed in a button, or with a browser-zoom modifier, is not the map's", () => {
    const { el, zooms } = harness();
    const button = el.appendChild(document.createElement("button"));
    key(button, "+");
    key(el, "-", { ctrlKey: true });
    key(el, "=", { metaKey: true });
    expect(zooms).toHaveLength(0);
  });
});
