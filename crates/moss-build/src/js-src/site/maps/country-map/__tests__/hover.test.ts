/** Hover previews and the legend lift; neither commits anything. */
import { beforeEach, describe, expect, test } from "vitest";
import { placeLabel } from "../hover";
import { $, boot, HOVER, media, page, panel } from "./fixture";

const stage = { w: 800, h: 400 };
const label = { w: 120, h: 24 };

describe("placeLabel", () => {
  test("sits below and right of the pointer", () => {
    expect(placeLabel({ x: 100, y: 100 }, label, stage, null)).toEqual({ x: 114, y: 118 });
  });
  test("flips left of the pointer at the right edge and above it at the bottom", () => {
    expect(placeLabel({ x: 760, y: 100 }, label, stage, null)).toEqual({ x: 626, y: 118 });
    expect(placeLabel({ x: 100, y: 390 }, label, stage, null)!.y).toBe(352);
  });
  test("stays inside the map", () => {
    const at = placeLabel({ x: 2, y: 2 }, label, stage, null)!;
    expect(at.x).toBeGreaterThanOrEqual(8);
    const far = placeLabel({ x: 799, y: 399 }, label, stage, null)!;
    expect(far.x + label.w).toBeLessThanOrEqual(792);
    expect(far.y + label.h).toBeLessThanOrEqual(392);
  });
  test("flips away from the panel, and hides when both sides would cover it", () => {
    const panel = { x0: 480, y0: 0, x1: 800, y1: 400 };
    expect(placeLabel({ x: 400, y: 100 }, label, stage, panel)!.x).toBeLessThan(400);
    expect(placeLabel({ x: 600, y: 100 }, label, stage, panel)).toBeNull();
  });
});

const move = (id: string, x = 50, y = 50) => document.getElementById(id)!.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, clientX: x, clientY: y }));
const tip = () => document.querySelector<HTMLElement>(".moss-country-tip");

describe("hover label", () => {
  beforeEach(() => {
    page();
    location.hash = "";
  });
  test("names the country and its level, and leaves the map alone", async () => {
    media({ [HOVER]: true });
    await boot();
    move("mexico");
    expect(tip()!.hidden).toBe(false);
    expect(tip()!.textContent).toBe("Mexico · High");
    move("chad");
    expect(tip()!.textContent).toBe("Chad · Not rated");
    expect(panel().hidden).toBe(true);
    expect(document.querySelector(".is-chosen")).toBeNull();
  });
  test("disputed land gives no label", async () => {
    media({ [HOVER]: true });
    await boot();
    move("mexico");
    move("disputed");
    expect(tip()!.hidden).toBe(true);
  });
  test("a device that cannot hover gets no label", async () => {
    media({});
    await boot();
    move("mexico");
    expect(tip()?.hidden ?? true).toBe(true);
  });
});

describe("legend lift", () => {
  const items = () => [...document.querySelectorAll<HTMLElement>(".moss-country-legend li")];
  beforeEach(async () => {
    page();
    media({ [HOVER]: true });
    await boot();
  });
  test("legend items are no tab stops: the list below answers the same question", () => {
    expect(items().map((li) => li.hasAttribute("tabindex"))).toEqual([false, false]);
  });
  test("hovering a level lifts its countries and dims the rest; leaving restores", () => {
    items()[0].dispatchEvent(new MouseEvent("mouseenter"));
    expect($("figure").classList.contains("is-lifting")).toBe(true);
    expect(document.getElementById("japan")!.classList.contains("is-lifted")).toBe(true);
    expect(document.getElementById("mexico")!.classList.contains("is-lifted")).toBe(false);
    items()[0].dispatchEvent(new MouseEvent("mouseleave"));
    expect($("figure").classList.contains("is-lifting")).toBe(false);
    expect(document.querySelector(".is-lifted")).toBeNull();
  });
});
