/**
 * Render gate: the places-explorer embed — a `style:map` card and an
 * article's own locator, hydrated to the canonical live map (`embed.ts`).
 * Article locators start with the page; place cards hydrate near the
 * viewport. Both reuse the existing immersive/fullscreen mechanism.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-embed.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

const POSTER = "[data-moss-place-embed]";
const IFRAME = "iframe.moss-places-embed-frame";
const SETTLED = "iframe.moss-places-embed-frame.moss-places-embed-frame--settled";
const PREVIEW_BRIDGE = new URL("../../../crates/moss-build/src/ops/serve/js/iframe-bridge.js", import.meta.url).pathname;

/** Waits for the iframe to become the host's visible map after READY. */
async function waitForSettled(page: Page): Promise<void> {
  await expect(page.locator(SETTLED)).toHaveCount(1, { timeout: 10000 });
}

/** Holds map raster decoding in the real browser. Most first-frame tests
 * hold only the world raster so complete regional coverage remains usable;
 * the morph deadline test also holds tiles to guarantee this view needs both
 * raster paths before it can become ready. */
async function holdMapRasterDecode(page: Page, holdTiles = false): Promise<void> {
  await page.addInitScript((holdTiles: boolean) => {
    const blobs = new Map<string, Blob>();
    const createObjectURL = URL.createObjectURL.bind(URL);
    const decode = HTMLImageElement.prototype.decode;
    const waiting: Array<{ image: HTMLImageElement; resolve: () => void; reject: (reason: unknown) => void; isWorld: boolean }> = [];
    const audit = { worldDecodeCount: 0, tileDecodeCount: 0, released: false, releasedWorld: false, releasedTiles: false };
    const release = (isWorld: boolean): void => {
      if (isWorld) {
        audit.releasedWorld = true;
        audit.released = true;
      }
      else audit.releasedTiles = true;
      for (const pending of waiting.splice(0)) {
        if (pending.isWorld ? audit.releasedWorld : audit.releasedTiles) {
          decode.call(pending.image).then(pending.resolve, pending.reject);
        } else {
          waiting.push(pending);
        }
      }
    };
    (window as unknown as { __mapRasterAudit: typeof audit; __releaseWorldRaster: () => void; __releaseTileRaster: () => void }).__mapRasterAudit = audit;
    (window as unknown as { __releaseWorldRaster: () => void; __releaseTileRaster: () => void }).__releaseWorldRaster = () => release(true);
    (window as unknown as { __releaseTileRaster: () => void }).__releaseTileRaster = () => release(false);
    URL.createObjectURL = (blob: Blob) => {
      const url = createObjectURL(blob);
      blobs.set(url, blob);
      return url;
    };
    HTMLImageElement.prototype.decode = function (): Promise<void> {
      const url = this.currentSrc || this.src;
      const blob = blobs.get(url);
      if (!blob) return decode.call(this);
      return blob.text().then((svg) => {
        const root = /<svg\b([^>]*)>/i.exec(svg)?.[1] ?? "";
        const viewBox = /\bviewBox=["']([^"']+)["']/i.exec(root)?.[1]?.trim().split(/[\s,]+/).map(Number);
        const isWorld = !!viewBox && viewBox.length === 4 && Math.abs(viewBox[2] - 842.035025) < 0.001 && Math.abs(viewBox[3] - 480) < 0.001;
        if (!isWorld) {
          audit.tileDecodeCount++;
          if (holdTiles && !audit.releasedTiles) {
            return new Promise<void>((resolve, reject) => waiting.push({ image: this, resolve, reject, isWorld: false }));
          }
          return decode.call(this);
        }
        audit.worldDecodeCount++;
        if (audit.releasedWorld) return decode.call(this);
        return new Promise<void>((resolve, reject) => waiting.push({ image: this, resolve, reject, isWorld: true }));
      });
    };
  }, holdTiles);
}

async function holdWorldRasterDecode(page: Page): Promise<void> {
  await holdMapRasterDecode(page);
}

async function worldRasterAudit(page: Page) {
  return page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate(() =>
    (window as unknown as { __mapRasterAudit: { worldDecodeCount: number; tileDecodeCount: number; released: boolean } }).__mapRasterAudit,
  );
}

async function releaseWorldRaster(page: Page): Promise<void> {
  await page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate(() =>
    (window as unknown as { __releaseWorldRaster: () => void }).__releaseWorldRaster(),
  );
}

async function releaseTileRaster(page: Page): Promise<void> {
  await page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate(() =>
    (window as unknown as { __releaseTileRaster: () => void }).__releaseTileRaster(),
  );
}

async function regionalCanvasCoverage(page: Page) {
  return page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate((viewport) => {
    const bounds = viewport.getBoundingClientRect();
    const canvases = [...document.querySelectorAll<HTMLCanvasElement>(".moss-places-tile > canvas")].filter((canvas) => {
      const box = canvas.getBoundingClientRect();
      return canvas.width > 0 && canvas.height > 0 && box.right > bounds.left && box.left < bounds.right && box.bottom > bounds.top && box.top < bounds.bottom;
    });
    const boxes = canvases.map((canvas) => canvas.getBoundingClientRect());
    const xEdges = [...new Set([bounds.left, bounds.right, ...boxes.flatMap((box) => [Math.max(bounds.left, box.left), Math.min(bounds.right, box.right)])])].sort((a, b) => a - b);
    const complete = xEdges.slice(1).every((right, index) => {
      const left = xEdges[index]!;
      if (right <= left) return true;
      const middle = (left + right) / 2;
      const spans = boxes.filter((box) => box.left <= middle && box.right >= middle).sort((a, b) => a.top - b.top);
      let coveredTo = bounds.top;
      for (const span of spans) {
        if (span.bottom <= coveredTo) continue;
        if (span.top > coveredTo + 0.01) return false;
        coveredTo = Math.max(coveredTo, span.bottom);
        if (coveredTo >= bounds.bottom - 0.01) return true;
      }
      return false;
    });
    const dpr = window.devicePixelRatio || 1;
    const backingAtDpr = canvases.every((canvas, index) => canvas.width + 1 >= boxes[index]!.width * dpr && canvas.height + 1 >= boxes[index]!.height * dpr);
    const tiles = document.querySelector<HTMLElement>(".moss-places-tiles")!;
    return {
      visibleCanvasCount: canvases.length,
      complete,
      backingAtDpr,
      opacity: Number(getComputedStyle(tiles).opacity),
      targetOpacity: Number(tiles.style.getPropertyValue("--moss-place-tile-opacity")),
    };
  });
}

// World geometry the live camera and the static poster share
// (`projection.ts` WORLD_*; `geometry.rs` VIEWBOX_*/MIN_FRAME_DEGREES/LATITUDE_SHARE).
const WORLD_W = 842.035;
const WORLD_H = 480;
const POSTER_VIEWBOX_H = 480;
const POSTER_PX_PER_DEGREE = POSTER_VIEWBOX_H / (10 * 0.68);

/** Visible longitude, in degrees, of an embed at the zoom its own URL records. */
async function liveSpanDegrees(page: Page, embedUrl: string): Promise<number> {
  const z = Number(new URL(embedUrl).searchParams.get("z"));
  const box = (await page.locator(IFRAME).boundingBox())!;
  const scale = z * Math.max(box.width / WORLD_W, box.height / WORLD_H);
  return (box.width / scale / WORLD_W) * 360;
}

/** Longitude the static poster shows, from its own SVG: viewBox width over the build's design px/degree. */
async function posterSpanDegrees(page: Page): Promise<number> {
  const vbWidth = await page.locator(`${POSTER} svg`).first().evaluate((svg) => (svg as SVGSVGElement).viewBox.baseVal.width);
  return vbWidth / POSTER_PX_PER_DEGREE;
}

/**
 * Activates the fullscreen/expand control — by keyboard on WebKit, by a
 * plain click everywhere else. Two independent WebKit limitations rule out
 * a plain `locator.click()` or a raw `page.mouse.click()` there: (1)
 * `locator.click()`'s own pre-click actionability wait stalls ~15s in this
 * harness (same actionability-polling limitation `places-explorer-ring.spec.ts`'s
 * "a real click on a ring dot" test found on the full explorer page), and
 * (2) a raw click's own `boundingBox()` snapshot can read a stale
 * cross-document position for an element inside this iframe shortly after
 * an earlier fullscreen transition (measured: `getComputedStyle` agrees
 * with the new layout immediately, `getBoundingClientRect()` does not).
 * Focus + a keyboard `Enter` needs neither an actionability poll nor a
 * coordinate snapshot, so it sidesteps both — measured at ~30-50ms on
 * WebKit, same as Chromium's plain click. Chromium keeps the plain click
 * because swapping it for the keyboard path is unnecessary (it never
 * stalls) and because a raw click there measurably raced the fullscreen
 * enter transition's own before/after size capture.
 */
