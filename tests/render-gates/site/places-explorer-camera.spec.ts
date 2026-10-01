/**
 * Render gate: cover camera, the zoom round-trip crispness fix, and the
 * zoom capsule/reset control shapes.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-camera.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function gotoReady(page: Page): Promise<void> {
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
}

/**
 * RGBA at a grid of sample points inside a screenshot PNG, decoded by the
 * BROWSER itself (an <img> + <canvas>, both native) rather than a Node-side
 * PNG library this repo doesn't otherwise depend on. Comparing the raw
 * pixels this way — not the PNG-encoded bytes — matters: PNG's own
 * scanline filters (Paeth/Up/Sub) cascade a single differing source pixel
 * into a largely different COMPRESSED byte stream, so a byte-for-byte
 * buffer comparison was measured failing even between two screenshots of
 * one genuinely unchanged frame, taken back to back with no interaction at
 * all — the wrong metric for "did the pixels actually change", not
 * evidence of a real difference.
 */
async function samplePixels(page: Page, png: Buffer, width: number, height: number): Promise<number[][]> {
  const points = [];
  for (let gx = 1; gx < 4; gx++) {
    for (let gy = 1; gy < 4; gy++) {
      points.push({ x: Math.round((width * gx) / 4), y: Math.round((height * gy) / 4) });
    }
  }
  return samplePixelsAt(page, png, points);
}

/** Like {@link samplePixels}, at caller-chosen points rather than a fixed 3x3 grid — for sampling along a specific line (a tile boundary) instead of across the whole clip. */
async function samplePixelsAt(page: Page, png: Buffer, points: Array<{ x: number; y: number }>): Promise<number[][]> {
  const dataUrl = `data:image/png;base64,${png.toString("base64")}`;
  return page.evaluate(
    async ({ dataUrl, points }) => {
      const img = new Image();
      // `onload`, not `img.decode()`: WebKit's own `decode()` on a `data:`
      // URL was measured hanging past this test's timeout for a clip whose
      // dimensions `decode()` handled fine moments earlier in the SAME
      // suite (`samplePixels`'s own fixed 3x3-grid call) — `onload` is the
      // older, more broadly reliable readiness signal and costs nothing
      // here, since nothing after this point needs the decode-vs-paint
      // ordering guarantee `decode()` exists for.
      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve();
        img.onerror = () => reject(new Error("image failed to load"));
        img.src = dataUrl;
      });
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext("2d")!;
      ctx.drawImage(img, 0, 0);
      return points.map((p) => Array.from(ctx.getImageData(Math.round(p.x), Math.round(p.y), 1, 1).data));
    },
    { dataUrl, points },
  );
}

/** The Patterson (2014) cylindrical projection `projection.ts` implements, reimplemented here from the published polynomial rather than imported — the cross-check that module's own doc describes, so a drift between the build's runtime and this gate would fail loudly instead of cancelling out. */
function pattersonProject(latitude: number, longitude: number): { x: number; y: number } {
  const K1 = 1.0148;
  const K2 = 0.23185;
  const K3 = -0.14499;
  const K4 = 0.02406;
  const pattersonY = (phi: number): number => {
    const phi2 = phi * phi;
    const phi4 = phi2 * phi2;
    return phi * (K1 + phi4 * (K2 + phi2 * (K3 + K4 * phi2)));
  };
  const HEIGHT = 480;
  const yAtPole = pattersonY(Math.PI / 2);
  const scale = HEIGHT / 2 / yAtPole;
  const width = (HEIGHT * Math.PI) / yAtPole;
  return {
    x: width / 2 + scale * ((longitude * Math.PI) / 180),
    y: HEIGHT / 2 - scale * pattersonY((latitude * Math.PI) / 180),
  };
}

