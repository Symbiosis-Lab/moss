/**
 * Render gate: a theme that flattens `.nav-right` keeps its own masthead.
 *
 * The fixture's theme sets `.nav-right { display: contents }` and orders the
 * pieces itself: the name alone on row 1, the links at the start of row 2 and
 * the toggles at its end. Where one row would not fit, nav-split must not
 * apply its split, whose grid lifts the toggles up beside the name.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-flattened.config.ts
 */

import { test, expect } from "@playwright/test";

for (const width of [412, 767, 768, 769, 1280]) {
  test(`a flattened masthead keeps the toggles on the links' row at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const m = await page.evaluate(() => {
      const rect = (sel: string) => document.querySelector(sel)!.getBoundingClientRect();
      const name = rect(".nav-left");
      const links = rect(".nav-links");
      const icons = rect(".nav-icons");
      return {
        nameBottom: name.bottom,
        linksTop: links.top,
        linksBottom: links.bottom,
        linksLeft: links.left,
        iconsTop: icons.top,
        iconsMid: (icons.top + icons.bottom) / 2,
        iconsLeft: icons.left,
      };
    });
    const where = JSON.stringify(m);
    expect(m.linksTop, where).toBeGreaterThanOrEqual(m.nameBottom - 0.5);
    expect(m.iconsTop, where).toBeGreaterThanOrEqual(m.nameBottom - 0.5);
    expect(m.iconsMid, where).toBeGreaterThanOrEqual(m.linksTop);
    expect(m.iconsMid, where).toBeLessThanOrEqual(m.linksBottom);
    expect(m.iconsLeft, where).toBeGreaterThan(m.linksLeft);
  });
}