async function clickFullscreenButton(page: Page, browserName: string): Promise<void> {
  if (browserName === "webkit") {
    await page.locator(".immersive-fullscreen-btn").evaluate((el) => (el as HTMLElement).focus());
    await page.keyboard.press("Enter");
  } else {
    await page.locator(POSTER).hover();
    await page.locator(".immersive-fullscreen-btn").click();
  }
}

/** Wait for the controls' own completion state, including reduced-motion entry. */
async function clickAndWaitForFullscreen(
  page: Page,
  browserName: string,
  afterActivation?: () => Promise<void>,
): Promise<void> {
  await clickFullscreenButton(page, browserName);
  await afterActivation?.();
  await expect(page.locator("body")).toHaveClass(/immersive-fs-active/, { timeout: 2000 });
  const wrapper = page.locator(POSTER).locator(".immersive-iframe-wrapper");
  await expect(wrapper).not.toHaveClass(/fs-animating-enter/, { timeout: 2000 });
  await expect(wrapper.locator(".immersive-fullscreen-btn")).toBeEnabled({ timeout: 2000 });
}

test.describe("style:map embed", () => {
  test("keeps one stable host, shows a loading state, then reveals only the ready iframe", async ({ page }) => {
    let releaseWorld!: () => void;
    const worldHeld = new Promise<void>((resolve) => (releaseWorld = resolve));
    await page.route("**/world.svg", async (route) => {
      await worldHeld;
      await route.continue();
    });
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    const poster = page.locator(POSTER);
    await expect(page.locator(IFRAME)).toHaveCount(1);
    await expect(poster).toHaveAttribute("data-moss-place-embed-state", "loading");
    await expect(poster).toHaveAttribute("aria-busy", "true");
    await expect(poster.locator(":scope > .moss-places-embed-status")).toBeVisible();
    await expect(poster.locator(":scope > svg")).toHaveCSS("display", "none");
    releaseWorld();
    await waitForSettled(page);
    await expect(poster).toHaveAttribute("data-moss-place-embed-ready", "ready");
    await expect(poster).toHaveAttribute("data-moss-place-embed-state", "ready");
    await expect(poster).toHaveAttribute("aria-busy", "false");
    await expect(poster.locator(":scope > .moss-places-embed-status")).toHaveCount(0);
    await expect(poster.locator(":scope > svg")).toHaveCSS("display", "none");
    await expect(page.locator(IFRAME)).toHaveCSS("visibility", "visible");
    await expect(page.locator(IFRAME)).toHaveCSS("opacity", "1");
    const tilePaint = await page.frameLocator(IFRAME).locator(".moss-places-tiles").evaluate((tiles) => ({
      actual: Number(getComputedStyle(tiles).opacity),
      target: Number.parseFloat(getComputedStyle(tiles).getPropertyValue("--moss-place-tile-opacity")),
      count: tiles.childElementCount,
    }));
    expect(tilePaint.count).toBeGreaterThan(0);
    expect(tilePaint.actual).toBeCloseTo(tilePaint.target, 3); // READY must not rely on hover to finish the tiles' opacity transition
  });

  test("keeps the host in its loading state until every visible regional tile has decoded", async ({ page }) => {
    const releaseTileRequests: Array<{ released: boolean; release: () => void }> = [];
    await page.route(/\/tile-\d+-\d+\.svg(?:\?|$)/, async (route) => {
      await new Promise<void>((resolve) => releaseTileRequests.push({ released: false, release: resolve }));
      await route.continue();
    });

    try {
      await page.goto("tile-boundary-story/", { waitUntil: "domcontentloaded" });
      const poster = page.locator(POSTER);
      await expect(poster.locator("> svg")).toHaveCount(1);
      await expect(page.frameLocator(IFRAME).locator(".moss-places-viewport")).toHaveCount(1);
      await expect.poll(() => releaseTileRequests.length, { timeout: 5000 }).toBeGreaterThanOrEqual(2);
      await expect(page.locator(SETTLED)).toHaveCount(0);
      await expect(poster.locator("> svg")).toHaveCount(1);

      // Let one cell at a time through until a tile that actually intersects
      // the viewport has decoded. Keep all other visible cells blocked to
      // prove that a partial regional paint is not enough to expose the iframe.
      let loadedVisibleTiles = 0;
      while (loadedVisibleTiles === 0) {
        const next = releaseTileRequests.find((request) => !request.released);
        if (!next) throw new Error("no held tile request remains");
        next.released = true;
        next.release();
        await page.waitForTimeout(120);
        loadedVisibleTiles = await page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate((viewport) => {
          const bounds = viewport.getBoundingClientRect();
          return [...document.querySelectorAll<HTMLCanvasElement>(".moss-places-tile > canvas")].filter((canvas) => {
            const box = canvas.getBoundingClientRect();
            return canvas.width > 0 && box.right > bounds.left && box.left < bounds.right && box.bottom > bounds.top && box.top < bounds.bottom;
          }).length;
        });
      }
      await expect(page.locator(SETTLED)).toHaveCount(0);
      await expect(poster.locator("> svg")).toHaveCount(1);

      releaseTileRequests.filter(({ released }) => !released).forEach((request) => {
        request.released = true;
        request.release();
      });
      await waitForSettled(page);
      const visibleTiles = await page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate((viewport) => {
        const bounds = viewport.getBoundingClientRect();
        return [...document.querySelectorAll<HTMLCanvasElement>(".moss-places-tile > canvas")]
          .map((canvas) => ({ canvas, box: canvas.getBoundingClientRect() }))
          .filter(({ box }) => box.right > bounds.left && box.left < bounds.right && box.bottom > bounds.top && box.top < bounds.bottom)
          .map(({ canvas }) => ({ width: canvas.width, height: canvas.height, hidden: canvas.getAttribute("aria-hidden") }));
      });
      expect(visibleTiles.length).toBeGreaterThanOrEqual(2);
      expect(visibleTiles.every((tile) => tile.width > 0 && tile.height > 0 && tile.hidden === "true")).toBe(true);
    } finally {
      releaseTileRequests.filter(({ released }) => !released).forEach((request) => request.release());
    }
  });

  test("restores the static fallback when a visible regional tile fails", async ({ page }) => {
    let failedTileRequests = 0;
    await page.route(/\/tile-\d+-\d+\.svg(?:\?|$)/, async (route) => {
      failedTileRequests++;
      await route.fulfill({ status: 503, body: "tile unavailable" });
    });

    await page.goto("tile-boundary-story/", { waitUntil: "domcontentloaded" });
    const poster = page.locator(POSTER);
    await expect(poster.locator("> svg")).toHaveCount(1);
    await expect(page.frameLocator(IFRAME).locator(".moss-places-viewport")).toHaveCount(1);
    await expect.poll(() => failedTileRequests, { timeout: 5000 }).toBeGreaterThan(0);
    await expect(page.locator(SETTLED)).toHaveCount(0);
    await expect(poster.locator("> svg")).toHaveCount(1);
    // The host's bounded hydration timeout removes the failed iframe/wrapper
    // and restores the accessible static fallback.
    await expect(page.locator(IFRAME)).toHaveCount(0, { timeout: 10000 });
    await expect(poster.locator("> svg")).toHaveCount(1);
    await expect(poster).toHaveAttribute("data-moss-place-embed-state", "fallback");
    await expect(poster.locator(":scope > svg")).toHaveCSS("display", "block");
  });

  test("rechecks the resized viewport after pending visible tiles are superseded", async ({ page }) => {
    let holdTiles = true;
    const pending: Array<() => void> = [];
    await page.route(/\/tile-\d+-\d+\.svg(?:\?|$)/, async (route) => {
      if (holdTiles) await new Promise<void>((resolve) => pending.push(resolve));
      await route.continue();
    });

    await page.goto("tile-boundary-story/", { waitUntil: "domcontentloaded" });
    const poster = page.locator(POSTER);
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");
    await expect(viewport).toHaveCount(1);
    await expect.poll(() => pending.length, { timeout: 5000 }).toBeGreaterThan(0);
    const before = await viewport.boundingBox();
    await page.setViewportSize({ width: 600, height: 800 });
    await expect.poll(() => viewport.boundingBox()).not.toEqual(before);
    await expect(page.locator(SETTLED)).toHaveCount(0);
    await expect(poster.locator("> svg")).toHaveCount(1);

    holdTiles = false;
    pending.splice(0).forEach((release) => release());
    await waitForSettled(page);
    await expect(poster).toHaveAttribute("data-moss-place-embed-ready", "ready");
  });

  test("the iframe's own explorer is scoped to the embedded place", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const frame = page.frameLocator(IFRAME);
    await expect(frame.locator(".moss-places-viewport")).toHaveCount(1);
    const src = await page.locator(IFRAME).getAttribute("src");
    expect(src).toContain("place=places/lisbon");
    expect(src).toContain("embed=1");
  });

  test("the hydrated iframe fills the stable host exactly, at a narrow and a wide viewport", async ({ page }) => {
    // Forces the explicit load path (same as the Save-Data test below)
    // instead of the IntersectionObserver one: at the narrow width the
    // article column reflows taller, so the embed can start outside the
    // observer's near-viewport margin at scrollY 0, and scrolling it into
    // view first is its own source of flakiness under load (a locator
    // action's actionability wait, same class of thing the ctrl+wheel
    // test's own comment reports as unreliable here). A raw DOM click (not
    // a locator action) sidesteps both: hydration starts synchronously,
    // nothing to wait on.
    await page.addInitScript(() => {
      Object.defineProperty(window.navigator, "connection", { value: { saveData: true }, configurable: true });
    });
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
      await page.locator(`${POSTER} > .moss-places-embed-load`).click();
      await waitForSettled(page);
      const hostBox = (await page.locator(POSTER).boundingBox())!;
      const frameBox = (await page.locator(IFRAME).boundingBox())!;
      const SLACK = 1;
      expect(Math.abs(hostBox.x - frameBox.x)).toBeLessThanOrEqual(SLACK);
      expect(Math.abs(hostBox.y - frameBox.y)).toBeLessThanOrEqual(SLACK);
      expect(Math.abs(hostBox.width - frameBox.width)).toBeLessThanOrEqual(SLACK);
      expect(Math.abs(hostBox.height - frameBox.height)).toBeLessThanOrEqual(SLACK);
    }
  });

  test("the poster hands its accessible description to the live map once settled: the no-JS figure role/label is dropped, the static floor is made inert", async ({ page }) => {
    // The pre-settle state (role="img" present, no inert yet) is the
    // server-rendered HTML itself, already covered without a race by the
    // unit test (embed.test.ts) and by the Rust-side render tests
    // (context.rs) — checked here only post-settle, since hydration can
    // finish fast enough to race a "before" assertion in a real browser.
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    const poster = page.locator(POSTER);
    const staticFloor = page.locator(`${POSTER} > svg`);
    await waitForSettled(page);
    await expect(poster).not.toHaveAttribute("role", "img");
    await expect(poster).not.toHaveAttribute("aria-label", /.+/);
    await expect(staticFloor).toHaveAttribute("inert", "");
  });

  test("the hydrated iframe carries a descriptive title naming the embedded place", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await expect(page.locator(IFRAME)).toHaveAttribute("title", /Lisbon/);
  });

  function intersects(a: { x: number; y: number; width: number; height: number }, b: { x: number; y: number; width: number; height: number }): boolean {
    return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
  }

  // The open-in-new-tab control is declined entirely for a places embed
  // (`embed.ts`'s `setupImmersiveIframe(iframe, cb, false)`) — never
  // emitted, so there is no second control left to overlap with anything;
  // only the expand/collapse control remains beside the zoom controls.
  test("no open-in-new-tab control exists beside the expand control, collapsed", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await expect(page.locator(".immersive-new-window-btn")).toHaveCount(0);
    await expect(page.locator(".immersive-fullscreen-btn")).toHaveCount(1);
  });

  test("no open-in-new-tab control exists beside the expand control, expanded, and the expand control never overlaps the chip", async ({ page, browserName }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await clickFullscreenButton(page, browserName);
    await expect(page.locator(".immersive-iframe-wrapper")).not.toHaveClass(/fs-animating-enter/, { timeout: 2000 });
    await expect(page.locator(".immersive-new-window-btn")).toHaveCount(0);
    // The wrapper class clears before the iframe has finished resizing and
    // the chip has been laid out inside it, so a single sample can catch
    // either box mid-move (an intermittent WebKit failure). Wait until both
    // hold still across two samples, then compare.
    const chip = page.frameLocator(IFRAME).locator(".moss-places-chip");
    const expand = page.locator(".immersive-fullscreen-btn");
    await expect(chip).toBeVisible();
    const sample = async () => JSON.stringify([await chip.boundingBox(), await expand.boundingBox()]);
    let previous = "";
    await expect
      .poll(async () => {
        const current = await sample();
        const still = current === previous;
        previous = current;
        return still;
      }, { timeout: 5000, intervals: [100] })
      .toBe(true);
    const chipBox = (await chip.boundingBox())!;
    const expandBoxExpanded = (await expand.boundingBox())!;
    expect(intersects(chipBox, expandBoxExpanded)).toBe(false);
  });

  // Over white map (ice, a polar band) the exit control must still read: a
  // solid surface and an icon at WCAG AA contrast against it.
  test("the exit-fullscreen control is an opaque surface with a legible icon", async ({ page, browserName }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const button = page.locator(".immersive-fullscreen-btn");
    const channels = (css: string) => css.match(/[\d.]+/g)!.map(Number);
    const read = () => button.evaluate((el) => {
      const cs = getComputedStyle(el);
      const box = el.getBoundingClientRect();
      return { background: cs.backgroundColor, color: cs.color, width: box.width, height: box.height };
    });
    // The exit surface must be opaque on the first fullscreen computed-style
    // read. Safari can leave a background transition pending forever during
    // native fullscreen, so check before waiting for the ancestor FLIP to settle.
    await clickAndWaitForFullscreen(page, browserName, async () => {
      await expect(page.locator("body")).toHaveClass(/immersive-fs-active/);
      const transitionProperties = await button.evaluate((el) => getComputedStyle(el).transitionProperty.split(",").map((property) => property.trim()));
      expect(transitionProperties).not.toContain("background");
      const immediateBackground = channels((await read()).background);
      expect(immediateBackground.length === 3 ? 1 : immediateBackground[3]).toBe(1);
    });
    // Fullscreen moves the button away from the pointer location used to
    // click it. Wait for that hover transform to finish before measuring its
    // untransformed 36px hit target; the background's separate transition is
    // not a proxy for this one.
    await page.mouse.move(0, 0);
    await expect.poll(() => button.evaluate((el) => getComputedStyle(el).transform), { timeout: 2000 }).toBe("none");
    const style = await read();
    const luminance = ([r, g, b]: number[]) => {
      const lin = (v: number) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
      return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    };
    const [hi, lo] = [luminance(channels(style.background)), luminance(channels(style.color))].sort((a, b) => b - a);
    expect((hi + 0.05) / (lo + 0.05), `icon ${style.color} on ${style.background}`).toBeGreaterThanOrEqual(4.5);
    expect(style.width).toBe(36);
    expect(style.height).toBe(36);
  });

  // The embed document keeps the site's "Skip to content" link, which points
  // nowhere useful inside a map the size of a card: Tab from the iframe goes
  // straight to the map.
  test("Tab from the iframe lands on the map viewport, not a skip link", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await page.locator(IFRAME).focus();
    await page.keyboard.press("Tab");
    const frame = page.frameLocator(IFRAME);
    await expect(frame.locator(".moss-places-viewport")).toBeFocused();
  });

  test("collapsed mode runs cooperative gestures", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");
    await expect(viewport).toHaveAttribute("data-gesture-mode", "cooperative");
  });

  test("map controls reveal on hover and focus, and remain visible on touch", async ({ page, browser, baseURL }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const expand = page.locator(".immersive-fullscreen-btn");
    const mapControls = page.frameLocator(IFRAME).locator(".moss-places-controls");
    await expect(expand).toHaveCSS("opacity", "0");
    await expect(mapControls).toHaveCSS("opacity", "0");

    const posterBox = (await page.locator(POSTER).boundingBox())!;
    await page.mouse.move(posterBox.x + posterBox.width / 2, posterBox.y + posterBox.height / 2);
    await expect(expand).toHaveCSS("opacity", "1");
    await expect(mapControls).toHaveCSS("opacity", "1");

    await page.mouse.move(0, 0);
    await expand.focus();
    await expect(expand).toHaveCSS("opacity", "1");
    await mapControls.locator("button").first().focus();
    await expect(mapControls).toHaveCSS("opacity", "1");

    const touchContext = await browser.newContext({ baseURL, hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });
    try {
      const touchPage = await touchContext.newPage();
      await touchPage.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
      await waitForSettled(touchPage);
      await expect(touchPage.locator(".immersive-fullscreen-btn")).toHaveCSS("opacity", "1");
      await expect(touchPage.frameLocator(IFRAME).locator(".moss-places-controls")).toHaveCSS("opacity", "1");
    } finally {
      await touchContext.close();
    }
  });

  test("the embedded iframe's document is requested exactly once — the wrapper built for the expand/open controls must not re-navigate an already-loaded iframe", async ({ page }) => {
    // Counts actual HTTP document requests, not Playwright's own
    // `framenavigated` event: that event also fires for a same-document
    // `history.replaceState` (map.ts's own camera-settle URL write, which
    // runs at mount regardless of embed/fullscreen state), which would
    // over-count a correctly-single-navigation iframe.
    let documentRequests = 0;
    page.on("request", (request) => {
      if (request.resourceType() === "document" && request.url().includes("places/lisbon") && request.url().includes("embed=1")) {
        documentRequests++;
      }
    });
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    expect(documentRequests).toBe(1);
  });

  test("ctrl+wheel zooms the embedded map", async ({ page, browserName }) => {
    // Previously fixme'd on WebKit: the ~15s wheel-delivery stall this
    // comment used to describe was the same live-filtered-SVG repaint cost
    // `map.ts`'s raster layers fixed (the main thread wasn't idle because
    // of this gate's own input dispatch — it was busy re-running relief
    // filters on every transform change). Passes for real now; see
    // map.ts's own module doc for the numbers.
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");
    await viewport.hover();
    const embedFrame = page.frames().find((f) => f.url().includes("place=places%2Flisbon"))!;
    const before = new URL(embedFrame.url()).searchParams.get("z");
    await page.keyboard.down("Control");
    await page.mouse.wheel(0, -200);
    await page.keyboard.up("Control");
    await expect.poll(() => new URL(embedFrame.url()).searchParams.get("z")).not.toBe(before);
  });

  test("the keyboard zooms the embedded map, same as ctrl+wheel — WebKit coverage for the test above", async ({ page }) => {
    // gestures.ts's own keydown handler on the viewport ("+"/"-") calls the
    // same `zoomAt`/`applyCamera` path ctrl+wheel does, unconditionally of
    // cooperative mode, and dispatches as a plain keydown rather than a
    // wheel event — so it carries none of that event's compositor-coalescing
    // cost (measured: ~40-50ms in both engines, including on WebKit, where
    // the wheel-based test above is fixme'd).
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const embedFrame = page.frames().find((f) => f.url().includes("place=places%2Flisbon"))!;
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");
    await viewport.evaluate((el) => (el as HTMLElement).focus());
    const before = new URL(embedFrame.url()).searchParams.get("z");
    await page.keyboard.press("+");
    await expect.poll(() => new URL(embedFrame.url()).searchParams.get("z")).not.toBe(before);
  });

  test("plain wheel zooms the collapsed embed without scrolling its host page", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");
    const embedFrame = page.frames().find((f) => f.url().includes("place=places%2Flisbon"))!;
    const before = new URL(embedFrame.url()).searchParams.get("z");
    await viewport.hover();
    const scrollBefore = await page.evaluate(() => window.scrollY);
    await page.mouse.wheel(0, -200);
    await expect.poll(() => new URL(embedFrame.url()).searchParams.get("z")).not.toBe(before);
    expect(await page.evaluate(() => window.scrollY)).toBe(scrollBefore);
    await expect(page.frameLocator(IFRAME).locator(".moss-places-coop-hint")).toHaveCount(0);
  });

  test("the expand control opens the full control set, with no open-in-new-tab control alongside it", async ({ page, browserName }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await expect(page.locator(".immersive-new-window-btn")).toHaveCount(0);

    await clickAndWaitForFullscreen(page, browserName);
    await expect(page.locator("body")).toHaveClass(/immersive-fs-active/);
    await page.mouse.move(0, 0);
    await expect(page.locator(".immersive-fullscreen-btn")).toHaveCSS("opacity", "1");
    const embedFrame = page.frames().find((f) => f.url().includes("place=places%2Flisbon"))!;
    await expect
      .poll(async () => embedFrame.locator("[data-moss-places-explorer]").getAttribute("data-moss-places-embed-mode"))
      .toBe("expanded");
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");
    await expect(viewport).not.toHaveAttribute("data-gesture-mode", "cooperative");

    // The data attribute above is necessary but not sufficient: the embed's
    // own wrapper sits several levels deeper than article.container's
    // direct child (unlike every other immersive iframe on the site), so
    // the fullscreen CSS has its own, easy-to-miss direct-child assumption
    // to clear too — this caught the wrapper collapsing to a 0-height box
    // (site.css's own chrome-hiding rule hid the embed's whole containing
    // figure, not just the chrome) with the data attribute alone still
    // reading "expanded". The FLIP class is added inside a requestAnimationFrame
    // and removed by afterTransition; a polling assertion can miss this short
    // interval entirely, so the observer above records its addition before
    // the action and resolves only after the class has been removed.
    const wrapper = page.locator(".immersive-iframe-wrapper");

    // Previously fixme'd on WebKit: the host document's own cross-document
    // layout/compositor flush for the <iframe> element was starved by the
    // same live-filtered-SVG repaint cost map.ts's raster layers fixed.
    // Passes for real now. Read as a poll, not a one-shot box: `transitionend`
    // (what the wait above keys off) was measured firing a frame or two
    // before the host's own layout/paint for the now-full-size iframe
    // actually committed — a few px short, settling within the poll's own
    // short window right after.
    const viewportSize = page.viewportSize()!;
    const SLACK = 4;
    await expect
      .poll(async () => (await page.locator(IFRAME).boundingBox())!.width, { timeout: 2000 })
      .toBeGreaterThanOrEqual(viewportSize.width - SLACK);
    await expect
      .poll(async () => (await page.locator(IFRAME).boundingBox())!.height, { timeout: 2000 })
      .toBeGreaterThanOrEqual(viewportSize.height - SLACK);
  });

  test("a real click on a marker inside the frame selects and zooms", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const embedFrame = page.frames().find((f) => f.url().includes("place=places%2Flisbon"))!;
    const viewport = page.frameLocator(IFRAME).locator(".moss-places-viewport");

    // Zoom away from the marker's own boot-time fit camera first (real
    // keyboard input), so the click's own re-fit below is an observable
    // camera change, not a no-op repeat of the camera already in place.
    await viewport.evaluate((el) => (el as HTMLElement).focus());
    await page.keyboard.press("-");
    await page.keyboard.press("-");
    const zBefore = new URL(embedFrame.url()).searchParams.get("z");

    // A raw `page.mouse.click()`, not `locator.click()`: the latter's own
    // pre-click actionability wait is what stalls in WebKit shortly after
    // boot (see `clickFullscreenButton`'s own doc for the same limitation
    // elsewhere in this file) — real input either way. Safe here, unlike
    // the fullscreen button, because no prior transition leaves this
    // iframe's own cross-document position stale.
    const marker = page.frameLocator(IFRAME).locator(".moss-places-marker");
    await expect(marker).toHaveCount(1); // this scope's one work, Lisbon Walk
    const box = (await marker.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);

    await expect(marker).toHaveAttribute("data-selected", "true");
    await expect(page.frameLocator(IFRAME).locator(".moss-places-status")).toHaveText("Lisbon Walk");
    await expect.poll(() => new URL(embedFrame.url()).searchParams.get("z")).not.toBe(zBefore);
  });
});