for (const [label, size] of [
  ["16:9", { width: 1280, height: 720 }],
  ["9:16", { width: 540, height: 960 }],
] as const) {
  test(`cover leaves no empty band at ${label}`, async ({ page }) => {
    await page.setViewportSize(size);
    await gotoReady(page);
    const viewportBox = (await page.locator(".moss-places-viewport").boundingBox())!;
    const worldBox = (await page.locator(".moss-places-world").boundingBox())!;
    // The world layer's own box must cover the viewport's box on every
    // edge — "no empty band" is exactly the world never falling short of
    // the frame it fills, on either axis.
    expect(worldBox.x).toBeLessThanOrEqual(viewportBox.x + 1);
    expect(worldBox.y).toBeLessThanOrEqual(viewportBox.y + 1);
    expect(worldBox.x + worldBox.width).toBeGreaterThanOrEqual(viewportBox.x + viewportBox.width - 1);
    expect(worldBox.y + worldBox.height).toBeGreaterThanOrEqual(viewportBox.y + viewportBox.height - 1);
  });
}

test("zooming in and resetting back to cover leaves the map crisp at the coast", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page);
  const viewport = page.locator(".moss-places-viewport");
  const box = (await viewport.boundingBox())!;
  const clip = {
    x: Math.round(box.x + box.width / 2 - 40),
    y: Math.round(box.y + box.height / 2 - 40),
    width: 80,
    height: 80,
  };
  await page.waitForTimeout(150);
  const before = await page.screenshot({ clip });
  const transformBefore = await page.locator(".moss-places-world").evaluate((el) => (el as HTMLElement).style.transform);

  // A real pinch — not the zoom capsule — is what exercises the bug: the
  // capsule's own click handler always calls applyCamera(true) directly
  // (never mid-gesture), so it never promotes the world layer to its own
  // compositing layer in the first place and could not catch a regression
  // here. A pinch spans many real pointermove frames between pointerdown
  // and pointerup, each one unsettled (data-gesture present, will-change
  // active) until the final settle — the actual window the blur bug lived
  // in. Symmetric spread-then-pinch (10..100..10px half-distance) returns
  // the same distance ratio product (telescoping to 1), so the camera lands
  // back on the same zoom without any explicit reset.
  await page.locator(".moss-places-viewport").evaluate((el) => {
    const rect = el.getBoundingClientRect();
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    const fire = (type: string, id: number, x: number, y: number) =>
      el.dispatchEvent(new PointerEvent(type, { pointerId: id, clientX: x, clientY: y, bubbles: true, cancelable: true, pointerType: "touch" }));
    fire("pointerdown", 1, cx - 10, cy);
    fire("pointerdown", 2, cx + 10, cy);
    for (let d = 20; d <= 100; d += 10) {
      fire("pointermove", 1, cx - d, cy);
      fire("pointermove", 2, cx + d, cy);
    }
    for (let d = 90; d >= 10; d -= 10) {
      fire("pointermove", 1, cx - d, cy);
      fire("pointermove", 2, cx + d, cy);
    }
    fire("pointerup", 1, cx - 10, cy);
    fire("pointerup", 2, cx + 10, cy);
  });
  await page.waitForTimeout(150);

  const stillCompositing = await page.locator(".moss-places-world").evaluate((el) => el.hasAttribute("data-gesture"));
  expect(stillCompositing, "the world layer must demote out of compositing once the camera settles").toBe(false);

  // The camera itself must also land back on very nearly the same
  // transform — belt and braces alongside the pixel sample below: a
  // genuinely blurred re-render would still show the same transform (blur
  // is a rasterisation artefact, not a camera-math one), so this alone
  // could not catch the bug the pixel sample exists for; it rules out the
  // OTHER way this assertion could pass for the wrong reason, a pinch that
  // quietly lands on a different camera. A small tolerance, not exact
  // string equality: the symmetric spread-then-pinch returns the same
  // distance-ratio PRODUCT in exact arithmetic, not bit-identical
  // floating-point through ~16 multiply steps.
  const numbers = (s: string) => [...s.matchAll(/-?\d+\.?\d*/g)].map((m) => Number(m[0]));
  const transformAfter = await page.locator(".moss-places-world").evaluate((el) => (el as HTMLElement).style.transform);
  const beforeNums = numbers(transformBefore);
  const afterNums = numbers(transformAfter);
  expect(afterNums.length).toBe(beforeNums.length);
  for (let i = 0; i < beforeNums.length; i++) {
    expect(Math.abs(afterNums[i] - beforeNums[i]), `transform number ${i}: before=${transformBefore} after=${transformAfter}`).toBeLessThan(0.5);
  }

  const after = await page.screenshot({ clip });
  const beforePixels = await samplePixels(page, before, clip.width, clip.height);
  const afterPixels = await samplePixels(page, after, clip.width, clip.height);
  for (let i = 0; i < beforePixels.length; i++) {
    for (let channel = 0; channel < 4; channel++) {
      expect(
        Math.abs(beforePixels[i][channel] - afterPixels[i][channel]),
        `sample point ${i} channel ${channel}: before=${beforePixels[i]} after=${afterPixels[i]}`,
      ).toBeLessThanOrEqual(6);
    }
  }
});

