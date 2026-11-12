/**
 * The series nav's direction mark reads as part of its "Previous"/"Next"
 * label: a clear gap from the label's words, centred on the label's line, and
 * no taller than it, so the link's box starts where its text does instead of
 * standing above its neighbours' text line.
 */
import { test, expect, type Page } from "@playwright/test";

/** The mark's gap needs to read as a space between words at least. */
const MIN_GAP = 8;

async function sides(page: Page) {
  return page.evaluate(() => {
    const glyphs = (el: Element) => {
      const r = document.createRange();
      r.selectNodeContents(el);
      return r.getBoundingClientRect();
    };
    return [...document.querySelectorAll("a.moss-series-nav-link")].map((a) => {
      const arrow = a.querySelector(".moss-series-nav-arrow")!;
      const label = a.querySelector(".moss-series-nav-label")!;
      const text = a.querySelector(".moss-series-nav-text")!;
      const [arrowInk, labelInk] = [glyphs(arrow), glyphs(label)];
      const prev = a.classList.contains("moss-series-nav-prev");
      const titleInk = glyphs(a.querySelector(".moss-series-nav-title")!);
      return {
        side: prev ? "prev" : "next",
        gap: prev ? labelInk.left - arrowInk.right : arrowInk.left - Math.max(labelInk.right, titleInk.right),
        centreOff: Math.abs((arrowInk.top + arrowInk.bottom) / 2 - (labelInk.top + labelInk.bottom) / 2),
        textBelowLinkTop: text.getBoundingClientRect().top - a.getBoundingClientRect().top,
      };
    });
  });
}

for (const path of ["/course/one/", "/course/two/", "/course/three/"]) {
  for (const width of [390, 1280]) {
    test(`${path} at ${width}px: the direction mark sits in its label's line`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(path);
      const found = await sides(page);
      expect(found.length).toBeGreaterThan(0);
      for (const s of found) {
        expect(s.gap, `${s.side}: gap between the mark and the label`).toBeGreaterThanOrEqual(MIN_GAP);
        expect(s.centreOff, `${s.side}: mark off the label's centre line`).toBeLessThanOrEqual(1);
        expect(s.textBelowLinkTop, `${s.side}: link box above its text`).toBeLessThanOrEqual(1);
      }
    });
  }
}
