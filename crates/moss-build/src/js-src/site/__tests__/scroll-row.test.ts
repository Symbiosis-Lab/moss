/**
 * Tests for scroll-row.ts — the dots are built from the row's own cards,
 * named for assistive tech, bring their card into view on click, and survive
 * a preview morph without compounding their listeners (idiomorph reuses the
 * row but drops the injected dots, so `moss-morph-patched` re-inits it).
 */
import { afterEach, describe, expect, test, vi } from "vitest";
import { __resetWheelGestureForTests, initScrollDots, onWheel } from "../scroll-row";

function row(cards: number, label?: string): HTMLElement {
  const g = document.createElement("div");
  g.className = "moss-grid";
  g.setAttribute("data-scroll", "");
  // The base `.moss-grid[data-scroll]` rule (site.css) always declares this;
  // real rows never see anything else on the horizontal axis, so it is the
  // default here too, not something each horizontal test restates.
  g.style.overflowX = "auto";
  if (label) g.setAttribute("aria-label", label);
  for (let i = 0; i < cards; i++) {
    const c = document.createElement("a");
    c.className = "moss-grid-card";
    c.scrollIntoView = vi.fn();
    g.appendChild(c);
  }
  document.body.appendChild(g);
  return g;
}

/** jsdom never lays anything out, so overflow has to be stubbed by hand. */
function setOverflow(g: HTMLElement, max: number): void {
  Object.defineProperty(g, "scrollWidth", { value: 1000 + max, configurable: true });
  Object.defineProperty(g, "clientWidth", { value: 1000, configurable: true });
}

/** The vertical-typesetting counterpart of `setOverflow`: a row whose inline
 * axis (the one it scrolls on under `writing-mode: vertical-rl`) is vertical. */
function setVerticalOverflow(g: HTMLElement, max: number): void {
  g.style.writingMode = "vertical-rl";
  g.style.overflowY = "auto";
  Object.defineProperty(g, "scrollHeight", { value: 1000 + max, configurable: true });
  Object.defineProperty(g, "clientHeight", { value: 1000, configurable: true });
}

const wheel = (init: WheelEventInit) => new WheelEvent("wheel", { cancelable: true, ...init });

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  // jsdom doesn't implement matchMedia at all, so a test that wants
  // prefers-reduced-motion assigns it directly rather than spying on an
  // existing method; undo that by hand since restoreAllMocks won't.
  Reflect.deleteProperty(window, "matchMedia");
  // A test that dispatches real wheel events to exercise gesture latching
  // would otherwise leak which row "owns" the gesture into the next test.
  __resetWheelGestureForTests();
});

