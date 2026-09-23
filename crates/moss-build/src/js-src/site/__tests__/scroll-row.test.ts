/**
 * Tests for scroll-row.ts — the dots are built from the row's own cards,
 * named for assistive tech, bring their card into view on click, and survive
 * a preview morph without compounding their listeners (idiomorph reuses the
 * row but drops the injected dots, so `moss-morph-patched` re-inits it).
 */
import { afterEach, describe, expect, test, vi } from "vitest";
import { initScrollDots, onWheel } from "../scroll-row";

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

describe("re-init after a preview morph", () => {
  test("a morph that drops the dots gets exactly one set back, and one wheel event still moves by one delta", () => {
    const g = row(4);
    setOverflow(g, 400);
    initScrollDots();
    // idiomorph reuses `g` (it matches the served row) but removes the
    // injected `.moss-scroll-dots`, which has no counterpart in the served
    // HTML — that is the actual shape of a morph, not a second initScrollDots() call.
    g.nextElementSibling!.remove();
    document.dispatchEvent(new CustomEvent("moss-morph-patched"));

    expect(document.querySelectorAll(".moss-scroll-dots")).toHaveLength(1);
    g.dispatchEvent(wheel({ deltaY: 100 }));
    expect(g.scrollLeft).toBe(100);
  });
});

describe("wheel over the row", () => {
  function scroller(pos: number, max = 400): HTMLElement {
    const g = row(4);
    setOverflow(g, max);
    g.scrollLeft = pos;
    return g;
  }

  test("a vertical wheel scrolls the row sideways and not the page", () => {
    const g = scroller(0);
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    expect(e.defaultPrevented).toBe(true);
    expect(g.scrollLeft).toBe(100);
  });

  test("at the end of the row the wheel passes through to the page", () => {
    const g = scroller(400);
    const down = wheel({ deltaY: 100 });
    onWheel(g, down);
    expect(down.defaultPrevented).toBe(false);
    const start = scroller(0);
    const up = wheel({ deltaY: -100 });
    onWheel(start, up);
    expect(up.defaultPrevented).toBe(false);
  });

  test("a trackpad's sideways gesture and pinch-zoom are left to the browser", () => {
    const g = scroller(100);
    const sideways = wheel({ deltaX: 80, deltaY: 10 });
    onWheel(g, sideways);
    expect(sideways.defaultPrevented).toBe(false);
    const pinch = wheel({ deltaY: 50, ctrlKey: true });
    onWheel(g, pinch);
    expect(pinch.defaultPrevented).toBe(false);
  });

  test("a row with nothing to scroll never takes the wheel", () => {
    const g = scroller(0, 0);
    const e = wheel({ deltaY: 100 });
    onWheel(g, e);
    expect(e.defaultPrevented).toBe(false);
  });

  test("deltaMode 1 reports lines, converted to pixels at 16px each", () => {
    const g = scroller(0);
    onWheel(g, wheel({ deltaY: 5, deltaMode: 1 }));
    expect(g.scrollLeft).toBe(80);
  });

  test("an RTL row moves scrollLeft the other way", () => {
    const g = scroller(0);
    g.style.direction = "rtl";
    onWheel(g, wheel({ deltaY: 100 }));
    expect(g.scrollLeft).toBe(-100);
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
