/**
 * Render gate: a masthead whose long name wraps ends row 2 at the end edge,
 * under the toggles, packs its links there, and wraps the name instead of
 * clipping it.
 *
 * Four links keep row 2 mostly empty, so spreading would show: the gaps would
 * grow past the normal link gap. The name shares row 1 only with the toggles,
 * so it wraps within the room they leave; few links then share its last line
 * at the end edge, across at least the name gap.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-split-pack.config.ts
 */

import { test, expect, type Page } from "../../support/browser-test";

/** Whether the links left row 1, and whether the name runs under the toggles or past the end edge. */
async function geometry(page: Page) {
  return page.evaluate(() => {
    const icons = document.querySelector(".nav-icons")!.getBoundingClientRect();
    const firstLink = document.querySelector(".nav-links a")!.getBoundingClientRect();
    const name = [...document.querySelector(".nav-left .site-name")!.getClientRects()];
    return {
      split: firstLink.top >= icons.bottom,
      nameUnderToggles: name.some((r) => r.right > icons.left && r.left < icons.right && r.bottom > icons.top && r.top < icons.bottom),
      nameRight: Math.max(...name.map((r) => r.right)),
      contentRight: document.querySelector(".nav-content")!.getBoundingClientRect().right,
    };
  });
}

/** Row 2 in inline coordinates (distance from the start edge), so one check reads LTR and RTL. */
async function rowTwo(page: Page) {
  return page.evaluate(() => {
    const rtl = getComputedStyle(document.documentElement).direction === "rtl";
    const content = document.querySelector(".nav-content") as HTMLElement;
    const c = content.getBoundingClientRect();
    const span = (r: DOMRect) => {
      const start = rtl ? c.right - r.right : r.left - c.left;
      return { start, end: start + r.width, top: r.top, bottom: r.bottom };
    };
    const icons = span(document.querySelector(".nav-icons")!.getBoundingClientRect());
    const links = [...document.querySelectorAll(".nav-links a")].map((a) => span(a.getBoundingClientRect()));
    const text = document.createRange();
    text.selectNodeContents(document.querySelector(".site-name")!);
    const glyphs = [...text.getClientRects()].map(span);
    const last = glyphs[glyphs.length - 1];
    return {
      split: links[0].top >= icons.bottom,
      linksEndToIconsEnd: Math.max(...links.map((l) => l.end)) - icons.end,
      sharesNameLine: links[0].top < last.bottom && links[0].bottom > last.top,
      nameToLinks: links[0].start - last.end,
      linkGap: links[1].start - links[0].end,
      iconsEnd: c.width - icons.end,
    };
  });
}

for (const dir of ["ltr", "rtl"]) {
  for (const width of [1280, 768]) {
    test(`a wrapped masthead ends row 2 at the end edge and packs the links at ${width}px, ${dir}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/", { waitUntil: "load" });
      await page.evaluate((d) => { document.documentElement.dir = d; }, dir);
      await page.evaluate(() => document.fonts.ready);
      const g = await rowTwo(page);
      const where = JSON.stringify(g);
      // Precondition: the geometry below only means something once the links leave row 1.
      expect(g.split, `the links must sit below the toggles at ${width}px: ${where}`).toBe(true);
      // Four links share the name's last line, a name gap from its tail, and end under the toggles' end.
      expect(g.sharesNameLine, where).toBe(true);
      expect(g.nameToLinks, where).toBeGreaterThanOrEqual(24);
      expect(Math.abs(g.linksEndToIconsEnd), where).toBeLessThanOrEqual(1);
      // The links are not spread: the gap between the first two is a normal gap.
      expect(g.linkGap, where).toBeLessThan(64);
      // The toggles end on the end edge.
      expect(Math.abs(g.iconsEnd), where).toBeLessThanOrEqual(4);
    });
  }
}

for (const width of [768, 390]) {
  test(`a long site name wraps instead of clipping at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const g = await geometry(page);
    expect(g.split, `masthead must split at ${width}px`).toBe(true);
    const where = JSON.stringify(g);
    // Clipped or overrun, the name would run under the toggles or past the edge.
    expect(g.nameUnderToggles, where).toBe(false);
    expect(g.nameRight, where).toBeLessThanOrEqual(g.contentRight);
  });
}
