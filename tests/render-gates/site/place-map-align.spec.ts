/**
 * The article locator floats right, positioned immediately before the
 * body's first TEXT block (a paragraph, list or blockquote) rather than
 * always right after the masthead. A leading heading, rule or figure stays
 * above it at full column width — it comes first in the DOM, so nothing has
 * to clear it out of the way — and the map's top edge tracks the top of the
 * text block it actually sits beside.
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

/** The FIRST match's own top edge — what the margin behaviour this gate
 * proves actually controls. A Range over the first text node would read a
 * few px lower still, inside the glyph box past the line's own half-leading;
 * that offset is set by the block's font metrics (line height vs. font
 * size), not by anything this fix touches. */
async function blockTop(page: import("@playwright/test").Page, selector: string): Promise<number> {
  return page.locator(selector).first().evaluate((el) => el.getBoundingClientRect().top);
}

async function fullColumnWidth(page: import("@playwright/test").Page, selector: string): Promise<void> {
  const { blockWidth, columnWidth } = await page.evaluate((sel) => {
    const block = document.querySelector(sel)!;
    const column = document.querySelector("article.container > h1")!;
    return { blockWidth: block.getBoundingClientRect().width, columnWidth: column.getBoundingClientRect().width };
  }, selector);
  expect(
    Math.abs(blockWidth - columnWidth),
    `${selector} width ${blockWidth} vs. column width ${columnWidth} — it must not be narrowed by the float`,
  ).toBeLessThanOrEqual(1);
}

test("a heading-first body keeps the heading full width and aligns the map with the paragraph below it", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("heading-first/", { waitUntil: "domcontentloaded" });

  await expect(page.locator("article.container > h2").first()).toHaveText("Where the story begins");
  await fullColumnWidth(page, "article.container > h2");

  const paragraph = page.locator("article.container > p").first();
  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const paragraphTop = await blockTop(page, "article.container > p");
  expect(
    Math.abs(mapTop - paragraphTop),
    `map top ${mapTop} vs. paragraph top ${paragraphTop}`,
  ).toBeLessThanOrEqual(2);

  const columnLeft = await page.locator("article.container > h1").evaluate((el) => el.getBoundingClientRect().left);
  const paragraphLeft = await paragraph.evaluate((el) => el.getBoundingClientRect().left);
  expect(
    Math.abs(columnLeft - paragraphLeft),
    `paragraph left ${paragraphLeft} vs. column left ${columnLeft} — it must flow beside the locator, not under it`,
  ).toBeLessThanOrEqual(1);

  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const paragraphBox = await paragraph.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the paragraph text").toBeGreaterThan(paragraphBox.left);
  expect(locatorBox.top, "locator top must be at or above the paragraph's bottom (they overlap vertically)").toBeLessThan(paragraphBox.bottom);

  // The heading precedes the locator in the DOM, so it is never narrowed by
  // the float in the first place — confirming that directly, rather than
  // just inferring it from the width check above.
  const headingBottom = await page.locator("article.container > h2").first().evaluate((el) => el.getBoundingClientRect().bottom);
  expect(headingBottom, `heading bottom ${headingBottom} vs. map top ${mapTop}`).toBeLessThanOrEqual(mapTop);
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

test("a list-first body flows beside the locator, top-aligned with its text", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("list-first/", { waitUntil: "domcontentloaded" });

  const firstBlock = page.locator("article.container > ul").first();

  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const listTop = await blockTop(page, "article.container > ul");
  expect(
    Math.abs(mapTop - listTop),
    `map top ${mapTop} vs. list top ${listTop}`,
  ).toBeLessThanOrEqual(2);

  const columnLeft = await page.locator("article.container > h1").evaluate((el) => el.getBoundingClientRect().left);
  const listLeft = await firstBlock.evaluate((el) => el.getBoundingClientRect().left);
  expect(
    Math.abs(columnLeft - listLeft),
    `list left ${listLeft} vs. column left ${columnLeft} — it must flow beside the locator, not under it`,
  ).toBeLessThanOrEqual(1);

  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const listBox = await firstBlock.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the list").toBeGreaterThan(listBox.left);
  expect(locatorBox.top, "locator top must be at or above the list's bottom (they overlap vertically)").toBeLessThan(listBox.bottom);
});

