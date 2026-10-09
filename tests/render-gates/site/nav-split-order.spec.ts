/**
 * Render gate: the split masthead keeps the toggles on the links' row.
 *
 * When the nav wraps (`data-nav-split`), row 1 is the site name and row 2
 * holds the links followed by the toggle cluster. Whether the toggles fit on
 * row 2 depends on the measured width of the link set, so the row placement
 * is asserted in a real engine at several widths.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-split-order.config.ts
 */

import { test, expect, type Page } from "@playwright/test";

interface Row {
  split: boolean;
  links: { top: number; bottom: number };
  icons: { top: number; bottom: number };
}

async function measure(page: Page): Promise<Row> {
  return page.evaluate(() => {
    const content = document.querySelector(".nav-content") as HTMLElement;
    const links = document.querySelector(".nav-links") as HTMLElement;
    const icons = document.querySelector(".nav-icons") as HTMLElement;
    const r = (el: Element) => {
      const b = el.getBoundingClientRect();
      return { top: b.top, bottom: b.bottom };
    };
    return {
      split: content.hasAttribute("data-nav-split"),
      links: r(links),
      icons: r(icons),
    };
  });
}

for (const width of [360, 500, 768, 1280]) {
  test(`toggles share the links' row at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/", { waitUntil: "load" });
    await page.evaluate(() => document.fonts.ready);
    const row = await measure(page);
    await page.screenshot({
      path: testInfo.outputPath(`nav-${width}.png`),
      clip: { x: 0, y: 0, width, height: 160 },
    });

    // The toggles must sit in the band the link block occupies. When the
    // links wrap to several lines the toggles centre on that block, so the
    // test is against the whole block: a third row puts the toggles entirely
    // below it, and the overlap fails.
    const sameRow = row.icons.top < row.links.bottom && row.links.top < row.icons.bottom;
    expect(
      sameRow,
      `split=${row.split} links=[${row.links.top},${row.links.bottom}] ` +
        `icons=[${row.icons.top},${row.icons.bottom}]`,
    ).toBe(true);
  });
}

test("the split engages on a narrow masthead", async ({ page }) => {
  // Precondition: the fixture must actually exercise the split, or the
  // row assertions above would pass on a single-row nav.
  await page.setViewportSize({ width: 360, height: 900 });
  await page.goto("/", { waitUntil: "load" });
  const row = await measure(page);
  expect(row.split, "fixture must wrap the masthead at 360px").toBe(true);
});
