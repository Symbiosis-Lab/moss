/**
 * Tests for index.ts's boot sequence: the handshake read, the fetch, and
 * the one-shot swap from the static figure to the interactive layer. The
 * interactive layer's own rendering (markers, cards, gestures) is covered
 * at the unit level by its own modules' tests, and in a real browser by
 * the render gates — this file only has to prove the swap happens, and
 * that a fetch failure or a bad handshake never throws and never touches
 * the static figure.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { initPlacesExplorer } from "../index";

const WORLD_SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 842.035025 480"></svg>';
const PLACES_JSON = { works: [], places: [] };
const TILES_JSON = { k: 4, bleed: 0.1, cells: [] as Array<[number, number]> };

function handshakeFigure(): string {
  return (
    '<figure class="moss-place-map" data-moss-places-explorer ' +
    'data-world="/_moss/map.abc/world.svg" data-tiles="/_moss/map.abc/tiles.json" ' +
    'data-places="/_moss/places.def.json" data-scope="places">' +
    '<svg data-static-floor aria-hidden="true"></svg></figure>'
  );
}

function stubSuccessfulFetch(): void {
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("world.svg")) return Promise.resolve({ ok: true, text: () => Promise.resolve(WORLD_SVG) } as Response);
      if (url.endsWith("tiles.json")) return Promise.resolve({ ok: true, json: () => Promise.resolve(TILES_JSON) } as Response);
      if (url.endsWith(".json")) return Promise.resolve({ ok: true, json: () => Promise.resolve(PLACES_JSON) } as Response);
      return Promise.reject(new Error(`unexpected fetch: ${url}`));
    }),
  );
}

beforeEach(() => {
  // A real, non-zero layout so mountPlacesMap's render pipeline actually
  // runs its full body instead of early-returning on a 0x0 viewport.
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width: 800, height: 500, top: 0, left: 0, right: 800, bottom: 500, x: 0, y: 0, toJSON() {},
  } as DOMRect);
});

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("initPlacesExplorer", () => {
  test("does nothing when no handshake element is on the page", async () => {
    document.body.innerHTML = "<p>no map here</p>";
    stubSuccessfulFetch();
    await expect(initPlacesExplorer()).resolves.toBeUndefined();
    expect(fetch).not.toHaveBeenCalled();
    expect(document.querySelector("[data-moss-places-explorer-ready]")).toBeNull();
  });

  test("marks pending, then swaps in the interactive layer and marks ready once both fetches resolve", async () => {
    document.body.innerHTML = handshakeFigure();
    stubSuccessfulFetch();
    const figure = document.querySelector<HTMLElement>("[data-moss-places-explorer]")!;

    const pending = initPlacesExplorer();
    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("pending");
    await pending;

    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("ready");
    expect(figure.querySelector("[data-static-floor]")).toBeNull();
    expect(figure.querySelector(".moss-places-viewport")).not.toBeNull();
  });

  test("a failed fetch leaves the static figure untouched and never throws", async () => {
    document.body.innerHTML = handshakeFigure();
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("network down"))));
    const figure = document.querySelector<HTMLElement>("[data-moss-places-explorer]")!;

    await expect(initPlacesExplorer()).resolves.toBeUndefined();

    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("pending");
    expect(figure.querySelector("[data-static-floor]")).not.toBeNull();
    expect(figure.querySelector(".moss-places-viewport")).toBeNull();
  });

  test("a handshake missing one of the three data URLs is a no-op, not a partial fetch", async () => {
    document.body.innerHTML =
      '<figure class="moss-place-map" data-moss-places-explorer data-world="/w.svg"></figure>';
    stubSuccessfulFetch();
    await initPlacesExplorer();
    expect(fetch).not.toHaveBeenCalled();
  });
});