test("the capsule's cells and the reset circle follow their own rounded shape", async ({ page }) => {
  await gotoReady(page);
  const capsuleRadius = await page.locator(".moss-places-capsule").evaluate((el) => getComputedStyle(el).borderRadius);
  expect(capsuleRadius).toBe("18px");

  const firstCellRadii = await page
    .locator(".moss-places-capsule .moss-places-control")
    .first()
    .evaluate((el) => {
      const cs = getComputedStyle(el);
      return [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomLeftRadius, cs.borderBottomRightRadius];
    });
  // The capsule's own rounded ends, inherited at just the two OUTER
  // corners — the inner corners (at the hairline divider) stay flat.
  expect(firstCellRadii).toEqual(["18px", "18px", "0px", "0px"]);

  const resetRadius = await page.locator('[data-control="reset"]').evaluate((el) => getComputedStyle(el).borderRadius);
  expect(resetRadius).toBe("50%");
});

test("regional tiles are absent at the world's own cover zoom and present past its detail ceiling", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page);
  // At rest the world layer alone carries the view — a tile would sit on
  // top of it as a visibly lighter rectangle (the bug this guards), not
  // merely an invisible one.
  await expect(page.locator(".moss-places-tiles > svg")).toHaveCount(0);

  // Lisbon (38.722N, 9.139W): a coastal fixture place whose own cell plus
  // its eight neighbours (`place_map::explorer::relevant_tiles`) are all
  // populated (the pack's seafloor data reaches everywhere) — panning here
  // at z=8, comfortably past this viewport's own world ceiling (about 5,
  // `detailMaxZoom`), is guaranteed a real regional tile to fetch.
  const world = pattersonProject(38.722, -9.139);
  await page.goto(`places/?p=patterson&z=8&x=${world.x}&y=${world.y}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(500); // tile fetch + position settle
  const count = await page.locator(".moss-places-tiles > svg").count();
  expect(count, "expected a regional tile past the world's own detail ceiling").toBeGreaterThan(0);
});

/** `camera.ts`'s own `detailMaxZoom`, reimplemented from its published formula rather than imported — the same cross-check `pattersonProject` above already applies to the projection itself. `WORLD_WIDTH` comes straight from `pattersonProject` rather than a second hardcoded constant: at longitude 180 its own `x` is exactly the full canvas width (`width/2 + scale*PI`, and `scale*PI === width/2` by `projection.ts`'s own derivation). */
function detailMaxZoomFor(viewport: { width: number; height: number }): number {
  const DETAIL_MAX_SCALE = 7.21 * (560 / 480);
  const WORLD_WIDTH = pattersonProject(0, 180).x;
  const WORLD_HEIGHT = 480;
  const coverScale = Math.max(viewport.width / WORLD_WIDTH, viewport.height / WORLD_HEIGHT);
  return DETAIL_MAX_SCALE / coverScale;
}

test("the tile cross-fade respects prefers-reduced-motion", async ({ page }) => {
  const viewport = { width: 1280, height: 800 };
  await page.setViewportSize(viewport);
  // Inside the last 20% of the world's own zoom range — `tileFadeOpacity`'s
  // own fade band (`tiles.ts`) — so the transition is actually doing
  // something at this camera, not merely declared and unused.
  const fadeBandZoom = detailMaxZoomFor(viewport) * 0.9;
  const center = pattersonProject(0, 0);
  const gotoFadeBand = () => page.goto(`places/?p=patterson&z=${fadeBandZoom}&x=${center.x}&y=${center.y}`, { waitUntil: "domcontentloaded" });
  const waitReady = () =>
    expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
      "data-moss-places-explorer-ready",
      "ready",
      { timeout: 10000 },
    );

  await page.emulateMedia({ reducedMotion: "reduce" });
  await gotoFadeBand();
  await waitReady();
  const reducedDuration = await page.locator(".moss-places-tiles").evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(reducedDuration).toBe("0s");

  await page.emulateMedia({ reducedMotion: "no-preference" });
  await gotoFadeBand();
  await waitReady();
  const normalDuration = await page.locator(".moss-places-tiles").evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(normalDuration).not.toBe("0s");
});

interface AdjacentPair {
  edgeX: number;
  top: number;
  bottom: number;
}

/** Navigate to a deep Lisbon-area zoom past the world ceiling, where regional tiles exist, and return every horizontally-adjacent pair of tile rects' shared edge — throwing if any two rects overlap on both axes (a tile placed outside its own cell). */
async function loadAndFindAdjacentTilePairs(page: import("@playwright/test").Page): Promise<{ pairs: AdjacentPair[]; viewportSize: { width: number; height: number } }> {
  const viewport = { width: 1280, height: 800 };
  await page.setViewportSize(viewport);

  // Lisbon (38.722N, 9.139W): a coastal fixture place whose own cell plus
  // its eight neighbours (`place_map::explorer::relevant_tiles`) are all
  // populated (the pack's seafloor data reaches everywhere), so panning
  // here is guaranteed real regional tiles, not open cells with nothing to
  // fetch.
  const world = pattersonProject(38.722, -9.139);
  // z=8 is comfortably past this viewport's own world ceiling (about 5,
  // `detailMaxZoom`) and under the raised tile ceiling (about 20,
  // `tileDetailMaxZoom` at k=4) — exactly the zoom band regional tiles
  // exist to cover.
  await page.goto(`places/?p=patterson&z=8&x=${world.x}&y=${world.y}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(500); // tile fetches + position settle

  const tileEls = page.locator(".moss-places-tiles > svg");
  const count = await tileEls.count();
  expect(count, "expected multiple regional tiles in view at this zoom/pan").toBeGreaterThan(1);
  const rects: Array<{ x: number; y: number; width: number; height: number }> = [];
  for (let i = 0; i < count; i++) {
    rects.push((await tileEls.nth(i).boundingBox())!);
  }

  // No tile element sits outside its own cell's world rectangle: any two
  // tiles whose screen rects overlap on one axis must abut or deliberately
  // OVERLAP by no more than a tile's own bleed margin on the other — the
  // shape a correctly placed `translate(cellX - bleed, cellY - bleed)
  // scale(1/k)` produces for every neighbour pair (`tiles.ts`'s
  // `tileOverlayTransform`), and the shape the pre-fix bug (each tile its
  // own small misplaced rectangle, visible in the original review
  // screenshot) could not. The bound is the bleed margin's own worst case,
  // not a tuned fudge factor: two world units of bleed (one on each
  // neighbour) at the tile's own native screen scale — the ceiling this
  // zoom band stays under (`tileDetailMaxZoom`, `camera.ts`) —
  // 2 * 0.1 * (8.4117 * 4) is about 6.7px; a real misplacement (the wrong
  // cell entirely, or the pre-fix bug) overshoots that by a cell's width,
  // not a few px.
  const TOLERANCE = 10;
  const MARGIN = 4; // px pulled in from the shared edge's own start/end so the sampled column stays inside both tiles, not right at a corner.
  const pairs: AdjacentPair[] = [];
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      const a = rects[i];
      const b = rects[j];
      const yOverlap = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
      const xOverlap = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
      if (yOverlap > TOLERANCE && xOverlap > TOLERANCE) {
        throw new Error(`tile rects ${i} and ${j} overlap on both axes: ${JSON.stringify(a)} / ${JSON.stringify(b)}`);
      }
      const bLeftOfA = Math.abs(a.x - (b.x + b.width)) < TOLERANCE;
      const aLeftOfB = Math.abs(b.x - (a.x + a.width)) < TOLERANCE;
      if (yOverlap > MARGIN * 2 && (bLeftOfA || aLeftOfB)) {
        pairs.push({
          edgeX: bLeftOfA ? a.x : b.x,
          top: Math.max(a.y, b.y) + MARGIN,
          bottom: Math.min(a.y + a.height, b.y + b.height) - MARGIN,
        });
      }
    }
  }
  expect(pairs.length, "expected at least one pair of horizontally adjacent tiles").toBeGreaterThan(0);
  return { pairs, viewportSize: page.viewportSize()! };
}

