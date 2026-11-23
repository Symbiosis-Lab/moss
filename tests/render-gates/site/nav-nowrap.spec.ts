/**
 * Render gate: a theme that holds the masthead to one fixed-height row keeps
 * its links inside the header.
 *
 * The fixture's theme sets `.nav-content { block-size: 64px; flex-wrap:
 * nowrap }` and shows its own menu button only below 48rem, so from 48rem
 * until the row fits there is no room for every link. It runs once with a
 * plain site name and once with a logo. A second row would
 * hang below the 64px box over the page, and one unbroken row would push the
 * page wider than the window; neither may happen.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-nowrap.config.ts
 */

import { test, expect } from "@playwright/test";
import { gatePort } from "../../../playwright/gate-ports";

const SITES = [
  ["a plain name", "/"],
  ["a logo", `http://localhost:${gatePort("nav-nowrap:logo")}/`],
] as const;

for (const [masthead, home] of SITES) {
  for (const width of [767, 768, 769, 840, 1280]) {
    test(`a nowrap masthead with ${masthead} keeps its links inside the header at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(home, { waitUntil: "load" });
      await page.evaluate(() => document.fonts.ready);
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
      const m = await page.evaluate(() => {
        const header = document.querySelector("body > header")!.getBoundingClientRect();
        const links = [...document.querySelectorAll<HTMLElement>(".nav-links a")]
          .map((a) => a.getBoundingClientRect())
          .filter((r) => r.width > 0 && r.height > 0);
        return {
          headerTop: header.top,
          headerBottom: header.bottom,
          linkTop: Math.min(...links.map((r) => r.top)),
          linkBottom: Math.max(...links.map((r) => r.bottom)),
          visible: links.length,
          scrollWidth: document.documentElement.scrollWidth,
          innerWidth: window.innerWidth,
        };
      });
      const where = JSON.stringify(m);
      expect(m.visible, where).toBeGreaterThan(0);
      expect(m.linkTop, where).toBeGreaterThanOrEqual(m.headerTop - 0.5);
      expect(m.linkBottom, where).toBeLessThanOrEqual(m.headerBottom + 0.5);
      expect(m.scrollWidth, where).toBeLessThanOrEqual(m.innerWidth);
    });
  }

  test(`with ${masthead}, at 769px the last link is reachable by keyboard and stays inside the header`, async ({ page }) => {
    await page.setViewportSize({ width: 769, height: 900 });
    await page.goto(home, { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const last = page.locator(".nav-links > a, .nav-links > .moss-nav-group > a").last();
    await last.focus();
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const m = await last.evaluate((a) => {
      const r = a.getBoundingClientRect();
      const box = document.querySelector(".nav-links")!.getBoundingClientRect();
      const header = document.querySelector("body > header")!.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, boxLeft: box.left, boxRight: box.right, headerBottom: header.bottom };
    });
    const where = JSON.stringify(m);
    expect(m.left, where).toBeGreaterThanOrEqual(m.boxLeft - 0.5);
    expect(m.right, where).toBeLessThanOrEqual(m.boxRight + 0.5);
    expect(m.bottom, where).toBeLessThanOrEqual(m.headerBottom + 0.5);
  });

  test(`with ${masthead}, at 769px a submenu past the scrolled edge opens below the row, unclipped, under its entry`, async ({ page }) => {
    await page.setViewportSize({ width: 769, height: 900 });
    await page.goto(home, { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const scrolled = await page.evaluate(() => {
      const links = document.querySelector(".nav-links")!;
      links.scrollLeft = links.scrollWidth;
      return links.scrollLeft;
    });
    expect(scrolled, "the group sits past the row's edge, so the links must scroll to reach it").toBeGreaterThan(0);
    await page.locator(".moss-nav-sub-toggle").click();
    const panel = page.locator(".moss-nav-sub");
    await expect(panel).toBeVisible();
    const m = await page.evaluate(() => {
      const p = document.querySelector(".moss-nav-sub")!.getBoundingClientRect();
      const g = document.querySelector(".moss-nav-group")!.getBoundingClientRect();
      const header = document.querySelector("body > header")!.getBoundingClientRect();
      const x = p.left + p.width / 2;
      const y = p.bottom - 4;
      const hit = document.elementFromPoint(x, y);
      return {
        top: p.top,
        left: p.left,
        right: p.right,
        groupLeft: g.left,
        groupRight: g.right,
        headerBottom: header.bottom,
        hitInPanel: !!hit?.closest(".moss-nav-sub"),
      };
    });
    const where = JSON.stringify(m);
    expect(m.top, where).toBeGreaterThanOrEqual(m.headerBottom - 8);
    expect(m.hitInPanel, `the panel's lower edge is painted, not clipped: ${where}`).toBe(true);
    expect(m.right > m.groupLeft && m.left < m.groupRight, `panel overlaps its entry's column: ${where}`).toBe(true);
  });
}
