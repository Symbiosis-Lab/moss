/**
 * The article locator floats right, but the article body must not be
 * stranded under it: the locator's top edge tracks the top of the body's
 * first block — heading or paragraph — and that block flows beside the
 * float, on the left, rather than dropping under it.
 *
 * `article.container > h2/h3/hr { clear: both }` used to fire on the block
 * immediately after the locator too, which is what stranded it: the
 * heading cleared the float instead of running beside it, leaving the map
 * alone at the right with the body starting under it.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACE_MAP_ALIGN_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/place-map-align.config.ts
 */
import { test, expect } from "@playwright/test";

const LOCATOR = ".moss-place-locator";
const MAP = ".moss-place-map";

/** The FIRST match's own top edge — what the margin-matching CSS this
 * gate proves actually controls. A Range over the first text node would
 * read a few px lower still, inside the glyph box past the line's own
 * half-leading; that offset is set by the block's font metrics (line
 * height vs. font size), not by any margin this fix touches, and differs
 * between the heading and paragraph fixtures for that reason alone. */
async function blockTop(page: import("@playwright/test").Page, selector: string): Promise<number> {
  return page.locator(selector).first().evaluate((el) => el.getBoundingClientRect().top);
}

test("a heading-first body flows beside the locator, top-aligned with its text", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("heading-first/", { waitUntil: "domcontentloaded" });

  const firstBlock = page.locator("article.container > h2").first();
  await expect(firstBlock).toHaveText("Where the story begins");

  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const headingTop = await blockTop(page, "article.container > h2");
  expect(
    Math.abs(mapTop - headingTop),
    `map top ${mapTop} vs. heading top ${headingTop}`,
  ).toBeLessThanOrEqual(2);

  const columnLeft = await page.locator("article.container > h1").evaluate((el) => el.getBoundingClientRect().left);
  const headingLeft = await firstBlock.evaluate((el) => el.getBoundingClientRect().left);
  expect(
    Math.abs(columnLeft - headingLeft),
    `heading left ${headingLeft} vs. column left ${columnLeft} — it must flow beside the locator, not under it`,
  ).toBeLessThanOrEqual(1);

  // The locator must genuinely be beside the heading, not merely aligned by
  // coincidence while sitting elsewhere: its left edge is right of the
  // heading's, and it overlaps the heading's vertical span.
  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const headingBox = await firstBlock.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the heading text").toBeGreaterThan(headingBox.left);
  expect(locatorBox.top, "locator top must be at or above the heading's bottom (they overlap vertically)").toBeLessThan(headingBox.bottom);

  // A second `##`, past real prose, is not the locator's immediate sibling
  // and must still clear the float — the exception is scoped to exactly one
  // block, not every heading in the article.
  const laterHeading = page.locator("article.container > h2").nth(1);
  await expect(laterHeading).toHaveText("A later heading");
  expect(await laterHeading.evaluate((el) => getComputedStyle(el).clear)).toBe("both");
});

test("a paragraph-first body flows beside the locator, top-aligned with its text", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("paragraph-first/", { waitUntil: "domcontentloaded" });

  const firstBlock = page.locator("article.container > p").first();

  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const paragraphTop = await blockTop(page, "article.container > p");
  expect(
    Math.abs(mapTop - paragraphTop),
    `map top ${mapTop} vs. paragraph top ${paragraphTop}`,
  ).toBeLessThanOrEqual(2);

  const columnLeft = await page.locator("article.container > h1").evaluate((el) => el.getBoundingClientRect().left);
  const paragraphLeft = await firstBlock.evaluate((el) => el.getBoundingClientRect().left);
  expect(
    Math.abs(columnLeft - paragraphLeft),
    `paragraph left ${paragraphLeft} vs. column left ${columnLeft} — it must flow beside the locator, not under it`,
  ).toBeLessThanOrEqual(1);

  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const paragraphBox = await firstBlock.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the paragraph text").toBeGreaterThan(paragraphBox.left);
  expect(locatorBox.top, "locator top must be at or above the paragraph's bottom (they overlap vertically)").toBeLessThan(paragraphBox.bottom);
});

test("an hr-first body flows beside the locator, top-aligned with its own top edge", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("hr-first/", { waitUntil: "domcontentloaded" });

  const firstBlock = page.locator("article.container > hr").first();

  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const hrTop = await blockTop(page, "article.container > hr");
  expect(
    Math.abs(mapTop - hrTop),
    `map top ${mapTop} vs. hr top ${hrTop}`,
  ).toBeLessThanOrEqual(2);

  const columnLeft = await page.locator("article.container > h1").evaluate((el) => el.getBoundingClientRect().left);
  const hrLeft = await firstBlock.evaluate((el) => el.getBoundingClientRect().left);
  expect(
    Math.abs(columnLeft - hrLeft),
    `hr left ${hrLeft} vs. column left ${columnLeft} — it must flow beside the locator, not under it`,
  ).toBeLessThanOrEqual(1);

  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const hrBox = await firstBlock.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the hr").toBeGreaterThan(hrBox.left);
  expect(locatorBox.top, "locator top must be at or above the hr's bottom (they overlap vertically)").toBeLessThan(hrBox.bottom);
});

for (const fixture of ["heading-first", "paragraph-first"]) {
  test(`the locator runs the full column width, above the text, below the mobile breakpoint (${fixture})`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`${fixture}/`, { waitUntil: "domcontentloaded" });

    const widths = await page.evaluate(() => {
      const figure = document.querySelector(".moss-place-map")!;
      const column = document.querySelector("article.container > h1")!;
      return { figure: figure.getBoundingClientRect().width, column: column.getBoundingClientRect().width };
    });
    expect(widths.figure / widths.column, `map ${widths.figure}px vs. column ${widths.column}px`).toBeCloseTo(1, 1);

    const { mapBottom, firstBlockTop } = await page.evaluate(() => {
      const figure = document.querySelector(".moss-place-map")!;
      const firstBlock = document.querySelector("article.container > :is(h2, p)")!;
      return {
        mapBottom: figure.getBoundingClientRect().bottom,
        firstBlockTop: firstBlock.getBoundingClientRect().top,
      };
    });
    expect(mapBottom, `map bottom ${mapBottom} vs. first block top ${firstBlockTop} — the map must sit above the text on mobile`).toBeLessThanOrEqual(firstBlockTop);
  });
}
