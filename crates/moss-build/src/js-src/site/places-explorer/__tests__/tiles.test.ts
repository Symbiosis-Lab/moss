/**
 * Tests for tiles.ts's pure geometry — the DOM/fetch half (`TileLayer`) is
 * covered by the render gates instead, the layer that can actually see a
 * fetched tile element positioned over a real, composited viewport (the
 * same split `markers.ts`'s own tests draw for `MarkerLayer`).
 */
import { describe, test, expect } from "vitest";
import { detailMaxZoom, MIN_ZOOM } from "../camera";
import { WORLD_WIDTH, WORLD_HEIGHT } from "../projection";
import { tileCellBounds, tileFadeOpacity, tileOverlayTransform, tilesForView } from "../tiles";

describe("tileCellBounds", () => {
  test("the world's own four corner cells sit at the canvas edges", () => {
    // x=0 is the westmost cell (-180..-170), y=0 the southmost (-90..-80).
    const southwest = tileCellBounds(0, 0);
    expect(southwest.minX).toBeCloseTo(0);
    expect(southwest.maxY).toBeCloseTo(WORLD_HEIGHT);
    // x=35 is the eastmost cell (170..180), y=17 the northmost (80..90).
    const northeast = tileCellBounds(35, 17);
    expect(northeast.maxX).toBeCloseTo(WORLD_WIDTH);
    expect(northeast.minY).toBeCloseTo(0);
  });

  test("adjacent cells share an edge with no gap or overlap", () => {
    const a = tileCellBounds(10, 5);
    const b = tileCellBounds(11, 5);
    expect(a.maxX).toBeCloseTo(b.minX);
  });
});

describe("tilesForView", () => {
  const cells: Array<[number, number]> = [
    [0, 0],
    [10, 5],
    [11, 5],
    [35, 17],
  ];

  test("a cover-zoom camera over the whole world sees every cell", () => {
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: 1 };
    const viewport = { width: 1200, height: 700 };
    const visible = tilesForView(cells, camera, viewport);
    expect(visible).toEqual(expect.arrayContaining(cells));
  });

  test("a tight zoom over one cell excludes a cell on the far side of the map", () => {
    const bounds = tileCellBounds(10, 5);
    const centerX = (bounds.minX + bounds.maxX) / 2;
    const centerY = (bounds.minY + bounds.maxY) / 2;
    const camera = { x: centerX, y: centerY, zoom: 20 };
    const viewport = { width: 800, height: 500 };
    const visible = tilesForView(cells, camera, viewport);
    expect(visible).toEqual(expect.arrayContaining([[10, 5]]));
    expect(visible).not.toEqual(expect.arrayContaining([[0, 0]]));
    expect(visible).not.toEqual(expect.arrayContaining([[35, 17]]));
  });

  test("an empty cell list selects nothing, however wide the view", () => {
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: 1 };
    const viewport = { width: 1200, height: 700 };
    expect(tilesForView([], camera, viewport)).toEqual([]);
  });
});

describe("tileFadeOpacity", () => {
  const viewport = { width: 1200, height: 700 };

  test("is zero at the world's own cover zoom — tiles are absent at rest", () => {
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: MIN_ZOOM };
    expect(tileFadeOpacity(camera, viewport)).toBe(0);
  });

  test("reaches full opacity exactly at the world's own detail ceiling — tiles are present past it", () => {
    const ceiling = detailMaxZoom(viewport);
    const camera = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: ceiling };
    expect(tileFadeOpacity(camera, viewport)).toBe(1);
  });

  test("ramps linearly across the fade band and stays zero short of it", () => {
    const ceiling = detailMaxZoom(viewport);
    const fadeStart = ceiling * 0.8; // the last 20% of the zoom range
    const justShort = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: fadeStart - 0.01 };
    const midBand = { x: WORLD_WIDTH / 2, y: WORLD_HEIGHT / 2, zoom: (fadeStart + ceiling) / 2 };
    expect(tileFadeOpacity(justShort, viewport)).toBe(0);
    expect(tileFadeOpacity(midBand, viewport)).toBeCloseTo(0.5);
  });
});

describe("tileOverlayTransform", () => {
  // A tile's own native SVG size, as `explorer::emit_tile_svg` actually
  // emits it (`Projection::canvas_size`, backed by
  // `PattersonProjection::for_tile`): its cell's world-unit rectangle,
  // padded by `bleed` on every edge, times `k`. Re-derived here from
  // `tileCellBounds` rather than imported, so this test can check the
  // geometric CONTRACT (a rendered-and-scaled tile covers its own cell plus
  // a deliberate overlap margin) rather than restate the production
  // formula under a different name.
  function nativeTileSize(x: number, y: number, k: number, bleed: number): { width: number; height: number } {
    const bounds = tileCellBounds(x, y);
    return { width: (bounds.maxX - bounds.minX + 2 * bleed) * k, height: (bounds.maxY - bounds.minY + 2 * bleed) * k };
  }

  test("maps a cell to its own rectangle padded by bleed, at any unitScale/k", () => {
    const unitScale = 2.3;
    const k = 4;
    const bleed = 0.1;
    for (const [x, y] of [[10, 5], [0, 0], [35, 17], [20, 14]] as const) {
      const bounds = tileCellBounds(x, y);
      const { width: nativeWidth, height: nativeHeight } = nativeTileSize(x, y, k, bleed);
      const { translateX, translateY, scale } = tileOverlayTransform(x, y, unitScale, k, bleed);

      // Top-left corner lands bleed world units outside the cell's own
      // origin, not exactly on it.
      expect(translateX).toBeCloseTo((bounds.minX - bleed) * unitScale);
      expect(translateY).toBeCloseTo((bounds.minY - bleed) * unitScale);
      // Rendered (native size * scale) equals the cell's own rectangle
      // padded by bleed on every edge, in the SAME unitScale px space the
      // world layer's internal layout uses — not the tile's own larger,
      // k-magnified canvas.
      expect(nativeWidth * scale).toBeCloseTo((bounds.maxX - bounds.minX + 2 * bleed) * unitScale);
      expect(nativeHeight * scale).toBeCloseTo((bounds.maxY - bounds.minY + 2 * bleed) * unitScale);
    }
  });

  test("adjacent tiles overlap by exactly 2 * bleed at their shared edge, at any k", () => {
    const unitScale = 1.7;
    const bleed = 0.1;
    for (const k of [1, 4, 10]) {
      const a = tileOverlayTransform(10, 5, unitScale, k, bleed);
      const { width: nativeWidthA } = nativeTileSize(10, 5, k, bleed);
      const b = tileOverlayTransform(11, 5, unitScale, k, bleed);
      const aRightEdge = a.translateX + nativeWidthA * a.scale;
      // A's own right edge lands PAST b's own left edge by 2 * bleed world
      // units (in unitScale px) — an overlap, not a meet — so a sub-pixel
      // rounding disagreement between the two leaves overlap, not a gap.
      expect(aRightEdge - b.translateX).toBeCloseTo(2 * bleed * unitScale);
      expect(aRightEdge).toBeGreaterThan(b.translateX);
    }
  });
});