test.describe("card links inside an embed", () => {
  test("a real click on an opened card's link navigates the page, not the iframe", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const frame = page.frameLocator(IFRAME);
    const marker = frame.locator(".moss-places-marker");
    await expect(marker).toHaveCount(1);
    const box = (await marker.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    const read = frame.locator(".moss-places-card-read");
    await expect(read).toBeVisible();
    await expect(read).toHaveAttribute("target", "_top");
    await read.click();
    await expect(page).toHaveURL(/lisbon-walk\/$/);
  });
});

test.describe("article locator embed", () => {
  test("hands off on fully covering regional canvases while the real world raster decode is held", async ({ page }) => {
    await holdWorldRasterDecode(page);
    await page.goto("tile-boundary-story/", { waitUntil: "domcontentloaded" });
    const audit = () => worldRasterAudit(page);
    try {
      await expect.poll(async () => (await audit()).worldDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
      await waitForSettled(page);

      const state = await audit();
      expect(state.worldDecodeCount).toBeGreaterThan(0);
      expect(state.tileDecodeCount).toBeGreaterThan(0);
      expect(state.released).toBe(false);
      const firstFrame = await regionalCanvasCoverage(page);
      expect(firstFrame.visibleCanvasCount).toBeGreaterThan(0);
      expect(firstFrame.complete).toBe(true);
      expect(firstFrame.backingAtDpr).toBe(true);
      expect(firstFrame.opacity).toBe(1);
      expect(firstFrame.targetOpacity).toBe(1);
      await expect(page.locator(SETTLED)).toHaveCSS("visibility", "visible");
    } finally {
      await releaseWorldRaster(page);
    }
  });

  test("waits for the world raster when regional tiles cannot cover a resized frame", async ({ page }) => {
    await holdWorldRasterDecode(page);
    const pendingTiles: Array<() => void> = [];
    let releaseTiles = false;
    await page.route(/\/tile-\d+-\d+\.svg(?:\?|$)/, async (route) => {
      if (!releaseTiles) await new Promise<void>((resolve) => pendingTiles.push(resolve));
      await route.continue();
    });
    await page.goto("tile-boundary-story/", { waitUntil: "domcontentloaded" });
    const audit = () => worldRasterAudit(page);
    try {
      await expect.poll(async () => (await audit()).worldDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
      await expect.poll(() => pendingTiles.length, { timeout: 5000 }).toBeGreaterThan(0);

      // Widen the real embed frame while tile fetches are held. Check the
      // actual decoded coverage rather than assuming a tile count: the
      // world must remain required wherever the regional surface has gaps.
      await page.locator(POSTER).evaluate((poster) => {
        const locator = poster.closest<HTMLElement>(".moss-place-locator");
        if (locator) {
          locator.style.width = "2400px";
          locator.style.maxWidth = "none";
          locator.style.float = "none";
        }
        poster.style.width = "2400px";
        poster.style.height = "420px";
        poster.style.maxWidth = "none";
        poster.style.aspectRatio = "auto";
      });
      await expect.poll(() => page.frameLocator(IFRAME).locator(".moss-places-viewport").evaluate((viewport) => viewport.getBoundingClientRect().width), { timeout: 3000 }).toBeGreaterThan(2000);
      releaseTiles = true;
      pendingTiles.splice(0).forEach((release) => release());
      await expect.poll(async () => (await audit()).tileDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
      const incompleteFrame = await regionalCanvasCoverage(page);
      expect(incompleteFrame.complete, JSON.stringify(incompleteFrame)).toBe(false);
      await expect(page.locator(SETTLED)).toHaveCount(0);
      await expect(page.locator(POSTER)).toHaveAttribute("data-moss-place-embed-state", "loading");
      expect((await audit()).released).toBe(false);

      await releaseWorldRaster(page);
      await waitForSettled(page);
    } finally {
      await releaseWorldRaster(page);
    }
  });

  test("scopes to the article's own work and keeps it off its own card row", async ({ page }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const frame = page.frameLocator(IFRAME);
    // The marker for this article's own work is present (it's the locator's
    // whole point)...
    await expect(frame.locator(".moss-places-marker")).toHaveCount(1);
    // ...but its own card never shows in the row underneath, since the
    // reader is already reading it.
    await expect(frame.locator('[data-work-id="/lisbon-walk/"]')).toHaveCount(0);
  });

  test("an article with two places gets a clearly visible marker at each, and the camera fits both", async ({ page }) => {
    await page.goto("fjord-crossing/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const frame = page.frameLocator(IFRAME);
    const markers = frame.locator(".moss-places-marker");
    await expect(markers).toHaveCount(2);
    const frameBox = (await page.locator(IFRAME).boundingBox())!;
    for (const marker of await markers.all()) {
      const box = (await marker.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(frameBox.x);
      expect(box.y).toBeGreaterThanOrEqual(frameBox.y);
      expect(box.x + box.width).toBeLessThanOrEqual(frameBox.x + frameBox.width);
      expect(box.y + box.height).toBeLessThanOrEqual(frameBox.y + frameBox.height);
      // Solid centre, not only a faint fade: the region marker's dot is opaque.
      const dot = await marker.evaluate((el) => {
        const style = getComputedStyle(el, "::before");
        return { color: style.backgroundColor, shadow: style.boxShadow };
      });
      expect(dot.color).not.toMatch(/rgba\(.*, 0\)|transparent/);
    }
    // Region precision (Os) keeps the soft area around the dot.
    await expect
      .poll(async () => markers.evaluateAll((els) => els.filter((el) => getComputedStyle(el, "::before").boxShadow !== "none").length))
      .toBe(1);
  });

  test("carries no open-in-new-tab control", async ({ page }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await expect(page.locator(".immersive-new-window-btn")).toHaveCount(0);
  });

  test("the breadcrumb chip stays hidden while collapsed", async ({ page }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await expect(page.frameLocator(IFRAME).locator(".moss-places-chip")).not.toBeVisible();
  });

  test("expanding the embed shows the breadcrumb chip scoped to this article", async ({ page, browserName }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await clickFullscreenButton(page, browserName);
    await expect(page.locator(".immersive-iframe-wrapper")).not.toHaveClass(/fs-animating-enter/, { timeout: 2000 });
    const chip = page.frameLocator(IFRAME).locator(".moss-places-chip");
    await expect(chip).toBeVisible();
    await expect(chip).toContainText("This article");
  });

  // Fix: the locator used to open framed on the article's own place and
  // then re-fit itself out to the continental cover camera once the card
  // row's real height was known (`fitForScope`'s own "article" branch used
  // to fall through to `coverCamera(allPoints())`, same as a plain `all`
  // scope). `fitWork` (camera.ts) is the shared fix: a REGIONAL framing
  // that the enabled zoom-out button proves is not the continental cover
  // zoom, and the enabled zoom-in button proves is not the detail ceiling
  // either — read, not asserted against a hand-picked number, so this gate
  // can't go stale against a future retuning of either bound.
  test("opens framed on the article's own place, at a regional zoom, and stays there after 3s", async ({ page }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const embedFrame = page.frames().find((f) => f.url().includes("article=%2Flisbon-walk%2F"))!;
    const zFirst = new URL(embedFrame.url()).searchParams.get("z");
    expect(zFirst).not.toBeNull();

    const innerFrame = page.frameLocator(IFRAME);
    const zoomOutBtn = innerFrame.locator('.moss-places-control[data-control="zoom-out"]');
    const zoomInBtn = innerFrame.locator('.moss-places-control[data-control="zoom-in"]');
    await expect(zoomOutBtn).toBeEnabled(); // not the continental cover zoom
    await expect(zoomInBtn).toBeEnabled(); // not the detail ceiling either

    const marker = innerFrame.locator(".moss-places-marker");
    await expect(marker).toHaveCount(1);
    const markerBox = (await marker.boundingBox())!;
    const frameBox = (await page.locator(IFRAME).boundingBox())!;
    expect(markerBox.x).toBeGreaterThanOrEqual(frameBox.x);
    expect(markerBox.y).toBeGreaterThanOrEqual(frameBox.y);
    expect(markerBox.x + markerBox.width).toBeLessThanOrEqual(frameBox.x + frameBox.width);
    expect(markerBox.y + markerBox.height).toBeLessThanOrEqual(frameBox.y + frameBox.height);

    // Same framing as the static poster it replaces — not merely "not continental".
    const poster = await posterSpanDegrees(page);
    const live = await liveSpanDegrees(page, embedFrame.url());
    expect(live / poster).toBeGreaterThan(0.5);
    expect(live / poster).toBeLessThan(2);

    await page.waitForTimeout(3000);
    const zAfter = new URL(embedFrame.url()).searchParams.get("z");
    expect(zAfter).toBe(zFirst);
  });

  // Fix 3: activating the article's own single-marker dot used to fit it
  // through plain `fitPoints`, whose own near-zero span for a lone point
  // drives the zoom all the way to the detail ceiling (`camera.ts`'s own
  // doc on `fitPoints`) — "not a sensible level" either way: too tight to
  // be regional. `selectWork` (map.ts) now shares the same `fitWork` the
  // initial article scope uses.
  test("a real click on the article's own dot re-fits it at a regional zoom — not continental, not the maximum", async ({ page }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const frame = page.frameLocator(IFRAME);
    const viewport = frame.locator(".moss-places-viewport");
    const embedFrame = page.frames().find((f) => f.url().includes("article=%2Flisbon-walk%2F"))!;
    const zoomOf = () => new URL(embedFrame.url()).searchParams.get("z");
    const zInitial = zoomOf();

    // Zoom away from the boot-time fit first (real keyboard input, works
    // regardless of cooperative-gesture mode — gestures.ts's own doc), so
    // the click's own re-fit below is an observable camera change.
    await viewport.evaluate((el) => (el as HTMLElement).focus());
    await page.keyboard.press("-");
    await page.keyboard.press("-");
    await page.keyboard.press("-");

    const marker = frame.locator(".moss-places-marker");
    const box = (await marker.boundingBox())!;
    const cx = box.x + box.width / 2;
    const cy = box.y + box.height / 2;
    // The article's own work opens already selected; a real click on its dot
    // re-fits it rather than toggling the selection off.
    await page.mouse.click(cx, cy);

    await expect(marker).toHaveAttribute("data-selected", "true");
    const zoomOutBtn = frame.locator('.moss-places-control[data-control="zoom-out"]');
    const zoomInBtn = frame.locator('.moss-places-control[data-control="zoom-in"]');
    await expect(zoomOutBtn).toBeEnabled(); // regional, not continental
    await expect(zoomInBtn).toBeEnabled(); // regional, not the maximum zoom
    // The same fit the article opened on, give or take the card row's
    // height (the click's frame excludes it): well away from the zoomed-out
    // level the three "-" presses left it at.
    const spanBefore = await liveSpanDegrees(page, `http://x/?z=${zInitial}`);
    await expect
      .poll(async () => (await liveSpanDegrees(page, embedFrame.url())) / spanBefore)
      .toBeGreaterThan(0.9);
    await expect
      .poll(async () => (await liveSpanDegrees(page, embedFrame.url())) / spanBefore)
      .toBeLessThan(1.1);
  });
});

test.describe("article scope switch (fullscreen locator embed)", () => {
  const frameOf = (page: Page) => page.frameLocator(IFRAME);

  /** A real click, or focus + Enter on WebKit — see clickFullscreenButton's own doc, which applies to any control inside the just-expanded iframe. */
  async function activate(page: Page, browserName: string, target: ReturnType<ReturnType<typeof frameOf>["locator"]>): Promise<void> {
    if (browserName === "webkit") {
      await target.evaluate((el) => (el as HTMLElement).focus());
      await page.keyboard.press("Enter");
    } else {
      await target.click();
    }
  }

  async function openFullscreen(page: Page, browserName: string): Promise<void> {
    await page.goto("fjord-crossing/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await clickFullscreenButton(page, browserName);
    await expect(page.locator(".immersive-iframe-wrapper")).not.toHaveClass(/fs-animating-enter/, { timeout: 2000 });
    await expect(frameOf(page).locator(".moss-places-chip")).toBeVisible();
  }

  test("This article → All articles → This article: every article, then the one article's places, with the article's identity kept in the frame's URL", async ({ page, browserName }) => {
    await openFullscreen(page, browserName);
    const frame = frameOf(page);
    const articleOption = frame.locator('.moss-places-chip-scope-option[data-scope="article"]');
    const allOption = frame.locator('.moss-places-chip-scope-option[data-scope="all"]');
    await expect(articleOption).toHaveAttribute("aria-current", "true");
    await expect(frame.locator(".moss-places-marker")).toHaveCount(2);
    await expect(frame.locator("[data-work-id]")).toHaveCount(0);

    await activate(page, browserName, allOption);
    await expect(allOption).toHaveAttribute("aria-current", "true");
    await expect.poll(() => frame.locator(".moss-places-marker").count()).toBeGreaterThan(2);
    await expect(frame.locator('.moss-places-marker[data-current="true"]').first()).toBeVisible();
    await expect(frame.locator('[data-work-id="/fjord-crossing/"]')).toHaveCount(0); // never its own card

    await activate(page, browserName, articleOption);
    await expect(articleOption).toHaveAttribute("aria-current", "true");
    await expect(frame.locator(".moss-places-marker")).toHaveCount(2);
    await expect(frame.locator("[data-work-id]")).toHaveCount(0);
    const embedFrame = page.frames().find((f) => f.url().includes("embed=1"))!;
    expect(embedFrame.url()).toContain("article=");
  });

  test("a place scope picked under All articles survives a trip to This article and back", async ({ page, browserName }) => {
    await openFullscreen(page, browserName);
    const frame = frameOf(page);
    const allOption = frame.locator('.moss-places-chip-scope-option[data-scope="all"]');
    const articleOption = frame.locator('.moss-places-chip-scope-option[data-scope="article"]');
    await activate(page, browserName, allOption);
    await activate(page, browserName, allOption); // the chevron: opens the place menu
    const items = frame.locator(".moss-places-chip-menu-item");
    await expect(items.first()).toBeVisible();
    await activate(page, browserName, items.first());
    const terminal = frame.locator("[data-terminal]");
    const placeName = (await terminal.textContent())!.trim();
    expect(placeName).not.toBe("All articles");

    await activate(page, browserName, articleOption);
    await expect(frame.locator(".moss-places-chip-sep")).toHaveCount(0);
    await activate(page, browserName, allOption);
    await expect(frame.locator(".moss-places-chip-sep")).toHaveCount(1);
    await expect(frame.locator("[data-terminal]")).toHaveText(placeName);
  });

  test("a reload of the frame with a place picked under All articles keeps the switch, the article's ring and its absence from the cards", async ({ page, browserName }) => {
    await openFullscreen(page, browserName);
    const frame = frameOf(page);
    const allOption = frame.locator('.moss-places-chip-scope-option[data-scope="all"]');
    await activate(page, browserName, allOption);
    await activate(page, browserName, allOption); // the chevron: opens the place menu
    await activate(page, browserName, frame.locator(".moss-places-chip-menu-item", { hasText: "Bergen" }));
    await expect(frame.locator("[data-terminal]")).not.toHaveText("All articles");
    await expect(frame.locator('.moss-places-marker[data-current="true"]').first()).toBeVisible();

    const embedFrame = page.frames().find((f) => f.url().includes("embed=1"))!;
    expect(embedFrame.url()).toContain("place=");
    await embedFrame.evaluate(() => {
      (window as unknown as { __beforeReload: boolean }).__beforeReload = true;
      location.reload();
    });
    await expect.poll(() => embedFrame.evaluate(() => "__beforeReload" in window).catch(() => true)).toBe(false);
    // The reloaded document starts collapsed (the host sends the mode once, on
    // the toggle), so the chip is checked in the DOM, not for visibility.
    await expect(frame.locator('.moss-places-chip-scope-option[data-scope="all"][aria-current="true"]')).toHaveCount(1);
    await expect(frame.locator('.moss-places-marker[data-current="true"]').first()).toBeVisible();
    await expect(frame.locator('[data-work-id="/fjord-crossing/"]')).toHaveCount(0);
  });

  test("collapsing while on All articles returns to This article; expanding again opens on it", async ({ page, browserName }) => {
    await openFullscreen(page, browserName);
    const frame = frameOf(page);
    await activate(page, browserName, frame.locator('.moss-places-chip-scope-option[data-scope="all"]'));
    await expect.poll(() => frame.locator(".moss-places-marker").count()).toBeGreaterThan(2);

    // WebKit sometimes leaves keyboard focus inside the iframe, so the Enter
    // meant for the host's exit control lands on the chip; press again until
    // the page has actually left fullscreen.
    await expect(async () => {
      if (await page.locator("body.immersive-fs-active").count()) {
        await page.evaluate(() => window.focus());
        await clickFullscreenButton(page, browserName);
      }
      await expect(page.locator("body.immersive-fs-active")).toHaveCount(0, { timeout: 1500 });
    }).toPass({ timeout: 10000 });
    await expect(frame.locator(".moss-places-marker")).toHaveCount(2);

    await clickFullscreenButton(page, browserName);
    await expect(page.locator(".immersive-iframe-wrapper")).not.toHaveClass(/fs-animating-enter/, { timeout: 2000 });
    await expect(frame.locator('.moss-places-chip-scope-option[data-scope="article"]')).toHaveAttribute("aria-current", "true");
    await expect(frame.locator(".moss-places-marker")).toHaveCount(2);
  });

  test("pressing the checked All articles segment under a place scope widens and keeps focus on it", async ({ page, browserName }) => {
    await openFullscreen(page, browserName);
    const frame = frameOf(page);
    const allOption = frame.locator('.moss-places-chip-scope-option[data-scope="all"]');
    await activate(page, browserName, allOption);
    await activate(page, browserName, allOption); // the chevron: opens the place menu
    await activate(page, browserName, frame.locator(".moss-places-chip-menu-item", { hasText: "Bergen" }));
    await expect(frame.locator(".moss-places-chip-sep")).toHaveCount(1);

    await allOption.evaluate((el) => (el as HTMLElement).focus());
    await page.keyboard.press("Enter");
    await expect(frame.locator(".moss-places-chip-sep")).toHaveCount(0);
    await expect(allOption).toBeFocused();
  });

  test("the keyboard operates the switch and focus stays on the pressed segment", async ({ page, browserName }) => {
    await openFullscreen(page, browserName);
    const frame = frameOf(page);
    const allOption = frame.locator('.moss-places-chip-scope-option[data-scope="all"]');
    await allOption.evaluate((el) => (el as HTMLElement).focus());
    await page.keyboard.press("Enter");
    await expect(allOption).toHaveAttribute("aria-current", "true");
    await expect(allOption).toBeFocused();
    await page.keyboard.press("Escape"); // closes nothing here; must not drop focus or the scope
    await expect(allOption).toBeFocused();
    const articleOption = frame.locator('.moss-places-chip-scope-option[data-scope="article"]');
    // WebKit does not Tab to buttons by default, so focus is moved directly.
    await articleOption.evaluate((el) => (el as HTMLElement).focus());
    await page.keyboard.press("Space");
    await expect(articleOption).toHaveAttribute("aria-current", "true");
    await expect(articleOption).toBeFocused();
  });
});

test.describe("chip beside the host's exit control (fullscreen locator embed)", () => {
  async function expand(page: Page, browserName: string): Promise<void> {
    await page.goto("fjord-crossing/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    await clickAndWaitForFullscreen(page, browserName);
    await expect(page.frameLocator(IFRAME).locator(".moss-places-chip")).toBeVisible();
  }

  test.describe("on a phone", () => {
    test.use({ viewport: { width: 390, height: 844 } });
    test("the chip keeps the full 306px cap and never lies under the control", async ({ page, browserName }) => {
      await expand(page, browserName);
      const chip = page.frameLocator(IFRAME).locator(".moss-places-chip");
      // The widest the chip can get: stretch it and read what the cap leaves.
      const cap = await chip.evaluate((el) => {
        (el as HTMLElement).style.inlineSize = "2000px";
        const width = el.getBoundingClientRect().width;
        (el as HTMLElement).style.inlineSize = "";
        return width;
      });
      expect(cap).toBe((await chip.evaluate(() => document.documentElement.clientWidth)) - 84); // 306 on a 390px frame
      const control = (await page.locator(".immersive-fullscreen-btn").boundingBox())!;
      await expect
        .poll(async () => {
          const box = (await chip.boundingBox())!;
          return box.x < control.x + control.width && box.x + box.width > control.x && box.y < control.y + control.height && box.y + box.height > control.y;
        })
        .toBe(false);
    });
  });

  test("in a right-to-left page the chip stays at the right edge, where its start is", async ({ page, browserName }) => {
    await expand(page, browserName);
    const frame = page.frameLocator(IFRAME);
    await frame.locator("html").evaluate((el) => el.setAttribute("dir", "rtl"));
    const chip = frame.locator(".moss-places-chip");
    await expect.poll(async () => {
      const frameBox = (await page.locator(IFRAME).boundingBox())!;
      const box = (await chip.boundingBox())!;
      return frameBox.x + frameBox.width - (box.x + box.width);
    }).toBeLessThanOrEqual(20);
  });
});

// Entering fullscreen used to re-frame the SAME geographic range at the new
// size — `Camera.zoom` is relative to the viewport's own cover scale, so a
// bigger viewport magnified the embed's view — and the world raster and the
// regional tiles stayed baked for the small embed, so the magnified view was
// soft. Now a resize keeps the scale and centre (the bigger viewport shows
// more map), retaining the world's physical density, and leaving
// fullscreen returns to the embed's own range.
test.describe("fullscreen embed", () => {
  test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });

  /** World and regional backing pixels over displayed device pixels; the world floor retains its density, while visible regional detail stays >= 1. */
  async function sharpness(page: Page, embedFrame: ReturnType<Page["frames"]>[number]) {
    return embedFrame.evaluate(() => {
      const viewport = document.querySelector(".moss-places-viewport")!.getBoundingClientRect();
      const dpr = window.devicePixelRatio;
      const world = document.querySelector<HTMLCanvasElement>(".moss-places-world-surface");
      const tiles = [...document.querySelectorAll<HTMLCanvasElement>(".moss-places-tile > canvas")]
        .map((canvas) => ({ canvas, box: canvas.getBoundingClientRect() }))
        .filter(({ box }) => box.right > viewport.left && box.left < viewport.right && box.bottom > viewport.top && box.top < viewport.bottom)
        .map(({ canvas, box }) => canvas.width / (box.width * dpr));
      return { worldDensity: world ? world.width / (world.getBoundingClientRect().width * dpr) : 0, tiles };
    });
  }

  test("fullscreen shows a wider range at the same raster density; leaving returns to the embed's own range", async ({ page, browserName }) => {
    await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
    await waitForSettled(page);
    const embedFrame = page.frames().find((f) => f.url().includes("article=%2Flisbon-walk%2F"))!;
    const span = () => liveSpanDegrees(page, embedFrame.url());
    const spanEmbed = await span();
    const widthEmbed = (await page.locator(IFRAME).boundingBox())!.width;
    const bakedEmbed = (await sharpness(page, embedFrame)).worldDensity;
    expect(bakedEmbed).toBeGreaterThan(0);

    await clickAndWaitForFullscreen(page, browserName);
    await expect.poll(async () => (await page.locator(IFRAME).boundingBox())!.width / widthEmbed, { timeout: 2000 }).toBeGreaterThan(1.3);
    const widthFull = (await page.locator(IFRAME).boundingBox())!.width;

    // The scale (px per degree) is kept, so the visible range grows with the
    // viewport — not the same range magnified.
    await expect.poll(span, { timeout: 5000 }).toBeGreaterThanOrEqual(spanEmbed * (widthFull / widthEmbed) * 0.9);
    expect(widthFull / widthEmbed).toBeGreaterThan(1.3);

    // The world's physical magnification is unchanged, so its backing density
    // stays unchanged too. Newly visible regional detail remains sharp.
    expect((await sharpness(page, embedFrame)).worldDensity).toBeCloseTo(bakedEmbed, 3);
    await expect
      .poll(async () => Math.min(1, ...(await sharpness(page, embedFrame)).tiles), { timeout: 8000 })
      .toBeGreaterThanOrEqual(0.98);

    await clickFullscreenButton(page, browserName);
    await expect(page.locator(".immersive-iframe-wrapper")).not.toHaveClass(/fs-animating-exit/, { timeout: 4000 });
    await expect.poll(span, { timeout: 5000 }).toBeLessThanOrEqual(spanEmbed * 1.15);
    expect(await span()).toBeGreaterThanOrEqual(spanEmbed * 0.85);
  });
});

// Fix 2, same on the full page: `?article=…` with no `embed=1` never sets a
// `place` scope either (state.ts's own doc: article is a selection, not a
// scope), so this is the only other page that ever reaches `fitForScope`'s
// selection-aware branch.
test.describe("full-page ?article= locator", () => {
  test("opens framed on the article's own place, at a regional zoom, and stays there after 3s", async ({ page }) => {
    await page.goto("places/?article=%2Flisbon-walk%2F", { waitUntil: "domcontentloaded" });
    await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
      "data-moss-places-explorer-ready",
      "ready",
      { timeout: 10000 },
    );
    await page.waitForTimeout(300);
    const zFirst = new URL(page.url()).searchParams.get("z");
    expect(zFirst).not.toBeNull();

    const zoomOutBtn = page.locator('.moss-places-control[data-control="zoom-out"]');
    await expect(zoomOutBtn).toBeEnabled();
    // A 1440 px page lands on the tile ceiling (about 18 degrees across), so
    // zoom-in is legitimately disabled; regional is read from the span.
    const box = (await page.locator(".moss-places-viewport").boundingBox())!;
    const z = Number(zFirst);
    const span = (box.width / (z * Math.max(box.width / WORLD_W, box.height / WORLD_H)) / WORLD_W) * 360;
    expect(span).toBeGreaterThan(4);
    expect(span).toBeLessThan(20);

    // Neighbouring places are in view at this width; the article's own dot is the selected one.
    await expect(page.locator('.moss-places-marker[data-selected="true"]')).toHaveCount(1);

    await page.waitForTimeout(3000);
    const zAfter = new URL(page.url()).searchParams.get("z");
    expect(zAfter).toBe(zFirst);
  });
});

test.describe("hydration degrades to the static poster", () => {
  test("on Save-Data, hydration waits for an accessible load button", async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(window.navigator, "connection", {
        value: { saveData: true },
        configurable: true,
      });
    });
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    await page.waitForTimeout(500); // the ordinary path would have an iframe well within this
    await expect(page.locator(IFRAME)).toHaveCount(0);
    const loadButton = page.locator(`${POSTER} > .moss-places-embed-load`);
    await expect(loadButton).toBeVisible();
    await expect(loadButton).toHaveAttribute("aria-label", /Lisbon/);
    await expect(page.locator(POSTER)).toHaveAttribute("aria-busy", "false");
    await loadButton.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(IFRAME)).toHaveCount(1);
    await expect(page.locator(POSTER)).toHaveAttribute("aria-busy", "true");
    await waitForSettled(page);
  });

  test("without JavaScript, the accessible static SVG fallback remains visible", async ({ page }) => {
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    const url = page.url();
    const browser = page.context().browser();
    if (!browser) throw new Error("browser context is unavailable");
    const noJsPage = await browser.newPage({ javaScriptEnabled: false });
    try {
      await noJsPage.goto(url, { waitUntil: "domcontentloaded" });
      await expect(noJsPage.locator(POSTER)).toHaveAttribute("role", "img");
      await expect(noJsPage.locator(`${POSTER} > svg`)).toHaveCSS("display", "block");
      await expect(noJsPage.locator(IFRAME)).toHaveCount(0);
    } finally {
      await noJsPage.close();
    }
  });

  test("if the explorer bundle fails to load, the static fallback remains available", async ({ page }) => {
    await page.route("**/places-explorer*.js", (route) => route.abort());
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    const host = page.locator(POSTER);
    await expect(host).toHaveAttribute("data-moss-place-embed-state", "fallback");
    await expect(host.locator(":scope > svg")).toHaveCSS("display", "block");
    await expect(page.locator(IFRAME)).toHaveCount(0);
  });

  test("a failed data fetch inside the iframe restores the accessible static fallback", async ({ page }) => {
    await page.route("**/world.svg", (route) => route.abort());
    await page.goto("lisbon-overview/", { waitUntil: "domcontentloaded" });
    const poster = page.locator(POSTER);
    await expect(page.locator(IFRAME)).toHaveCount(1); // created...
    await page.waitForTimeout(8500); // ...then removed once the ready handshake times out
    await expect(page.locator(IFRAME)).toHaveCount(0);
    await expect(poster.locator("> svg")).toHaveCount(1);
    await expect(poster).toHaveAttribute("data-moss-place-embed-state", "fallback");
    await expect(poster.locator(":scope > svg")).toHaveCSS("display", "block");
    await expect(poster).not.toHaveAttribute("data-moss-place-embed-ready", "ready");
  });
});