test("past the world ceiling, regional tiles overlay the world with no gap or overlap", async ({ page }) => {
  await loadAndFindAdjacentTilePairs(page);
});

// Chromium only: WebKit's own `page.screenshot`/in-page `Image` round trip
// was measured hanging past this test's timeout at this exact deep-zoom,
// many-tile state, independent of the clip size or the decode mechanism
// (`img.decode()` and `onload` both hung) — a WebKit/Playwright test-harness
// interaction, not a rendering difference the geometric test above (which
// passes in both engines, and is what actually guards the placement fix)
// would miss. Tracked as a known gap rather than masked.
test("past the world ceiling, the coast at a tile boundary is pixel-continuous", async ({ page, browserName }) => {
  test.skip(browserName === "webkit", "page.screenshot hangs at this deep-zoom state in WebKit — see comment above");
  const { pairs, viewportSize } = await loadAndFindAdjacentTilePairs(page);

  // The widest shared edge in view, clamped to stay on-page — a robust pick
  // over "the first pair found", which can be a sliver too thin to clip a
  // screenshot from.
  const widest = pairs.reduce((best, pair) => (pair.bottom - pair.top > best.bottom - best.top ? pair : best));
  const top = Math.max(0, widest.top);
  const bottom = Math.min(viewportSize.height, widest.bottom);
  const edgeX = Math.min(Math.max(widest.edgeX, 20), viewportSize.width - 20);

  // The coast at a tile boundary is continuous: sample pixels a couple of
  // px either side of the shared edge, at several heights along it. Both
  // tiles draw the SAME underlying terrain through the SAME Patterson
  // projection at that boundary, so the paint a couple of px inside each
  // one should closely match; the pre-fix bug (two different crops, two
  // different projections) could not produce that regardless of which
  // coastline happened to sit there.
  const points: Array<{ x: number; y: number }> = [];
  const samples = 6;
  for (let s = 0; s <= samples; s++) {
    const y = top + ((bottom - top) * s) / samples;
    points.push({ x: edgeX - 3, y }, { x: edgeX + 3, y });
  }
  const clip = { x: Math.floor(edgeX - 20), y: Math.floor(top), width: 40, height: Math.max(1, Math.ceil(bottom - top)) };
  const png = await page.screenshot({ clip, animations: "disabled" });
  const localPoints = points.map((p) => ({ x: p.x - clip.x, y: p.y - clip.y }));
  const pixels = await samplePixelsAt(page, png, localPoints);
  for (let s = 0; s <= samples; s++) {
    const left = pixels[s * 2];
    const right = pixels[s * 2 + 1];
    for (let channel = 0; channel < 4; channel++) {
      expect(
        Math.abs(left[channel] - right[channel]),
        `sample ${s} channel ${channel}: left=${left} right=${right} (no step wider than the line's own width should appear across the seam)`,
      ).toBeLessThanOrEqual(24);
    }
  }
});
