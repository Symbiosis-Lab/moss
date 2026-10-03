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
import { test, expect } from "@playwright/test";

const FIGURE = "figure[data-moss-places-explorer]";
const POSTER = "[data-moss-place-embed]";

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

  // The embed page is the places root loaded with `embed=1` into an iframe
  // exactly as big as the locator's static poster, which is 352x235 in a
  // 1280px window and the window-wide 390x260 in a 390px one. Loaded as a
  // page of that size, its figure must fill the whole box. (Opened through the
  // host page instead, WebKit never reports this vertical page's locator as
  // intersecting, so the iframe would not exist to measure.)
  test(`the embed page's map paints its whole box at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("harbour/", { waitUntil: "domcontentloaded" });
    const poster = (await page.locator(`${POSTER} > svg`).boundingBox())!;
    const box = { width: Math.round(poster.width), height: Math.round(poster.height) };
    if (width === 1280) expect(box).toEqual({ width: 352, height: 235 });

    await page.setViewportSize(box);
    await page.goto("places/?place=places/lisbon&embed=1", { waitUntil: "domcontentloaded" });
    await expect(page.locator("html[data-moss-embed]")).toHaveCount(1);
    await expect(page.locator(`${FIGURE} .moss-places-viewport`).first()).toBeVisible();
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
