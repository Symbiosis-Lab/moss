/**
 * Choosing a country opens one panel from three entry points (a tap on the
 * map, a list row, the address hash); Esc, the close button and empty sea
 * clear it; disputed land is inert.
 */
import { beforeEach, describe, expect, test } from "vitest";
import { boot, click, media, page, panel, REDUCED, WIDE } from "./fixture";

describe("country map panel", () => {
  beforeEach(async () => {
    page();
    location.hash = "";
    media({});
    await boot();
  });

  test("a tap shows the name, level and the row's own columns", () => {
    click("mexico");
    expect(panel().hidden).toBe(false);
    expect(panel().textContent).toContain("Mexico");
    expect(panel().textContent).toContain("High");
    expect(panel().textContent).toContain("Night travel.");
    expect(panel().textContent).not.toContain("Avoid travel.");
    expect(document.getElementById("mexico")!.classList.contains("is-chosen")).toBe(true);
  });

  test("a row with no extra columns shows its level's text and the action", () => {
    click("japan");
    expect(panel().textContent).toContain("Take normal precautions.");
    expect(panel().querySelector("a")!.getAttribute("href")).toBe("/ask/?to=Japan");
  });

  test("an unrated country shows Not rated, the unrated text and the action", () => {
    click("chad");
    expect(panel().textContent).toContain("Chad");
    expect(panel().textContent).toContain("Not rated");
    expect(panel().textContent).toContain("Not assessed.");
    expect(panel().querySelector("a")!.getAttribute("href")).toBe("/ask/?to=Chad");
  });

  test("disputed land is inert and the sea clears", () => {
    click("japan");
    click("disputed");
    expect(panel().hidden).toBe(false);
    click("sea");
    expect(panel().hidden).toBe(true);
  });

  test("Esc and the close button clear", () => {
    click("japan");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(panel().hidden).toBe(true);
    click("japan");
    panel().querySelector<HTMLElement>(".moss-country-panel-close")!.click();
    expect(panel().hidden).toBe(true);
  });

  test("a list row opens the same panel", () => {
    click("row");
    expect(panel().textContent).toContain("Mexico");
  });
});

describe("the address", () => {
  beforeEach(async () => {
    page();
    location.hash = "";
    media({});
    await boot();
  });
  test("a tap puts the country in the address and Esc takes it out", () => {
    click("mexico");
    expect(location.hash).toBe("#mexico");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(location.hash).toBe("");
  });
  test("a list row chooses without leaving the map for the row", () => {
    const row = document.getElementById("row")!;
    const event = new MouseEvent("click", { bubbles: true, cancelable: true });
    row.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(location.hash).toBe("#mexico");
  });
});

describe("framing", () => {
  const svg = () => document.querySelector("svg")!;
  const rect = (el: Element, r: Partial<DOMRect>) => (el.getBoundingClientRect = () => ({ left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: 0, toJSON() {}, ...r }) as DOMRect);
  const layout = () => {
    rect(document.querySelector(".moss-country-stage")!, { left: 0, right: 800, width: 800, height: 348 });
    rect(panel(), { left: 480, right: 800, width: 320 });
  };
  beforeEach(() => {
    page();
    location.hash = "";
  });
  test("choosing on a wide screen moves the view to the country at once under reduced motion", async () => {
    media({ [WIDE]: true, [REDUCED]: true });
    await boot();
    layout();
    click("japan");
    const [x, , w] = svg().getAttribute("viewBox")!.split(" ").map(Number);
    expect(w).toBeCloseTo(400);
    expect(x).toBeGreaterThan(500);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(svg().getAttribute("viewBox")).toBe("0 0 1000 435");
  });
  test("reading the address on load frames without animating", async () => {
    media({ [WIDE]: true });
    location.hash = "#japan";
    await boot();
    expect(svg().getAttribute("viewBox")).not.toBe("0 0 1000 435");
  });
});

describe("closing the panel", () => {
  const svg = () => document.querySelector("svg")!;
  const viewBox = () => svg().getAttribute("viewBox");
  const esc = () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  const zoomIn = () => document.querySelector<HTMLElement>('[data-control="zoom-in"]')!.click();
  beforeEach(async () => {
    page();
    location.hash = "";
    media({ [WIDE]: true, [REDUCED]: true });
    await boot();
  });
  test("returns to the view the reader had before choosing", () => {
    zoomIn();
    const before = viewBox();
    expect(before).not.toBe("0 0 1000 435");
    click("japan");
    expect(viewBox()).not.toBe(before);
    esc();
    expect(viewBox()).toBe(before);
  });
  test("leaves the view where it is when the reader zoomed while the country was open", () => {
    click("japan");
    zoomIn();
    const after = viewBox();
    esc();
    expect(viewBox()).toBe(after);
  });
  test("choosing a second country does not replace the view to return to", () => {
    click("japan");
    click("mexico");
    esc();
    expect(viewBox()).toBe("0 0 1000 435");
  });
  test("reset shows the whole view and keeps the country", () => {
    click("japan");
    document.querySelector<HTMLElement>('[data-control="reset"]')!.click();
    expect(viewBox()).toBe("0 0 1000 435");
    expect(panel().hidden).toBe(false);
    expect(document.getElementById("japan")!.classList.contains("is-chosen")).toBe(true);
  });
});

describe("focus", () => {
  beforeEach(async () => {
    page();
    location.hash = "";
    media({});
    await boot();
  });
  const heading = () => document.querySelector<HTMLElement>(".moss-country-panel-name");
  test("a tap leaves focus where it was, before and after Esc", () => {
    click("mexico");
    expect(document.activeElement).toBe(document.body);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.activeElement).toBe(document.body);
  });
  test("a list row opened from the keyboard focuses the heading, and the close button returns to the row", () => {
    const row = document.getElementById("row")!;
    row.focus();
    row.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 0 }));
    expect(document.activeElement).toBe(heading());
    panel().querySelector<HTMLElement>(".moss-country-panel-close")!.click();
    expect(document.activeElement).toBe(row);
  });
  test("a list row clicked with a pointer leaves focus alone", () => {
    document.getElementById("row")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }));
    expect(document.activeElement).not.toBe(heading());
  });
});

describe("listeners", () => {
  test("re-running setup after a morph leaves one live handler", async () => {
    page();
    location.hash = "";
    media({});
    await boot();
    const old = panel();
    page();
    document.dispatchEvent(new Event("moss-morph-patched"));
    location.hash = "#japan";
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    expect(panel().hidden).toBe(false);
    expect(old.hidden, "the figure the morph replaced no longer listens").toBe(true);
  });
});
