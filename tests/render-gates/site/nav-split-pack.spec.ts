/**
 * Render gate: the split masthead packs its links at the left edge and wraps
 * a long site name instead of clipping it.
 *
 * Three links keep row 2 mostly empty, so `justify-content` is visible: with
 * `space-between` the links spread across the row, and the gaps grow past the
 * normal link gap. The name has row 1 to itself in split mode, so it must
 * wrap within it.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-split-pack.config.ts
 */

import { test, expect, type Page } from "@playwright/test";

interface Geometry {
  split: boolean;
  siteNameLeft: number;
  firstLinkLeft: number;
  firstLinkRight: number;
  secondLinkLeft: number;
  iconsRight: number;
  contentRight: number;
  navLeftScroll: number;
  navLeftClient: number;
}

async function geometry(page: Page): Promise<Geometry> {
  return page.evaluate(() => {
    const content = document.querySelector(".nav-content") as HTMLElement;
    const navLeft = document.querySelector(".nav-left") as HTMLElement;
    const siteName = document.querySelector(".nav-left .site-name") as HTMLElement;
    const links = document.querySelectorAll<HTMLElement>(".nav-links a");
    const icons = document.querySelector(".nav-icons") as HTMLElement;
    const first = links[0].getBoundingClientRect();
    const second = links[1].getBoundingClientRect();
    return {
      split: content.hasAttribute("data-nav-split"),
      siteNameLeft: siteName.getBoundingClientRect().left,
      firstLinkLeft: first.left,
      firstLinkRight: first.right,
      secondLinkLeft: second.left,
      iconsRight: icons.getBoundingClientRect().right,
      contentRight: content.getBoundingClientRect().right,
      navLeftScroll: navLeft.scrollWidth,
      navLeftClient: navLeft.clientWidth,
    };
  });
}

for (const width of [1280, 768]) {
  test(`split links pack left and icons pack right at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const g = await geometry(page);
    // Precondition: the geometry below only means something on the split layout.
    expect(g.split, `masthead must split at ${width}px`).toBe(true);

    // (a) the first link starts on the site name's left edge.
    expect(Math.abs(g.firstLinkLeft - g.siteNameLeft)).toBeLessThanOrEqual(2);
    // (b) links are not spread: the gap between the first two is a normal gap.
    expect(g.secondLinkLeft - g.firstLinkRight).toBeLessThan(64);
    // (c) the toggles end on the nav's right edge.
    expect(Math.abs(g.contentRight - g.iconsRight)).toBeLessThanOrEqual(4);
  });
}

for (const width of [768, 390]) {
  test(`a long site name wraps instead of clipping at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const g = await geometry(page);
    expect(g.split, `masthead must split at ${width}px`).toBe(true);
    // .nav-left clips with overflow: clip, so overflowing content shows up as
    // scrollWidth above clientWidth.
    expect(g.navLeftScroll, `nav-left scroll=${g.navLeftScroll} client=${g.navLeftClient}`).toBeLessThanOrEqual(
      g.navLeftClient,
    );
  });
}
