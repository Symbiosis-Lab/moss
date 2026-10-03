/**
 * Tests for raster.ts's pure/DOM-manipulation half: splitting a fetched map
 * SVG into a bakeable base and a live rivers overlay, and the theme-token
 * capture that stands in for the page cascade a rasterised resource cannot
 * read. `rasterize` itself (the actual `Blob`/`img.decode()` round trip) is
 * covered by the render gates instead — jsdom implements neither API, which
 * is also exactly the path `rasterizeOrFallback` exists to degrade through,
 * so that fallback is what gets exercised here.
 */
import { afterEach, describe, expect, test } from "vitest";
import {
  capturePlaceMapTheme,
  createRasterImage,
  rasterizeOrFallback,
  splitMapSvg,
  withRootSize,
} from "../raster";

const SAMPLE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="720" height="480" viewBox="0 0 720 480" role="img" aria-label="A map">
  <g data-map-layer="water"><rect width="720" height="480" fill="var(--moss-place-water, #dbe7ea)"/></g>
  <g data-map-layer="relief"><g data-map-band="1" style="fill:#000"><path d="M0 0"/></g></g>
  <g data-map-layer="lighting"><g opacity="0.5"><path d="M0 0"/></g></g>
  <g data-map-layer="rivers"><path d="M0 0L10 10" stroke-width="1.60" fill="none" stroke="var(--moss-place-rivers, #7fa6bd)"/></g>
</svg>`;

const SAMPLE_SVG_NO_RIVERS = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200" viewBox="0 0 300 200">
  <g data-map-layer="water"><rect width="300" height="200"/></g>
  <g data-map-layer="rivers"></g>
</svg>`;

afterEach(() => {
  // capturePlaceMapTheme reads document.documentElement's own computed
  // style — never leak one test's tokens into the next.
  document.documentElement.removeAttribute("style");
  document.body.replaceChildren();
});

describe("splitMapSvg", () => {
  test("null on markup that isn't a real svg", () => {
    expect(splitMapSvg("<html><body>not a map</body></html>")).toBeNull();
    expect(splitMapSvg("<<<not xml at all")).toBeNull();
  });

  test("reads the source's own canvas size", () => {
    const split = splitMapSvg(SAMPLE_SVG)!;
    expect(split.width).toBe(720);
    expect(split.height).toBe(480);
  });

  test("extracts a non-empty rivers group into its own live overlay, copying stroke-width onto --river-w", () => {
    const split = splitMapSvg(SAMPLE_SVG)!;
    expect(split.rivers).not.toBeNull();
    expect(split.rivers!.getAttribute("viewBox")).toBe("0 0 720 480");
    const path = split.rivers!.querySelector("path")!;
    expect(path.style.getPropertyValue("--river-w")).toBe("1.60");
    // Removed from the base that will be rasterised — a filtered resource
    // showing rivers twice (once baked, once as the live overlay) is
    // exactly the bug this split exists to avoid.
    expect(split.base.querySelector('[data-map-layer="rivers"]')).toBeNull();
  });

  test("an empty rivers group yields no overlay, and is still dropped from the base", () => {
    const split = splitMapSvg(SAMPLE_SVG_NO_RIVERS)!;
    expect(split.rivers).toBeNull();
    expect(split.base.querySelector('[data-map-layer="rivers"]')).toBeNull();
  });

  test("leaves relief and lighting at full strength: a tile's bands are tinted for the world's own, not dimmed", () => {
    const split = splitMapSvg(SAMPLE_SVG)!;
    for (const layer of ["relief", "lighting"]) {
      const group = split.base.querySelector(`[data-map-layer="${layer}"]`) as SVGElement;
      expect(group.style.getPropertyValue("opacity")).toBe("");
    }
  });

  test("strips the accessible-label attributes a rasterised duplicate must not repeat", () => {
    const split = splitMapSvg(SAMPLE_SVG)!;
    expect(split.base.hasAttribute("role")).toBe(false);
    expect(split.base.hasAttribute("aria-label")).toBe(false);
  });

  test("inlines the current theme's tokens onto the base's own root style", () => {
    document.documentElement.style.setProperty("--moss-place-water", "#123456");
    const split = splitMapSvg(SAMPLE_SVG)!;
    expect(split.base.getAttribute("style")).toContain("--moss-place-water:#123456");
  });
});

describe("capturePlaceMapTheme", () => {
  test("includes a token that's actually set, formatted as name:value", () => {
    document.documentElement.style.setProperty("--moss-place-land", "#abcdef");
    expect(capturePlaceMapTheme()).toContain("--moss-place-land:#abcdef");
  });

  test("reads the tokens off the map figure, where site.css declares them, not the root", () => {
    const figure = document.createElement("figure");
    figure.className = "moss-place-map";
    figure.style.setProperty("--moss-place-water", "#112233");
    document.body.append(figure);
    document.documentElement.style.setProperty("--moss-place-water", "#999999");
    const split = splitMapSvg(SAMPLE_SVG, 1)!;
    expect(split.base.getAttribute("style")).toContain("--moss-place-water:#112233");
    expect(capturePlaceMapTheme()).not.toContain("#999999");
  });

  test("omits a token nothing has set, rather than emitting an empty declaration", () => {
    const theme = capturePlaceMapTheme();
    expect(theme).not.toMatch(/--moss-place-water:(;|$)/);
  });
});

describe("withRootSize", () => {
  test("replaces only the root svg's own width/height, never the viewBox or a descendant's", () => {
    const markup = '<svg xmlns="http://www.w3.org/2000/svg" width="720" height="480" viewBox="0 0 720 480"><rect width="720" height="480"/></svg>';
    const sized = withRootSize(markup, 1440.4, 960.6);
    expect(sized).toBe('<svg xmlns="http://www.w3.org/2000/svg" width="1440" height="961" viewBox="0 0 720 480"><rect width="720" height="480"/></svg>');
  });

  test("floors a sub-pixel request at 1, never 0 or negative", () => {
    const markup = '<svg xmlns="http://www.w3.org/2000/svg" width="720" height="480" viewBox="0 0 720 480"></svg>';
    expect(withRootSize(markup, 0, -5)).toContain('width="1" height="1"');
  });
});

describe("splitMapSvg's cached baseMarkup", () => {
  test("is the base's own serialised markup, carrying its baked width/height and style", () => {
    const split = splitMapSvg(SAMPLE_SVG)!;
    expect(split.baseMarkup).toContain('width="720"');
    expect(split.baseMarkup).toContain('height="480"');
    expect(split.baseMarkup).not.toContain('data-map-layer="rivers"');
  });
});

describe("createRasterImage", () => {
  test("carries an empty alt — every raster sits inside the map's own single labelled region (viewportEl)", () => {
    expect(createRasterImage().alt).toBe("");
  });
});

describe("rasterizeOrFallback", () => {
  test("falls back to the live fallback element when Blob/decode aren't available (jsdom, same as this suite)", async () => {
    const split = splitMapSvg(SAMPLE_SVG)!;
    const surface = await rasterizeOrFallback(split.baseMarkup, split.base, 100, 100);
    expect(surface.el).toBe(split.base);
    expect(() => surface.release()).not.toThrow();
  });
});
