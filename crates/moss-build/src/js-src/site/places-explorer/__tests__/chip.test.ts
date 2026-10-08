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
  return { id, title, url: `/${id}/`, byline: [], authors: [], places: placeIds, companions: [], date };
}

const portoSteps = work("porto-steps", "Porto Steps", ["places/porto"], "2024-05-20");
const portoTram = work("porto-tram", "Porto Tram", ["places/porto"], "2024-05-25");
const coimbraLibrary = work("coimbra-library", "Coimbra Library", ["places/coimbra"], "2024-05-01");
const bragaCathedral = work("braga-cathedral", "Braga Cathedral", ["places/braga"], "2024-04-01");
const works: Work[] = [portoSteps, portoTram, coimbraLibrary, bragaCathedral];

describe("deriveCrumbs", () => {
  test("the root crumb of an article's scope switch names articles, not places", () => {
    expect(deriveCrumbs({ kind: "all" }, places, null, strings, "en", true)[0].label).toBe("All articles");
  });

  test("root: just the 'all' crumb, not itself a widen target", () => {
    expect(deriveCrumbs({ kind: "all" }, places, null, strings, "en")).toEqual([
      { label: "All places", target: null },
    ]);
  });

  test("a top-level place: 'all' widens, the place is terminal", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/portugal" }, places, null, strings, "en")).toEqual([
      { label: "All places", target: { kind: "all" } },
      { label: "Portugal", target: null },
    ]);
  });

  test("a nested place: 'all' and the parent both widen, the child is terminal", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/porto" }, places, null, strings, "en")).toEqual([
      { label: "All places", target: { kind: "all" } },
      { label: "Portugal", target: { kind: "place", id: "places/portugal" } },
      { label: "Porto", target: null },
    ]);
  });

  test("an open ring appends one more non-widenable crumb; the crumb before it keeps its own widen target", () => {
    expect(deriveCrumbs({ kind: "place", id: "places/porto" }, places, 2, strings, "en")).toEqual([
      { label: "All places", target: { kind: "all" } },
      { label: "Portugal", target: { kind: "place", id: "places/portugal" } },
      { label: "Porto", target: { kind: "place", id: "places/porto" } },
      { label: "2 works here", target: null },
    ]);
  });
});

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';
const VIEWPORT = { width: 1200, height: 800 };

