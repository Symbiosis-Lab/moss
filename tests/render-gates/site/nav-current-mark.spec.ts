/**
 * Render gate: the current page's nav link carries a 2px rule under its text,
 * and that rule must not lift the text off the line its siblings share.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-current-mark.config.ts
 */

import { test, expect } from "@playwright/test";

const THEME_MARK = ".nav-links a.active { border-block-end: 0; text-decoration: underline; }";

for (const width of [390, 1280]) {
  for (const theme of [false, true]) {
    test(`every nav link's text shares one line, current page included, at ${width}px${theme ? " with a theme's own mark" : ""}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 844 });
      await page.goto("/pears/", { waitUntil: "load" });
      if (theme) await page.addStyleTag({ content: THEME_MARK });
      await page.evaluate(() => document.fonts.ready);
      const tops = await page.evaluate(() =>
        [...document.querySelectorAll<HTMLElement>(".nav-links a")].map((a) => {
          const range = document.createRange();
          range.selectNodeContents(a);
          return { text: a.textContent, active: a.classList.contains("active"), top: range.getBoundingClientRect().top };
        }),
      );
      expect(tops.filter((t) => t.active).length, "one link is current").toBe(1);
      expect(tops.length).toBeGreaterThanOrEqual(3);
      for (const t of tops) {
        expect(Math.abs(t.top - tops[0].top), `${t.text} text top vs ${tops[0].text}: ${JSON.stringify(tops)}`).toBeLessThanOrEqual(0.5);
      }
    });
  }
}
