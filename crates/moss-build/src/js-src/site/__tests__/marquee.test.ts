/**
 * Tests for marquee.ts — the page's pause button is drawn once, before the
 * first strip that moves, and survives a reload through storage. jsdom lays
 * nothing out, so widths are stubbed by hand.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { initMarquees } from "../marquee";

function strip(setWidth: number, boxWidth: number): HTMLElement {
  const cards = document.createElement("div");
  cards.className = "moss-cards";
  const el = document.createElement("div");
  el.setAttribute("data-motion", "marquee");
  el.innerHTML = '<div class="moss-marquee-track"><div class="moss-marquee-set"></div></div>';
  Object.defineProperty(el, "clientWidth", { value: boxWidth, configurable: true });
  el.querySelector(".moss-marquee-set")!.getBoundingClientRect = () => ({ width: setWidth }) as DOMRect;
  cards.appendChild(el);
  document.body.appendChild(cards);
  return el;
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  localStorage.clear();
});
afterEach(() => {
  document.body.innerHTML = "";
  document.documentElement.removeAttribute("data-marquee-paused");
  vi.unstubAllGlobals();
});

const buttons = () => [...document.querySelectorAll<HTMLButtonElement>("button.moss-marquee-control")];

describe("the pause button", () => {
  test("one button sits before the first moving strip, not inside its link", () => {
    strip(900, 600);
    strip(900, 600);
    initMarquees();
    expect(buttons()).toHaveLength(1);
    expect(buttons()[0].nextElementSibling).toBe(document.querySelector(".moss-cards"));
    expect(buttons()[0].textContent).toBe("Pause motion");
  });

  test("clicking pauses every strip, flips the label and is remembered", () => {
    strip(900, 600);
    initMarquees();
    buttons()[0].click();
    expect(document.documentElement.hasAttribute("data-marquee-paused")).toBe(true);
    expect(buttons()[0].textContent).toBe("Play motion");
    expect(localStorage.getItem("moss-marquee-paused")).toBe("1");
    buttons()[0].click();
    expect(document.documentElement.hasAttribute("data-marquee-paused")).toBe(false);
    expect(localStorage.getItem("moss-marquee-paused")).toBe("0");
  });

  test("a remembered pause applies on the next page", () => {
    localStorage.setItem("moss-marquee-paused", "1");
    strip(900, 600);
    initMarquees();
    expect(document.documentElement.hasAttribute("data-marquee-paused")).toBe(true);
    expect(buttons()[0].textContent).toBe("Play motion");
  });

  test("no strip moves, so no button is shown", () => {
    const short = strip(300, 600);
    initMarquees();
    expect(short.hasAttribute("data-marquee-still")).toBe(true);
    expect(buttons().every((b) => b.hidden)).toBe(true);
  });
});