test.describe("the hero", () => {
  test("a screen-placed style:map embed goes edge to edge with no gutters", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto("hero/", { waitUntil: "domcontentloaded" });
    const frame = page.locator('.moss-place-map-frame[data-width="screen"]');
    await expect(frame).toHaveCount(1);
    const box = (await frame.boundingBox())!;
    const contentWidth = await page.evaluate(() => document.documentElement.clientWidth);
    const SLACK = 8; // scrollbar-gutter slack, same margin places-explorer-boot.spec.ts already allows
    expect(box.x).toBeLessThanOrEqual(SLACK);
    expect(box.x + box.width).toBeGreaterThanOrEqual(contentWidth - SLACK);
  });
});

test("a second located-article page in the same context does not re-fetch the world SVG", async ({ page, browserName }) => {
  // Chromium's own request/response events double-fire per fetch() call in
  // this harness regardless of cache status (confirmed against the
  // webServer's own access log: a SINGLE real network hit total across both
  // navigations below, even when Playwright's `request` event reports four)
  // — so a plain request COUNT cannot tell a cache hit from the artifact.
  // Network.responseReceived's own `fromDiskCache` is the ground truth, and
  // is CDP-only; WebKit has no equivalent exposed through Playwright.
  test.skip(browserName !== "chromium", "cache-hit visibility (Network.responseReceived.response.fromDiskCache) is Chromium-only");
  const client = await page.context().newCDPSession(page);
  await client.send("Network.enable");
  const worldResponses: boolean[] = []; // each entry: fromDiskCache
  client.on("Network.responseReceived", (event) => {
    if (event.response.url.includes("world.svg")) {
      worldResponses.push(Boolean(event.response.fromDiskCache));
    }
  });

  await page.goto("lisbon-walk/", { waitUntil: "domcontentloaded" });
  await waitForSettled(page);
  expect(worldResponses.length).toBeGreaterThanOrEqual(1);
  expect(worldResponses.some((fromCache) => !fromCache)).toBe(true); // the first visit really fetches it over the network
  const beforeSecondNav = worldResponses.length;

  await page.goto("lisbon-harbor-light/", { waitUntil: "domcontentloaded" });
  await waitForSettled(page);
  const afterSecondNav = worldResponses.slice(beforeSecondNav);
  // Either no response event at all for the second visit (the request never
  // left the renderer), or every one of them came from disk cache — both
  // mean the network never served the bytes a second time.
  expect(afterSecondNav.every((fromCache) => fromCache)).toBe(true);
});

