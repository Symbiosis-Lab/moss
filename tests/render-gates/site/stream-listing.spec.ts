/**
 * `style:stream` draws each post whole as an <article>, inside the page's own
 * <article>. The page article's measure and end margin must not reach the
 * posts: the space between two posts is the hairline's padding on each side,
 * and the listing ends on the last post's date.
 */
import { test, expect } from "@playwright/test";

const PHONE = { width: 390, height: 844 };
const WIDE = { width: 1440, height: 900 };

for (const [name, viewport] of [["phone", PHONE], ["wide", WIDE]] as const) {
  test.describe(`stream listing at ${name} width`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize(viewport);
    });

    test("the More link under a stream sits at its own margin below the last date", async ({ page }) => {
      await page.goto("/");
      const gap = await page.evaluate(() => {
        const date = document.querySelector(".moss-stream-item:last-child .moss-stream-date")!.getBoundingClientRect();
        const more = document.querySelector(".moss-stream + .moss-embed-more")!;
        return { space: more.getBoundingClientRect().top - date.bottom, margin: parseFloat(getComputedStyle(more).marginTop) };
      });
      expect(Math.abs(gap.space - gap.margin)).toBeLessThanOrEqual(1);
    });

    test("the hairline between two posts has the same space on each side", async ({ page }) => {
      await page.goto("/notes/");
      const sides = await page.evaluate(() => {
        const [first, second] = document.querySelectorAll(".moss-stream-item");
        const hairline = second.getBoundingClientRect().top;
        const above = hairline - first.querySelector(".moss-stream-date")!.getBoundingClientRect().bottom;
        const below = second.querySelector(".moss-stream-body")!.getBoundingClientRect().top - hairline - 1;
        return { above, below };
      });
      expect(sides.below).toBeGreaterThan(0);
      expect(Math.abs(sides.above - sides.below)).toBeLessThanOrEqual(1);
    });

    test("a post is as wide as the stream it sits in", async ({ page }) => {
      await page.goto("/notes/");
      const widths = await page.evaluate(() => ({
        stream: document.querySelector(".moss-stream")!.getBoundingClientRect().width,
        item: document.querySelector(".moss-stream-item")!.getBoundingClientRect().width,
      }));
      expect(Math.abs(widths.stream - widths.item)).toBeLessThanOrEqual(1);
    });
  });
}
