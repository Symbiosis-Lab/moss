/**
 * Tests for chip.ts — the breadcrumb scope chip.
 *
 * `deriveCrumbs` is tested as a pure function (no DOM). Everything that
 * needs a real menu/trail/URL round trip mounts the whole map the way
 * map.test.ts does: `chip.ts` is wired inside `mountPlacesMap`, not a
 * second place a site could mount it from, so that is the only way to
 * prove the chip's own DOM actually reaches `setScope`/the URL rather
 * than just calling a mocked callback.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { deriveCrumbs } from "../chip";
import { mountPlacesMap } from "../map";
import { readUrlState } from "../state";
import { copyFor } from "../strings";
import type { Place, Work } from "../types";

const strings = copyFor("en");

const portugal: Place = { id: "places/portugal", name: "Portugal", precision: "country", lat: 39.5, lng: -8 };
const porto: Place = { id: "places/porto", name: "Porto", parent: "places/portugal", precision: "city", lat: 41.15, lng: -8.6 };
const coimbra: Place = { id: "places/coimbra", name: "Coimbra", parent: "places/portugal", precision: "city", lat: 40.2, lng: -8.4 };
const braga: Place = { id: "places/braga", name: "Braga", parent: "places/portugal", precision: "city", lat: 41.5, lng: -8.4 };
const places: Place[] = [portugal, porto, coimbra, braga];

function work(id: string, title: string, placeIds: string[], date: string): Work {
  return { id, title, url: `/${id}/`, byline: [], places: placeIds, companions: [], date };
}

const portoSteps = work("porto-steps", "Porto Steps", ["places/porto"], "2024-05-20");
const portoTram = work("porto-tram", "Porto Tram", ["places/porto"], "2024-05-25");
const coimbraLibrary = work("coimbra-library", "Coimbra Library", ["places/coimbra"], "2024-05-01");
const bragaCathedral = work("braga-cathedral", "Braga Cathedral", ["places/braga"], "2024-04-01");
const works: Work[] = [portoSteps, portoTram, coimbraLibrary, bragaCathedral];

describe("deriveCrumbs", () => {
  test("root: just the 'all' crumb, not itself a widen target", () => {
    expect(deriveCrumbs({ kind: "all" }, places, null, null, strings, "en")).toEqual([
      { label: "All articles", target: null },
    ]);
  });

  test("a top-level place: 'all' widens, the place is terminal", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/portugal" }, places, null, null, strings, "en")).toEqual([
      { label: "All articles", target: { kind: "all" } },
      { label: "Portugal", target: null },
    ]);
  });

  test("a nested place: 'all' and the parent both widen, the child is terminal", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/porto" }, places, null, null, strings, "en")).toEqual([
      { label: "All articles", target: { kind: "all" } },
      { label: "Portugal", target: { kind: "place", id: "places/portugal" } },
      { label: "Porto", target: null },
    ]);
  });

  test("a selected work collapses the trail to 'all > This article', regardless of the place scope underneath it", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/porto" }, places, portoSteps, null, strings, "en")).toEqual([
      { label: "All articles", target: { kind: "all" } },
      { label: "This article", target: null },
    ]);
  });

  test("an open ring appends one more non-widenable crumb; the crumb before it keeps its own widen target", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/porto" }, places, null, 2, strings, "en")).toEqual([
      { label: "All articles", target: { kind: "all" } },
      { label: "Portugal", target: { kind: "place", id: "places/portugal" } },
      { label: "Porto", target: { kind: "place", id: "places/porto" } },
      { label: "2 works here", target: null },
    ]);
  });
});

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';
const VIEWPORT = { width: 1200, height: 800 };

function mount() {
  const figure = document.createElement("figure");
  document.body.append(figure);
  const controller = mountPlacesMap(figure, {
    worldSvgText: WORLD_SVG,
    tilesBaseUrl: "/_moss/map.abc/",
    tileCells: [],
    tileK: 4,
    tileBleed: 0.1,
    places: { works, places },
    lang: "en",
  });
  expect(controller).not.toBeNull();
  return figure;
}

function crumbButtons(figure: HTMLElement): HTMLButtonElement[] {
  return Array.from(figure.querySelectorAll<HTMLButtonElement>(".moss-places-chip-crumb")).filter(
    (el): el is HTMLButtonElement => el.tagName === "BUTTON",
  );
}

function crumbByText(figure: HTMLElement, text: string): HTMLButtonElement {
  const button = crumbButtons(figure).find((el) => el.textContent?.trim().startsWith(text));
  expect(button, `no crumb starting with "${text}"`).toBeTruthy();
  return button as HTMLButtonElement;
}

describe("the mounted chip", () => {
  beforeEach(() => {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      width: VIEWPORT.width, height: VIEWPORT.height, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, x: 0, y: 0, toJSON() {},
    } as DOMRect);
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
    history.replaceState(null, "", "/places/");
  });

  afterEach(() => {
    document.body.innerHTML = "";
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    history.replaceState(null, "", "/places/");
  });

  test("the root menu lists Portugal, and the dig-down menu sorts by count then name", () => {
    const figure = mount();
    const root = crumbByText(figure, "All articles");
    root.click();
    let items = Array.from(figure.querySelectorAll(".moss-places-chip-menu-item"));
    expect(items.map((item) => item.textContent)).toEqual(["Portugal(4)"]);

    items[0]!.click(); // narrows into Portugal
    const portugalCrumb = crumbByText(figure, "Portugal");
    portugalCrumb.click();
    items = Array.from(figure.querySelectorAll(".moss-places-chip-menu-item"));
    // Porto(2) first; Braga and Coimbra tie at 1, alphabetical after it.
    expect(items.map((item) => item.textContent)).toEqual(["Porto(2)", "Braga(1)", "Coimbra(1)"]);
  });

  test("narrowing from the menu updates the scope and the URL", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    const item = figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!;
    expect(item.textContent).toBe("Portugal(4)");
    item.click();

    expect(readUrlState().scope).toEqual({ kind: "place", id: "places/portugal" });
    expect(location.search).toContain("place=places%2Fportugal");
  });

  test("widening from an earlier crumb updates the scope and clears the URL's place param", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click();
    expect(readUrlState().scope).toEqual({ kind: "place", id: "places/portugal" });

    crumbByText(figure, "All articles").click();
    expect(readUrlState().scope).toEqual({ kind: "all" });
    expect(location.search).not.toContain("place=");
  });

  test("ArrowDown moves focus, Escape closes the menu and refocuses the crumb", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click(); // narrow into Portugal
    const trigger = crumbByText(figure, "Portugal");
    trigger.click(); // open Portugal's own dig-down menu
    const items = Array.from(figure.querySelectorAll<HTMLButtonElement>(".moss-places-chip-menu-item"));
    expect(document.activeElement).toBe(items[0]);

    items[0]!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(items[1]);

    (document.activeElement as HTMLElement).dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(figure.querySelector(".moss-places-chip-menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  test("an unrelated camera settle (a window resize) leaves an open menu and its focus untouched", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    const menuBefore = figure.querySelector(".moss-places-chip-menu");
    const focusedBefore = document.activeElement;
    expect(menuBefore).not.toBeNull();

    // map.ts's own window resize listener calls applyCamera(true), which
    // reaches scopeChip.render() with the exact same scope/selection/ring —
    // nothing here actually changed what the trail shows.
    window.dispatchEvent(new Event("resize"));

    expect(figure.querySelector(".moss-places-chip-menu")).toBe(menuBefore);
    expect(document.activeElement).toBe(focusedBefore);
  });

  test("choosing a menu item moves focus to the new trail's terminal crumb, not <body>", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click(); // narrows into Portugal
    const terminal = figure.querySelector(".moss-places-chip-crumb[data-terminal]");
    expect(document.activeElement).toBe(terminal);
  });

  test("choosing a menu item that narrows straight to a leaf place still moves focus onto it", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click(); // narrow into Portugal
    crumbByText(figure, "Portugal").click(); // open Portugal's own dig-down menu
    const items = Array.from(figure.querySelectorAll<HTMLButtonElement>(".moss-places-chip-menu-item"));
    const porto = items.find((item) => item.textContent?.startsWith("Porto"));
    expect(porto, "expected a Porto menu item").toBeTruthy();
    porto!.click(); // Porto has no children of its own — a leaf terminal crumb

    const terminal = figure.querySelector(".moss-places-chip-crumb[data-terminal]");
    expect(terminal?.tagName).toBe("SPAN");
    expect(document.activeElement).toBe(terminal);
  });

  test("the phone-width ellipsis is a real button that reveals the collapsed crumbs on activation", () => {
    const figure = mount();
    crumbByText(figure, "All articles").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click(); // narrow into Portugal
    crumbByText(figure, "Portugal").click();
    const items = Array.from(figure.querySelectorAll<HTMLButtonElement>(".moss-places-chip-menu-item"));
    items.find((item) => item.textContent?.startsWith("Porto"))!.click(); // 3-deep trail: All > Portugal > Porto

    const ellipsis = figure.querySelector<HTMLButtonElement>(".moss-places-chip-ellipsis")!;
    expect(ellipsis.tagName).toBe("BUTTON");
    expect(ellipsis.getAttribute("aria-label")).toBeTruthy();
    const collapsible = figure.querySelector(".moss-places-chip-collapsible")!;
    expect(ellipsis.getAttribute("aria-expanded")).toBe("false");
    expect(collapsible.hasAttribute("data-expanded")).toBe(false);

    ellipsis.click();
    expect(ellipsis.getAttribute("aria-expanded")).toBe("true");
    expect(collapsible.hasAttribute("data-expanded")).toBe(true);

    ellipsis.click();
    expect(ellipsis.getAttribute("aria-expanded")).toBe("false");
    expect(collapsible.hasAttribute("data-expanded")).toBe(false);
  });
});
