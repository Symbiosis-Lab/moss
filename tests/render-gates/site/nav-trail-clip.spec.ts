/**
 * Render gate: with JS off, the masthead trail gives way in its stated order:
 * middle crumbs to "…", the parent crumb to its floor, then the site name.
 *
 * `.nav-left` clips its overflow. When the site name could not shrink, a long
 * name at a phone width pushed the parent crumb past that clip: the trail read
 * "Field Notes from the Northern Ridge / J… / Wi". The parent's own box still
 * held its floor width, so the check measures the part left visible inside
 * `.nav-left`, not the box.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-trail-clip.config.ts
 */

import { test, expect, type Page } from "@playwright/test";

const PAGE = "/journal/expeditions/winter-camp-records/day-twelve-on-the-ice/";

test.use({ javaScriptEnabled: false });

async function measure(page: Page, width: number) {
  await page.setViewportSize({ width, height: 800 });
  await page.goto(PAGE, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  const m = await page.evaluate(() => {
    const shortfall = (el: HTMLElement) => {
      const shrunk = el.getBoundingClientRect().width;
      el.style.flexShrink = "0";
      const whole = el.getBoundingClientRect().width;
      el.style.flexShrink = "";
      return whole - shrunk;
    };
    const left = document.querySelector(".nav-left")!;
    const box = left.getBoundingClientRect();
    const name = left.querySelector<HTMLElement>(".site-name")!;
    const crumbs = [...left.querySelectorAll<HTMLElement>("a.breadcrumb-segment")];
    const parent = crumbs[crumbs.length - 1];
    const p = parent.getBoundingClientRect();
    const em = parseFloat(getComputedStyle(parent).fontSize);
    return {
      crumbs: crumbs.map((c) => c.textContent),
      parentVisible: Math.min(p.right, box.right) - Math.max(p.left, box.left),
      parentFloor: parseFloat(getComputedStyle(parent).minInlineSize),
      middleWidths: crumbs.slice(0, -1).map((c) => c.getBoundingClientRect().width / em),
      // scrollWidth rounds to whole pixels, but a sub-pixel shortfall still draws the "…".
      nameTruncated: shortfall(name) > 0,
      scrollWidth: document.documentElement.scrollWidth,
      innerWidth: window.innerWidth,
    };
  });
  const where = JSON.stringify(m);
  expect(m.crumbs, where).toEqual(["Journal", "Expeditions", "Winter Camp Records"]);
  expect(m.parentVisible, where).toBeGreaterThanOrEqual(m.parentFloor - 0.5);
  expect(m.scrollWidth, where).toBeLessThanOrEqual(m.innerWidth);
  return { m, where };
}

for (const width of [320, 375, 412]) {
  test(`at ${width}px the site name ellipsizes before the parent crumb is clipped under its floor`, async ({ page }) => {
    const { m, where } = await measure(page, width);
    expect(m.nameTruncated, where).toBe(true);
    for (const w of m.middleWidths) expect(w, where).toBeLessThan(1.5);
  });
}

// Wide enough that the parent absorbs the rest above its floor: shrink is shared by factor × base size, so
// the name still takes a sliver, and any sliver would draw a "…".
for (const width of [480, 560]) {
  test(`at ${width}px the parent crumb gives way and the site name stays whole`, async ({ page }) => {
    const { m, where } = await measure(page, width);
    expect(m.parentVisible, where).toBeGreaterThan(m.parentFloor + 1);
    expect(m.nameTruncated, where).toBe(false);
  });
}