test("the real preview bridge rehydrates an article embed after a morph and cancels the old READY deadline", async ({ page }) => {
  await holdMapRasterDecode(page, true);
  await page.goto("tile-boundary-story/", { waitUntil: "domcontentloaded" });
  await expect(page.locator(IFRAME)).toHaveCount(1);
  // The host's 8s READY deadline starts when it creates this first iframe;
  // capture its conservative deadline before raster setup/audits take time.
  const oldDeadline = Date.now() + 8000;
  await expect(page.locator(POSTER)).toHaveAttribute("data-moss-place-embed-state", "loading");
  await expect.poll(async () => (await worldRasterAudit(page)).worldDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
  await expect.poll(async () => (await worldRasterAudit(page)).tileDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
  await expect(page.locator(POSTER)).not.toHaveAttribute("data-moss-place-embed-ready", "ready");
  await expect(page.locator(SETTLED)).toHaveCount(0);
  // Leave a clear gap between the original deadline and the replacement's
  // own deadline, while both actual raster dependencies remain held.
  await page.waitForTimeout(2000);
  await page.addScriptTag({ path: PREVIEW_BRIDGE });

  const oldIframe = await page.locator(IFRAME).elementHandle();
  if (!oldIframe) throw new Error("initial article map iframe was not created");
  const oldScope = await oldIframe.evaluate((iframe) => new URL((iframe as HTMLIFrameElement).src).searchParams.get("article"));
  expect(oldScope).toBeTruthy();
  const oldHostBox = await page.locator(POSTER).boundingBox();
  const morph = await page.evaluate(() =>
    new Promise<{ patched: boolean; applied: boolean; initCount: number }>((resolve, reject) => {
      let patched = false;
      let applied = false;
      const timeout = window.setTimeout(() => reject(new Error("preview bridge morph did not complete")), 10000);
      const finish = (): void => {
        if (!patched || !applied) return;
        window.clearTimeout(timeout);
        resolve({ patched, applied, initCount: (window as unknown as { __bridgeInitCount: number }).__bridgeInitCount });
      };
      document.addEventListener("moss-morph-patched", () => { patched = true; finish(); }, { once: true });
      window.addEventListener("message", (event) => {
        if (event.data?.type !== "moss-morph-applied" || event.data?.gen !== 8417) return;
        applied = true;
        finish();
      });
      window.postMessage({ type: "moss-morph", gen: 8417 }, "*");
    }),
  );
  expect(morph).toEqual({ patched: true, applied: true, initCount: 1 });

  const nextIframe = await page.locator(IFRAME).elementHandle();
  if (!nextIframe) throw new Error("article map iframe was not re-created after morph");
  // Keep the old-deadline assertion strictly before the replacement attempt's
  // own timeout, so a locator poll cannot cross the new deadline.
  const newDeadline = Date.now() + 8000;
  expect(oldDeadline + 100).toBeLessThan(newDeadline);
  expect(await oldIframe.evaluate((iframe) => iframe.isConnected)).toBe(false);
  expect(await nextIframe.evaluate((iframe) => new URL((iframe as HTMLIFrameElement).src).searchParams.get("article"))).toBe(oldScope);
  expect(await page.locator(POSTER).boundingBox()).toEqual(oldHostBox);
  await expect(page.locator(POSTER)).toHaveAttribute("data-moss-place-embed-state", "loading");
  await expect.poll(async () => (await worldRasterAudit(page)).worldDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
  await expect.poll(async () => (await worldRasterAudit(page)).tileDecodeCount, { timeout: 5000 }).toBeGreaterThan(0);
  await expect(page.locator(POSTER)).not.toHaveAttribute("data-moss-place-embed-ready", "ready");

  const timeUntilOldDeadline = oldDeadline - Date.now() + 100;
  if (timeUntilOldDeadline > 0) await page.waitForTimeout(timeUntilOldDeadline);
  expect(await page.locator(IFRAME).count()).toBe(1);
  expect(await page.locator(SETTLED).count()).toBe(0);
  expect(await page.locator(POSTER).getAttribute("data-moss-place-embed-state")).toBe("loading");

  await releaseTileRaster(page);
  await releaseWorldRaster(page);
  await waitForSettled(page);
  await expect(
    page.frameLocator(IFRAME).locator('.moss-places-tiles[data-moss-places-tiles-state="idle"]'),
  ).toHaveCount(1, { timeout: 15000 });
  const afterMorph = await regionalCanvasCoverage(page);
  expect(afterMorph.visibleCanvasCount).toBeGreaterThan(0);
  expect(afterMorph.complete).toBe(true); // the fixture's two visible regional tiles fully cover the viewport
  expect(afterMorph.backingAtDpr).toBe(true);
  expect(afterMorph.opacity).toBe(afterMorph.targetOpacity);
});