describe("scroll dots", () => {
  test("one labelled dot per card, in a group named after the row", () => {
    const g = row(4, "Related");
    initScrollDots();
    const nav = g.nextElementSibling as HTMLElement;
    expect(nav.className).toBe("moss-scroll-dots");
    expect(nav.getAttribute("role")).toBe("group");
    expect(nav.getAttribute("aria-label")).toBe("Related");
    const labels = Array.from(nav.querySelectorAll("button")).map((b) => b.getAttribute("aria-label"));
    expect(labels).toEqual(["1 / 4", "2 / 4", "3 / 4", "4 / 4"]);
    expect(nav.dataset.indicator).toBe("dots");
  });

  test.each([10, 11])("uses dots at %s cards and a passive fraction above 10", (count) => {
    const g = row(count, "Related");
    initScrollDots();
    const nav = g.nextElementSibling as HTMLElement;
    expect(nav.dataset.indicator).toBe(count <= 10 ? "dots" : "fraction");
    expect(nav.querySelectorAll("button")).toHaveLength(count <= 10 ? count : 0);
    if (count > 10) {
      expect(nav.querySelector(".moss-scroll-fraction")?.textContent).toBe("1 / 11");
      expect(nav.querySelectorAll("[data-current]")).toHaveLength(1);
      expect(nav.getAttribute("role")).toBeNull();
      expect(nav.getAttribute("aria-label")).toBeNull();
      expect(nav.getAttribute("aria-hidden")).toBe("true");
      expect(nav.querySelectorAll("[tabindex], button, a, input, select, textarea")).toHaveLength(0);
    }
  });

  test("updates the fraction from the first visible card without a live region", () => {
    const g = row(11);
    let intersectionCallback: IntersectionObserverCallback | undefined;
    class FakeIntersectionObserver {
      constructor(observerCallback: IntersectionObserverCallback) {
        intersectionCallback = observerCallback;
      }
      observe = vi.fn();
      disconnect = vi.fn();
    }
    vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
    initScrollDots();
    intersectionCallback?.([
      { target: g.children[4], isIntersecting: true } as IntersectionObserverEntry,
    ], {} as IntersectionObserver);
    const nav = g.nextElementSibling as HTMLElement;
    expect(nav.querySelector("[data-current]")?.textContent).toBe("5");
    expect(nav.hasAttribute("aria-live")).toBe(false);
    expect(nav.querySelector(".moss-scroll-fraction")?.getAttribute("aria-hidden")).toBe("true");
  });

  test("clicking a dot brings its card to the start of the row", () => {
    const g = row(3);
    initScrollDots();
    (g.nextElementSibling!.querySelectorAll("button")[2] as HTMLButtonElement).click();
    const third = g.children[2] as HTMLElement;
    expect(third.scrollIntoView).toHaveBeenCalledWith(expect.objectContaining({ inline: "start", block: "nearest" }));
    expect((g.children[0] as HTMLElement).scrollIntoView).not.toHaveBeenCalled();
  });

  test("a grid without the scroll flag gets no dots", () => {
    const g = row(3);
    g.removeAttribute("data-scroll");
    initScrollDots();
    expect(document.querySelectorAll(".moss-scroll-dots")).toHaveLength(0);
  });

  test("the dots hide when the row doesn't overflow, and show when it does", () => {
    const flush = row(3);
    setOverflow(flush, 0);
    const overflowing = row(3);
    setOverflow(overflowing, 400);
    initScrollDots();
    expect((flush.nextElementSibling as HTMLElement).hidden).toBe(true);
    expect((overflowing.nextElementSibling as HTMLElement).hidden).toBe(false);
  });

  test("a row whose computed overflow-x is visible keeps its dots hidden even though it overflows", () => {
    // The shape of the originally reported bug: a size mismatch alone
    // (scrollWidth > clientWidth) used to be read as "scrollable", even on
    // an axis the row was never declared to scroll on.
    const g = row(3);
    g.style.overflowX = "visible";
    setOverflow(g, 400);
    initScrollDots();
    expect((g.nextElementSibling as HTMLElement).hidden).toBe(true);
  });

  test("a vertical row shows its dots once it actually overflows on its own (inline/vertical) axis", () => {
    const g = row(4);
    setVerticalOverflow(g, 400);
    initScrollDots();
    expect((g.nextElementSibling as HTMLElement).hidden).toBe(false);
  });

  test("a vertical row whose computed overflow-y isn't auto/scroll keeps its dots hidden, even though scrollHeight exceeds clientHeight", () => {
    const g = row(4);
    g.style.writingMode = "vertical-rl";
    g.style.overflowY = "visible";
    Object.defineProperty(g, "scrollHeight", { value: 1400, configurable: true });
    Object.defineProperty(g, "clientHeight", { value: 1000, configurable: true });
    initScrollDots();
    expect((g.nextElementSibling as HTMLElement).hidden).toBe(true);
  });
});

describe("keyboard reachability follows the dots", () => {
  // A `data-fits` row ships `tabindex="0"` in the HTML so a no-JS narrow view
  // stays keyboard-scrollable, but once the script can see it isn't actually
  // scrollable at the current width it shouldn't be a pointless tab stop —
  // `fit()` toggles `tabIndex` off the same `isScrollableRow` check that
  // hides the dots, so the two never disagree.
  test("a row that doesn't overflow is pulled out of the tab order", () => {
    const g = row(3);
    setOverflow(g, 0);
    initScrollDots();
    expect(g.tabIndex).toBe(-1);
  });

  test("a row that overflows stays a tab stop", () => {
    const g = row(3);
    setOverflow(g, 400);
    initScrollDots();
    expect(g.tabIndex).toBe(0);
  });

  test("a vertical row follows the same rule on its own (block) axis", () => {
    const flush = row(4);
    setVerticalOverflow(flush, 0);
    const overflowing = row(4);
    setVerticalOverflow(overflowing, 400);
    initScrollDots();
    expect(flush.tabIndex).toBe(-1);
    expect(overflowing.tabIndex).toBe(0);
  });
});