test("a quote-first body flows beside the locator, top-aligned with its text", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("quote-first/", { waitUntil: "domcontentloaded" });

  const firstBlock = page.locator("article.container > blockquote").first();

  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const quoteTop = await blockTop(page, "article.container > blockquote");
  expect(
    Math.abs(mapTop - quoteTop),
    `map top ${mapTop} vs. blockquote top ${quoteTop}`,
  ).toBeLessThanOrEqual(2);

  // Not a left-edge-vs-column check here, unlike the paragraph/list cases: a
  // blockquote carries its own border + padding-inline-start regardless of
  // any float, so its left edge is never flush with the column — that's true
  // whether or not it sits beside the locator, so it would not tell the two
  // cases apart. Nor is a right-edge/narrowing check: `getBoundingClientRect`
  // on a normal block returns its full border-box width even when a float
  // beside it shortens only the LINE boxes inside — true of the paragraph
  // and list cases too, not something particular to blockquote. What is
  // particular here, and worth checking, is vertical overlap with the
  // locator, on top of the top-edge alignment already checked above.
  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const quoteBox = await firstBlock.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the blockquote").toBeGreaterThan(quoteBox.left);
  expect(locatorBox.top, "locator top must be at or above the blockquote's bottom (they overlap vertically)").toBeLessThan(quoteBox.bottom);
});

test("a media-first body keeps the figure full width and aligns the map with the paragraph below it", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("media-first/", { waitUntil: "domcontentloaded" });

  await fullColumnWidth(page, "article.container > figure");

  const paragraph = page.locator("article.container > p").first();
  const mapTop = await page.locator(MAP).evaluate((el) => el.getBoundingClientRect().top);
  const paragraphTop = await blockTop(page, "article.container > p");
  expect(
    Math.abs(mapTop - paragraphTop),
    `map top ${mapTop} vs. paragraph top ${paragraphTop}`,
  ).toBeLessThanOrEqual(2);

  const figureBottom = await page.locator("article.container > figure").first().evaluate((el) => el.getBoundingClientRect().bottom);
  expect(figureBottom, `figure bottom ${figureBottom} vs. map top ${mapTop}`).toBeLessThanOrEqual(mapTop);

  const locatorBox = await page.locator(LOCATOR).evaluate((el) => el.getBoundingClientRect());
  const paragraphBox = await paragraph.evaluate((el) => el.getBoundingClientRect());
  expect(locatorBox.left, "locator must sit to the right of the paragraph text").toBeGreaterThan(paragraphBox.left);
  expect(locatorBox.top, "locator top must be at or above the paragraph's bottom (they overlap vertically)").toBeLessThan(paragraphBox.bottom);
});

test("a body with no text block keeps the locator at the front, above the now full-width heading", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("no-text/", { waitUntil: "domcontentloaded" });

  // Nothing for the locator to sit beside, so the base clear:both rule
  // (unconditional now — the fix this gate covers removed its one
  // exception) pushes the heading below the float instead.
  const { locatorBottom, headingTop } = await page.evaluate(() => {
    const locator = document.querySelector(".moss-place-locator")!;
    const heading = document.querySelector("article.container > h2")!;
    return { locatorBottom: locator.getBoundingClientRect().bottom, headingTop: heading.getBoundingClientRect().top };
  });
  expect(locatorBottom, `locator bottom ${locatorBottom} vs. heading top ${headingTop}`).toBeLessThanOrEqual(headingTop);
  await fullColumnWidth(page, "article.container > h2");
});

for (const fixture of ["heading-first", "paragraph-first", "media-first"]) {
  test(`the locator runs the full column width, above the text, below the mobile breakpoint (${fixture})`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`${fixture}/`, { waitUntil: "domcontentloaded" });

    const widths = await page.evaluate(() => {
      const figure = document.querySelector(".moss-place-map")!;
      const column = document.querySelector("article.container > h1")!;
      return { figure: figure.getBoundingClientRect().width, column: column.getBoundingClientRect().width };
    });
    expect(widths.figure / widths.column, `map ${widths.figure}px vs. column ${widths.column}px`).toBeCloseTo(1, 1);

    const { mapBottom, paragraphTop } = await page.evaluate(() => {
      const figure = document.querySelector(".moss-place-map")!;
      const paragraph = document.querySelector("article.container > p")!;
      return {
        mapBottom: figure.getBoundingClientRect().bottom,
        paragraphTop: paragraph.getBoundingClientRect().top,
      };
    });
    expect(mapBottom, `map bottom ${mapBottom} vs. paragraph top ${paragraphTop} — the map must sit above the text on mobile`).toBeLessThanOrEqual(paragraphTop);
  });
}
