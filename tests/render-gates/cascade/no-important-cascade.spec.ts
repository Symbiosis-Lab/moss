/**
 * Render gate for Task 2.2: user CSS wins without !important.
 *
 * Both rules below once needed `!important` to beat moss. They now sit in
 * `@layer themes`, which outranks `@layer shortcodes` on layer order alone.
 *
 * Runs against the gate-test.html probe the fixture writes into the built
 * output — it needs markup moss would not emit on its own. The scratch site
 * comes from tests/e2e/helpers/gate-sites.ts, built by the playwright config at
 * parse time and served by its webServer.
 *
 * Run via:
 *   npx playwright test -c playwright/no-important-cascade.config.ts
 */
import { test, expect } from "@playwright/test";

const EXPECTED_READ_MORE_COLOR = "rgb(0, 0, 255)";

// One render, both rules: same page, same 390px viewport, same cascade
// resolution. They were two tests, which meant two navigations for one answer.
test("user rules beat moss rules at mobile width with no !important", async ({
  page,
}) => {
  // 390px is below the 768px breakpoint where moss collapses the grid to 1fr.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("gate-test.html", { waitUntil: "domcontentloaded" });

  const computed = () =>
    page.evaluate(() => {
      const grid = document.querySelector("article footer .moss-grid");
      const readMore = document.querySelector(".read-more");
      return {
        gridColumns: grid ? window.getComputedStyle(grid).gridTemplateColumns : null,
        readMoreColor: readMore ? window.getComputedStyle(readMore).color : null,
      };
    });

  // `domcontentloaded` does not guarantee both `<link rel="stylesheet">` tags
  // are loaded and applied, and it is the second one (layer="themes") that
  // carries both rules this gate checks. Poll the actual cascade result
  // instead of reading it once, so a still-loading stylesheet is never
  // mistaken for a resolved cascade, while a real regression still fails once
  // the poll times out.
  await expect
    .poll(async () => (await computed()).readMoreColor, {
      message: "the user's .read-more colour must beat moss's accent without !important",
    })
    .toBe(EXPECTED_READ_MORE_COLOR);

  const seen = await computed();

  // Two space-separated resolved lengths. If moss's mobile 1fr had won we would
  // see a single track.
  const trackCount = seen.gridColumns ? seen.gridColumns.trim().split(/\s+/).length : 0;
  expect(
    trackCount,
    "the user's two-track grid rule must beat moss's mobile collapse, " +
      `but grid-template-columns resolved to "${seen.gridColumns}"`,
  ).toBe(2);

  expect(
    seen.readMoreColor,
    "the user's .read-more colour must beat moss's accent without !important",
  ).toBe(EXPECTED_READ_MORE_COLOR);
});