describe("re-init after a preview morph", () => {
  test("a morph that drops the dots gets exactly one set back, and one wheel event still asks the row to scroll", () => {
    const g = row(4);
    setOverflow(g, 400);
    g.scrollTo = vi.fn();
    initScrollDots();
    // idiomorph reuses `g` (it matches the served row) but removes the
    // injected `.moss-scroll-dots`, which has no counterpart in the served
    // HTML — that is the actual shape of a morph, not a second initScrollDots() call.
    g.nextElementSibling!.remove();
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    expect(document.querySelectorAll(".moss-scroll-dots")).toHaveLength(1);
    g.dispatchEvent(wheel({ deltaY: 100 }));
    expect(g.scrollTo).toHaveBeenCalledWith({ left: 100, behavior: "smooth" });
  });
});

describe("wheel over the row", () => {
  function scroller(pos: number, max = 400): HTMLElement {
    const g = row(4);
    setOverflow(g, max);
    g.scrollLeft = pos;
    // jsdom implements neither scrollTo nor scrollBy on elements at all, so
    // every test stubs whichever one onWheel actually calls.
    g.scrollTo = vi.fn();
    return g;
  }

  test("a vertical wheel scrolls the row sideways via a smooth scrollTo, never a direct scrollLeft write", () => {
    const g = scroller(0);
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    expect(e.defaultPrevented).toBe(true);
    expect(g.scrollTo).toHaveBeenCalledWith({ left: 100, behavior: "smooth" });
    expect(g.scrollLeft).toBe(0);
  });

  test("prefers-reduced-motion swaps the glide for an instant jump", () => {
    const g = scroller(0);
    window.matchMedia = vi.fn().mockReturnValue({ matches: true }) as typeof window.matchMedia;
    onWheel(g, wheel({ deltaY: 100 }));
    expect(g.scrollTo).toHaveBeenCalledWith({ left: 100, behavior: "auto" });
  });

  test("at the end of the row the wheel passes through to the page", () => {
    const g = scroller(400);
    const down = wheel({ deltaY: 100 });
    onWheel(g, down);
    expect(down.defaultPrevented).toBe(false);
    expect(g.scrollTo).not.toHaveBeenCalled();
    const start = scroller(0);
    const up = wheel({ deltaY: -100 });
    onWheel(start, up);
    expect(up.defaultPrevented).toBe(false);
    expect(start.scrollTo).not.toHaveBeenCalled();
  });

  test("a trackpad's sideways gesture and pinch-zoom are left to the browser", () => {
    const g = scroller(100);
    const sideways = wheel({ deltaX: 80, deltaY: 10 });
    onWheel(g, sideways);
    expect(sideways.defaultPrevented).toBe(false);
    const pinch = wheel({ deltaY: 50, ctrlKey: true });
    onWheel(g, pinch);
    expect(pinch.defaultPrevented).toBe(false);
    expect(g.scrollTo).not.toHaveBeenCalled();
  });

  test("a row with nothing to scroll never takes the wheel", () => {
    const g = scroller(0, 0);
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    expect(e.defaultPrevented).toBe(false);
    expect(g.scrollTo).not.toHaveBeenCalled();
  });

  test("deltaMode 1 reports lines, converted to pixels at 16px each", () => {
    const g = scroller(0);
    onWheel(g, wheel({ deltaY: 5, deltaMode: 1 }));
    expect(g.scrollTo).toHaveBeenCalledWith({ left: 80, behavior: "smooth" });
  });

  test("an RTL row moves the scroll target the other way", () => {
    const g = scroller(0);
    g.style.direction = "rtl";
    onWheel(g, wheel({ deltaY: 100 }));
    expect(g.scrollTo).toHaveBeenCalledWith({ left: -100, behavior: "smooth" });
  });

  test("a target that overshoots the end is clamped, so reversing right after moves back immediately", () => {
    // A single large notch (a fast wheel, or OS scroll acceleration) can add
    // more than the remaining distance in one step. Without clamping the
    // running target itself overshoots past `max`, and every reversed notch
    // afterward has to "unwind" that overshoot before the row visibly moves
    // — it would take two reversed notches here to undo an unclamped 790px
    // overshoot, instead of one.
    const g = scroller(390); // 10px from the end (max = 400)
    onWheel(g, wheel({ deltaY: 800 }));
    expect(g.scrollTo).toHaveBeenLastCalledWith({ left: 400, behavior: "smooth" });
    onWheel(g, wheel({ deltaY: -100 }));
    expect(g.scrollTo).toHaveBeenLastCalledWith({ left: 300, behavior: "smooth" });
  });
});

