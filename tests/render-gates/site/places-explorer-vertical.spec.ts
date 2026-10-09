/**
 * Render gate: under vertical typesetting the places explorer page and the
 * locator's hydrated embed page stay horizontal. `site/vertical.css` turns
 * `body` into a vertical-rl column, which made the explorer figure (sized
 * with `100cqi`) paint half outside it and the embed's map overflow its box.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_VERTICAL_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-vertical.config.ts
 */
import { test, expect, type FrameLocator, type Page } from "@playwright/test";

const FIGURE = "figure[data-moss-places-explorer]";
const POSTER = "[data-moss-place-embed]";

async function expectLiveMap(context: Page | FrameLocator, width: number, height: number) {
  const figure = context.locator(`${FIGURE}[data-moss-places-explorer-ready="ready"]`);
  await expect(figure).toHaveCount(1);
  const figureBox = await figure.boundingBox();
  expect(figureBox).not.toBeNull();
  expect(Math.abs(figureBox!.width - width), `live figure width ${figureBox!.width} vs ${width}`).toBeLessThanOrEqual(1);
  expect(Math.abs(figureBox!.height - height), `live figure height ${figureBox!.height} vs ${height}`).toBeLessThanOrEqual(1);

  const viewport = figure.locator(".moss-places-viewport");
  await expect(viewport).toBeVisible();
  await expect(figure.locator(".moss-places-controls")).toBeVisible();
  const terrain = figure.locator(".moss-places-world-surface");
  await expect(terrain).toBeVisible({ timeout: 10000 });
  const terrainBox = await terrain.boundingBox();
  expect(terrainBox).not.toBeNull();
  expect(terrainBox!.width, "terrain surface has a rendered width").toBeGreaterThan(0);
  expect(terrainBox!.height, "terrain surface has a rendered height").toBeGreaterThan(0);
}

for (const width of [1280, 390]) {
  test(`the explorer figure is as wide as the window at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    await expect(page.locator("body")).toHaveAttribute("data-typesetting", "vertical");
    await expect(page.locator(FIGURE)).toHaveCount(1);
    await expect(page.locator(`${FIGURE} .moss-places-viewport`).first()).toBeVisible();
    const box = (await page.locator(FIGURE).boundingBox())!;
    const window_ = await page.evaluate(() => document.documentElement.clientWidth);
    expect(Math.abs(box.x), `figure left ${box.x}`).toBeLessThanOrEqual(1);
    expect(Math.abs(box.width - window_), `figure width ${box.width} vs window ${window_}`).toBeLessThanOrEqual(1);
    expect(box.height).toBeGreaterThan(200);
    // The page itself must not scroll sideways (it did, 674px against 390, before the body was set back to horizontal).
    const scroll = await page.evaluate(() => ({ scrollWidth: document.documentElement.scrollWidth, clientWidth: document.documentElement.clientWidth }));
    expect(scroll.scrollWidth, "root page scrollWidth vs clientWidth").toBe(scroll.clientWidth);
  });

  // The hydrated locator keeps the poster's stable 3:2 host box (352x235 at
  // 1280px; window-wide 390x260 at 390px). Its static SVG is hidden once the
  // live iframe is ready, so measure the host and assert the actual embedded
  // map fills it before using those dimensions for the direct embed-page case.
  test(`the embed page's map paints its whole box at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("harbour/", { waitUntil: "domcontentloaded" });
    const host = page.locator(POSTER);
    await expect(host).toHaveAttribute("data-moss-place-embed-state", "ready");
    const hostBox = await host.boundingBox();
    expect(hostBox).not.toBeNull();
    const box = { width: Math.round(hostBox!.width), height: Math.round(hostBox!.height) };
    expect(box).toEqual(width === 1280 ? { width: 352, height: 235 } : { width: 390, height: 260 });
    const hostFrame = page.frameLocator(`${POSTER} iframe`);
    await expectLiveMap(hostFrame, box.width, box.height);

    await page.setViewportSize(box);
    await page.goto("places/?place=places/lisbon&embed=1", { waitUntil: "domcontentloaded" });
    await expect(page.locator("html[data-moss-embed]")).toHaveCount(1);
    await expectLiveMap(page, box.width, box.height);
    const figure = (await page.locator(FIGURE).boundingBox())!;
    expect(Math.abs(figure.x), `figure x ${figure.x}`).toBeLessThanOrEqual(1);
    expect(Math.abs(figure.y), `figure y ${figure.y}`).toBeLessThanOrEqual(1);
    // No vertical scrollbar: the embed's layout width is the whole box.
    const layoutWidth = await page.evaluate(() => document.documentElement.clientWidth);
    expect(layoutWidth, "embed layout width (a scrollbar would narrow it)").toBe(box.width);
    expect(Math.abs(figure.width - layoutWidth), `figure ${figure.width}x${figure.height} vs ${layoutWidth}x${box.height}`).toBeLessThanOrEqual(1);
    expect(Math.abs(figure.height - box.height), `figure ${figure.width}x${figure.height} vs ${box.width}x${box.height}`).toBeLessThanOrEqual(1);
  });
}
