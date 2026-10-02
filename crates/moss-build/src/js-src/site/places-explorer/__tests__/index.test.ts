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
const LABELS_JSON = { languages: ["en"], en: { cities: [], ranges: [], peaks: [], rivers: [] } };

function handshakeFigure(withLabels = false): string {
  const labelsAttr = withLabels ? ' data-labels="/_moss/map.abc/labels.json"' : "";
  return (
    '<figure class="moss-place-map" data-moss-places-explorer ' +
    'data-world="/_moss/map.abc/world.svg" data-tiles="/_moss/map.abc/tiles.json" ' +
    `data-places="/_moss/places.def.json"${labelsAttr} data-scope="places">` +
    '<svg data-static-floor aria-hidden="true"></svg></figure>'
  );
}

function stubSuccessfulFetch(placesJson: unknown = PLACES_JSON, labelsOutcome: "ok" | "fail" = "ok"): void {
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("world.svg")) return Promise.resolve({ ok: true, text: () => Promise.resolve(WORLD_SVG) } as Response);
      if (url.endsWith("tiles.json")) return Promise.resolve({ ok: true, json: () => Promise.resolve(TILES_JSON) } as Response);
      if (url.endsWith("labels.json")) {
        if (labelsOutcome === "fail") return Promise.resolve({ ok: false, status: 404 } as Response);
        return Promise.resolve({ ok: true, json: () => Promise.resolve(LABELS_JSON) } as Response);
      }
      if (url.endsWith(".json")) return Promise.resolve({ ok: true, json: () => Promise.resolve(placesJson) } as Response);
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

  test("a handshake with no data-labels attribute boots the map without ever fetching labels", async () => {
    document.body.innerHTML = handshakeFigure(false);
    stubSuccessfulFetch();
    const figure = document.querySelector<HTMLElement>("[data-moss-places-explorer]")!;
    await initPlacesExplorer();
    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("ready");
    expect(fetch).not.toHaveBeenCalledWith(expect.stringContaining("labels.json"), expect.anything());
  });

  test("a failed labels.json fetch never blocks the map from becoming ready", async () => {
    document.body.innerHTML = handshakeFigure(true);
    stubSuccessfulFetch(PLACES_JSON, "fail");
    const figure = document.querySelector<HTMLElement>("[data-moss-places-explorer]")!;
    await initPlacesExplorer();
    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("ready");
    expect(figure.querySelector(".moss-places-viewport")).not.toBeNull();
  });

  test("a present data-labels attribute is fetched alongside the required three", async () => {
    document.body.innerHTML = handshakeFigure(true);
    stubSuccessfulFetch(PLACES_JSON, "ok");
    await initPlacesExplorer();
    expect(fetch).toHaveBeenCalledWith(expect.stringContaining("labels.json"), expect.anything());
  });

  test("a work missing byline (and every other skip_serializing_if field) still reaches ready — the wire can omit them, not send null", async () => {
    document.body.innerHTML = handshakeFigure();
    // Exactly what `places_data.rs` serializes for a work with no byline, no
    // date, no description and no cover: those keys are absent, not `null`
    // or `[]` — `#[serde(skip_serializing_if = ...)]` on each one.
    stubSuccessfulFetch({
      works: [{ id: "/sparse/", title: "Sparse", url: "/sparse/", places: ["places/kyoto"], companions: [] }],
      places: [{ id: "places/kyoto", name: "Kyoto", precision: "city", lat: 35.01, lng: 135.77 }],
    });
    const figure = document.querySelector<HTMLElement>("[data-moss-places-explorer]")!;

    await expect(initPlacesExplorer()).resolves.toBeUndefined();

    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("ready");
  });

  test("a second call on an already-mounting figure is a no-op — WebKit has been seen firing DOMContentLoaded twice for an iframe-loaded copy of this page", async () => {
    document.body.innerHTML = handshakeFigure();
    stubSuccessfulFetch();
    const first = initPlacesExplorer();
    const second = initPlacesExplorer(); // fired before the first call's fetch even resolves
    await Promise.all([first, second]);
    expect(fetch).toHaveBeenCalledTimes(3); // world + places + tiles, exactly once (no labels attr on this handshake)
    const figure = document.querySelector<HTMLElement>("[data-moss-places-explorer]")!;
    expect(figure.getAttribute("data-moss-places-explorer-ready")).toBe("ready");
    expect(figure.querySelectorAll(".moss-places-viewport")).toHaveLength(1);
  });

  test("fetches every handshake URL with force-cache — each one is content-hashed, so a cached response is always correct to reuse", async () => {
    document.body.innerHTML = handshakeFigure();
    stubSuccessfulFetch();
    await initPlacesExplorer();
    const calls = (fetch as ReturnType<typeof vi.fn>).mock.calls;
    expect(calls.length).toBeGreaterThan(0);
    for (const [, init] of calls) {
      expect((init as RequestInit).cache).toBe("force-cache");
    }
  });
});