describe("wheel gesture latching", () => {
  function scroller(pos: number, max = 400): HTMLElement {
    const g = row(4);
    setOverflow(g, max);
    g.scrollLeft = pos;
    g.scrollTo = vi.fn();
    return g;
  }

  test("a gesture that began on the page is not taken by the row even once it passes under the cursor", () => {
    const g = scroller(0);
    document.body.dispatchEvent(wheel({ deltaY: 50 })); // the gesture starts on the page
    const e = wheel({ deltaY: 100 });
    onWheel(g, e); // <200ms later, cursor now over the row
    expect(e.defaultPrevented).toBe(false);
    expect(g.scrollTo).not.toHaveBeenCalled();
  });

  test("a fresh gesture, starting more than 200ms after the last wheel event anywhere, is taken by the row", async () => {
    const g = scroller(0);
    document.body.dispatchEvent(wheel({ deltaY: 50 })); // an old, unrelated page gesture
    await new Promise((resolve) => setTimeout(resolve, 220));
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    expect(e.defaultPrevented).toBe(true);
    expect(g.scrollTo).toHaveBeenCalledWith({ left: 100, behavior: "smooth" });
  });
});

describe("wheel over a vertical row", () => {
  test("a vertical wheel is left to the browser's own native scroll, but kept from theme.ts's page-wide handler", () => {
    const g = row(4);
    setVerticalOverflow(g, 400);
    const stop = vi.spyOn(WheelEvent.prototype, "stopPropagation");
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    // Neither preventDefault (the browser's native scroll is what should
    // move the row) nor a manual scrollTop write (there is none in onWheel).
    expect(e.defaultPrevented).toBe(false);
    expect(g.scrollTop).toBe(0);
    expect(stop).toHaveBeenCalled();
  });

  test("at the end of a vertical row the wheel passes all the way through, uncancelled and unstopped", () => {
    const g = row(4);
    setVerticalOverflow(g, 400);
    g.scrollTop = 400;
    const stop = vi.spyOn(WheelEvent.prototype, "stopPropagation");
    const down = wheel({ deltaY: 100 });
    onWheel(g, down);
    expect(down.defaultPrevented).toBe(false);
    expect(stop).not.toHaveBeenCalled();

    const top = row(4);
    setVerticalOverflow(top, 400);
    const stop2 = vi.spyOn(WheelEvent.prototype, "stopPropagation");
    const up = wheel({ deltaY: -100 });
    onWheel(top, up);
    expect(up.defaultPrevented).toBe(false);
    expect(stop2).not.toHaveBeenCalled();
  });

  test("a vertical row with nothing to scroll never intervenes", () => {
    const g = row(2);
    setVerticalOverflow(g, 0);
    const stop = vi.spyOn(WheelEvent.prototype, "stopPropagation");
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    expect(e.defaultPrevented).toBe(false);
    expect(stop).not.toHaveBeenCalled();
  });

  test("a trackpad's sideways gesture over a vertical row is left alone", () => {
    const g = row(4);
    setVerticalOverflow(g, 400);
    const stop = vi.spyOn(WheelEvent.prototype, "stopPropagation");
    const sideways = wheel({ deltaX: 80, deltaY: 10 });
    onWheel(g, sideways);
    expect(sideways.defaultPrevented).toBe(false);
    expect(stop).not.toHaveBeenCalled();
  });
});