function mount(articlePlaces: { works: Work[]; places: Place[] } = { works, places }, embedArticle: string | null = null) {
  const figure = document.createElement("figure");
  document.body.append(figure);
  const controller = mountPlacesMap(figure, {
    worldSvgText: WORLD_SVG,
    tilesBaseUrl: "/_moss/map.abc/",
    tileCells: [],
    tileK: 4,
    tileOrigins: {}, tileColumns: 36, tileRows: 18,
    places: articlePlaces,
    lang: "en",
  });
  expect(controller).not.toBeNull();
  if (embedArticle) {
    // What `attachEmbedModeIfRequested` does for `?article=…&embed=1`.
    controller!.setScope({ kind: "article", id: embedArticle });
    controller!.setCurrentArticle(embedArticle);
  }
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
    const root = crumbByText(figure, "All places");
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
    crumbByText(figure, "All places").click();
    const item = figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!;
    expect(item.textContent).toBe("Portugal(4)");
    item.click();

    expect(readUrlState().scope).toEqual({ kind: "place", id: "places/portugal" });
    expect(location.search).toContain("place=places%2Fportugal");
  });

  test("widening from an earlier crumb updates the scope and clears the URL's place param", () => {
    const figure = mount();
    crumbByText(figure, "All places").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click();
    expect(readUrlState().scope).toEqual({ kind: "place", id: "places/portugal" });

    crumbByText(figure, "All places").click();
    expect(readUrlState().scope).toEqual({ kind: "all" });
    expect(location.search).not.toContain("place=");
  });

  test("ArrowDown moves focus, Escape closes the menu and refocuses the crumb", () => {
    const figure = mount();
    crumbByText(figure, "All places").click();
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
    crumbByText(figure, "All places").click();
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
    crumbByText(figure, "All places").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click(); // narrows into Portugal
    const terminal = figure.querySelector(".moss-places-chip-crumb[data-terminal]");
    expect(document.activeElement).toBe(terminal);
  });

  test("choosing a menu item that narrows straight to a leaf place still moves focus onto it", () => {
    const figure = mount();
    crumbByText(figure, "All places").click();
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
    crumbByText(figure, "All places").click();
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

describe("the article scope switch", () => {
  // Two places an ocean apart, so the one article cannot cluster into a single marker.
  const kyoto: Place = { id: "places/kyoto", name: "Kyoto", precision: "city", lat: 35, lng: 135.7 };
  const twoPlaces = work("two-places", "Two Places", ["places/porto", "places/kyoto"], "2024-06-01");
  const data = { works: [...works, twoPlaces], places: [...places, kyoto] };

  beforeEach(() => {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      width: VIEWPORT.width, height: VIEWPORT.height, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, x: 0, y: 0, toJSON() {},
    } as DOMRect);
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in tests"))));
    history.replaceState(null, "", "/places/?article=two-places&embed=1");
  });
  afterEach(() => {
    document.body.innerHTML = "";
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    history.replaceState(null, "", "/places/");
  });

  const option = (figure: HTMLElement, which: "article" | "all") =>
    figure.querySelector<HTMLButtonElement>(`.moss-places-chip-scope-option[data-scope="${which}"]`)!;
  const markerCount = (figure: HTMLElement) => figure.querySelectorAll(".moss-places-marker").length;

  test("the root crumb is a labelled two-segment group with 'This article' current, and no place trail yet", () => {
    const figure = mount(data, "two-places");
    const group = figure.querySelector(".moss-places-chip-scope")!;
    expect(group.getAttribute("role")).toBe("group");
    expect(group.getAttribute("aria-label")).toBe("Which articles to show");
    expect(option(figure, "article").getAttribute("aria-current")).toBe("true");
    expect(option(figure, "all").hasAttribute("aria-current")).toBe(false);
    expect(figure.querySelector(".moss-places-chip-sep")).toBeNull();
    expect(markerCount(figure)).toBe(2); // one per place of the article
  });

  test("the switch's root segment says 'All articles' in both of its states, while the plain root crumb says 'All places'", () => {
    const figure = mount(data, "two-places");
    expect(option(figure, "all").textContent).toBe("All articles");
    option(figure, "all").click();
    expect(option(figure, "all").textContent).toBe("All articles");
  });

  test("All articles shows every article with the place menu on its chevron; This article returns, and focus stays on the pressed segment", () => {
    const figure = mount(data, "two-places");
    option(figure, "all").click();
    expect(document.activeElement).toBe(option(figure, "all"));
    expect(option(figure, "all").getAttribute("aria-current")).toBe("true");
    expect(option(figure, "all").getAttribute("aria-haspopup")).toBe("menu");
    expect(figure.querySelector('[data-current="true"]')).not.toBeNull();
    expect(figure.querySelector(".moss-places-status")!.textContent).toBe("Showing all articles.");
    expect(figure.querySelectorAll("[data-work-id]")).not.toHaveLength(0);
    expect(figure.querySelector('[data-work-id="two-places"]')).toBeNull(); // never its own card

    option(figure, "article").click();
    expect(document.activeElement).toBe(option(figure, "article"));
    expect(option(figure, "article").getAttribute("aria-current")).toBe("true");
    expect(markerCount(figure)).toBe(2);
    expect(figure.querySelector(".moss-places-status")!.textContent).toBe("Showing only this article.");
    expect(location.search).toContain("article=two-places"); // the embed's identity survives the round trip
  });

  test("leaving All articles remembers its place scope and camera, and This article → All articles restores both", () => {
    const figure = mount(data, "two-places");
    option(figure, "all").click();
    option(figure, "all").click(); // opens the place menu
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click(); // into Portugal
    const before = readUrlState();
    expect(before.scope).toEqual({ kind: "place", id: "places/portugal" });

    option(figure, "article").click();
    expect(readUrlState().scope).toEqual({ kind: "all" });
    expect(readUrlState().camera!.zoom).not.toBeCloseTo(before.camera!.zoom, 1);

    option(figure, "all").click();
    const after = readUrlState();
    expect(after.scope).toEqual({ kind: "place", id: "places/portugal" });
    expect(after.camera!.x).toBeCloseTo(before.camera!.x, 1);
    expect(after.camera!.y).toBeCloseTo(before.camera!.y, 1);
    expect(after.camera!.zoom).toBeCloseTo(before.camera!.zoom, 1);
    expect(figure.querySelector(".moss-places-chip-crumb[data-terminal]")!.textContent).toBe("Portugal");
  });

  test("inside All articles, the checked segment widens back from a place scope like the root crumb did", () => {
    const figure = mount(data, "two-places");
    option(figure, "all").click();
    option(figure, "all").click();
    figure.querySelector<HTMLButtonElement>(".moss-places-chip-menu-item")!.click();
    option(figure, "all").click();
    expect(readUrlState().scope).toEqual({ kind: "all" });
    expect(option(figure, "all").getAttribute("aria-current")).toBe("true");
  });

  test("a selected card on a plain page gets no switch and no 'This article' crumb", () => {
    history.replaceState(null, "", "/places/?article=porto-steps");
    const figure = mount(data);
    expect(figure.querySelector(".moss-places-chip-scope")).toBeNull();
    expect(figure.textContent).not.toContain("This article");
    expect(figure.querySelector(".moss-places-chip")!.textContent).toContain("All places");
  });

  test("a site with one article offers no switch", () => {
    const only = { works: [twoPlaces], places: data.places };
    const figure = mount(only, "two-places");
    expect(figure.querySelector(".moss-places-chip-scope")).toBeNull();
  });
});
