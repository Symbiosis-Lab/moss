/** The country map's zoom: its controls, the keyboard path and the modifier-gated wheel. */
import { beforeEach, describe, expect, test } from "vitest";
import { boot, media, page, REDUCED, WIDE } from "./fixture";

const svg = () => document.querySelector("svg")!;
const width = () => Number(svg().getAttribute("viewBox")!.split(" ")[2]);
const control = (name: string) => document.querySelector<HTMLButtonElement>(`[data-control="${name}"]`)!;
const stage = () => document.querySelector<HTMLElement>(".moss-country-stage")!;

beforeEach(async () => {
  page();
  location.hash = "";
  media({ [WIDE]: true, [REDUCED]: true });
  await boot();
});

describe("controls", () => {
  test("sit in the stage, named, with their group", () => {
    expect(stage().querySelector('[role="group"][aria-label="Map controls"]')).not.toBeNull();
    expect(control("zoom-in").getAttribute("aria-label")).toBe("Zoom in");
    expect(control("zoom-out").getAttribute("aria-label")).toBe("Zoom out");
    expect(control("reset").getAttribute("aria-label")).toBe("Show the whole map");
  });
  test("at the whole view only + works, and reset is out of the tab order", () => {
    expect(control("zoom-out").disabled).toBe(true);
    expect(control("zoom-in").disabled).toBe(false);
    expect(control("reset").hidden).toBe(true);
  });
  test("+ steps in by 1.5, − back out, and reset appears while zoomed", () => {
    control("zoom-in").click();
    expect(width()).toBeCloseTo(1000 / 1.5);
    expect(control("zoom-out").disabled).toBe(false);
    expect(control("reset").hidden).toBe(false);
    control("zoom-out").click();
    expect(svg().getAttribute("viewBox")).toBe("0 0 1000 435");
    expect(control("reset").hidden).toBe(true);
  });
  test("+ stops at 8 times the whole view", () => {
    for (let i = 0; i < 12; i++) control("zoom-in").click();
    expect(width()).toBeCloseTo(125);
    expect(control("zoom-in").disabled).toBe(true);
  });
  test("reset returns to the whole view", () => {
    control("zoom-in").click();
    control("zoom-in").click();
    control("reset").click();
    expect(svg().getAttribute("viewBox")).toBe("0 0 1000 435");
  });
});

describe("the stage", () => {
  test("is a focusable region named by the map and described by the keys", () => {
    expect(stage().tabIndex).toBe(0);
    expect(stage().getAttribute("role")).toBe("region");
    const hint = document.getElementById(stage().getAttribute("aria-describedby")!)!;
    expect(hint.textContent).toBe("Use + and − to zoom.");
  });
  test("+, −, 0 and the arrows work while it has focus", () => {
    svg().getBoundingClientRect = () => ({ left: 0, top: 0, right: 800, bottom: 348, width: 800, height: 348, x: 0, y: 0, toJSON() {} }) as DOMRect;
    const press = (key: string) => stage().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    press("+");
    expect(width()).toBeCloseTo(1000 / 1.5);
    press("ArrowRight");
    expect(Number(svg().getAttribute("viewBox")!.split(" ")[0])).toBeGreaterThan(0);
    press("0");
    expect(svg().getAttribute("viewBox")).toBe("0 0 1000 435");
    press("-");
    expect(width()).toBe(1000);
  });
  test("only ctrl or ⌘ + wheel zooms; a plain wheel is left for the page", () => {
    const wheel = (init: WheelEventInit) => {
      const event = new WheelEvent("wheel", { deltaY: -100, bubbles: true, cancelable: true, ...init });
      stage().dispatchEvent(event);
      return event;
    };
    expect(wheel({}).defaultPrevented).toBe(false);
    expect(width()).toBe(1000);
    expect(wheel({ ctrlKey: true }).defaultPrevented).toBe(true);
    const after = width();
    expect(after).toBeLessThan(1000);
    wheel({ metaKey: true });
    expect(width()).toBeLessThan(after);
  });
});
