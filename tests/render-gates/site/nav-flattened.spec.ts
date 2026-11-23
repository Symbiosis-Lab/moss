/**
 * Render gate: a theme that flattens `.nav-right` keeps its own masthead.
 *
 * Two fixture themes set `.nav-right { display: contents }` and order the
 * pieces themselves. "alone": the name or trail alone on row 1, the links at
 * the start of row 2 and the toggles at its end; nav-split must not apply its
 * split, whose grid lifts the toggles up beside the name. "beside" (phone
 * widths only): the name and the toggles on row 1, the links below. Either
 * masthead is the theme's, so moss's float layout for a plain name must stay
 * off it. In both the theme's `white-space: normal` must be able to wrap a
 * long name rather than have it clipped, and in the hamburger range (≤20rem)
 * a name past two lines gets its one-line width back, as on any other
 * masthead.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-flattened.config.ts
 */

import { test, expect, type Page } from "@playwright/test";
import { gatePort } from "../../../playwright/gate-ports";

const SITES = {
  alone: "/",
  trail: "/stations/akasaka/",
  beside: `http://localhost:${gatePort("nav-flattened:beside")}/`,
};
const HAMBURGER_MAX = 320;

async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

function read(page: Page) {
  return page.evaluate(() => {
    const rect = (sel: string) => document.querySelector(sel)!.getBoundingClientRect();
    const left = rect(".nav-left");
    const range = document.createRange();
    range.selectNodeContents(document.querySelector(".site-name")!);
    const text = [...range.getClientRects()].filter((r) => r.height > 0);
    const links = rect(".nav-links");
    const icons = rect(".nav-icons");
    const content = document.querySelector(".nav-content")!;
    return {
      owned: content.hasAttribute("data-nav-owned"),
      floated: getComputedStyle(document.querySelector(".nav-icons")!).float !== "none",
      nameLines: new Set(text.map((r) => Math.round(r.top))).size,
      textEnd: Math.max(...text.map((r) => r.right)),
      clipEnd: Math.min(left.right, document.documentElement.clientWidth),
      nameBottom: left.bottom,
      linksTop: links.top,
      linksBottom: links.bottom,
      linksLeft: links.left,
      iconsTop: icons.top,
      iconsBottom: icons.bottom,
      iconsMid: (icons.top + icons.bottom) / 2,
      iconsLeft: icons.left,
    };
  });
}

async function measure(page: Page, url: string, width: number) {
  await page.setViewportSize({ width, height: 900 });
  await page.goto(url, { waitUntil: "load" });
  await settle(page);
  return read(page);
}

for (const [shape, url] of Object.entries(SITES)) {
  for (const width of [320, 375, 412, 767, 768, 769, 1280]) {
    test(`a flattened masthead (${shape}) keeps its layout and its whole name at ${width}px`, async ({ page }) => {
      const m = await measure(page, url, width);
      const where = JSON.stringify(m);
      // A trail folds its crumbs to fit, by design.
      if (shape !== "trail") expect(m.textEnd, where).toBeLessThanOrEqual(m.clipEnd + 0.5);
      if (width <= HAMBURGER_MAX) {
        expect(m.nameLines, where).toBeLessThanOrEqual(2);
        return;
      }
      if (shape === "beside" && width >= 768) return;
      expect(m.floated, `the theme's masthead is not floated: ${where}`).toBe(false);
      if (shape !== "beside") {
        expect(m.linksTop, where).toBeGreaterThanOrEqual(m.nameBottom - 0.5);
        expect(m.iconsTop, where).toBeGreaterThanOrEqual(m.nameBottom - 0.5);
        expect(m.iconsMid, where).toBeGreaterThanOrEqual(m.linksTop);
        expect(m.iconsMid, where).toBeLessThanOrEqual(m.linksBottom);
        expect(m.iconsLeft, where).toBeGreaterThan(m.linksLeft);
      } else {
        expect(m.iconsBottom, where).toBeLessThanOrEqual(m.linksTop + 0.5);
        expect(m.linksTop, where).toBeGreaterThanOrEqual(m.nameBottom - 0.5);
      }
    });
  }
}

test("a theme that flattens only on a phone hands the masthead back and forth as the window crosses its width", async ({ page }) => {
  const url = SITES.beside;
  const at = async (width: number) => {
    await page.setViewportSize({ width, height: 900 });
    await settle(page);
    return read(page);
  };
  await page.setViewportSize({ width: 769, height: 900 });
  await page.goto(url, { waitUntil: "load" });
  await settle(page);
  for (const [width, theirs] of [[769, false], [767, true], [769, false]] as const) {
    const m = await at(width);
    const where = `${width}px: ${JSON.stringify(m)}`;
    expect(m.owned, where).toBe(theirs);
    expect(m.floated, where).toBe(!theirs);
  }
});
