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

// A located page's title is frontmatter metadata, not rendered markup, so the
// body column's own box is the full-width reference: the article's content
// box, without its padding.
const CONTENT_BOX = `window.contentBox = (el) => {
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const left = r.left + parseFloat(cs.borderLeftWidth) + parseFloat(cs.paddingLeft);
  const right = r.right - parseFloat(cs.borderRightWidth) - parseFloat(cs.paddingRight);
  return { left, width: right - left };
};`;
test.beforeEach(async ({ page }) => {
  await page.addInitScript(CONTENT_BOX);
});
declare const contentBox: (el: Element) => { left: number; width: number };

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
    const column = contentBox(document.querySelector("article.container")!);
    return { blockWidth: block.getBoundingClientRect().width, columnWidth: column.width };
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

  const columnLeft = await page.locator("article.container").evaluate((el) => contentBox(el).left);
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

  const columnLeft = await page.locator("article.container").evaluate((el) => contentBox(el).left);
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

  const columnLeft = await page.locator("article.container").evaluate((el) => contentBox(el).left);
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
      const column = contentBox(document.querySelector("article.container")!);
      return { figure: figure.getBoundingClientRect().width, column: column.width };
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

// ── Vertical typesetting ────────────────────────────────────────────────────
// The locator is a block in the column flow, never a float: a float to the
// inline end of a vertical-rl column lands at the column's BOTTOM and reaches
// sideways across whatever follows. Judged on boxes, not on computed style, so
// any CSS that gets there passes. Both an article (meta columns before the
// text) and a front page (short body, then cards and a listing) must hold, at
// desktop and phone width.
for (const width of [1280, 390]) {
  for (const fixture of ["vertical-article", "vertical-front", "vertical-plate"]) {
    test(`vertical locator is an in-flow column block, top-aligned, overlapping nothing (${fixture}, ${width}px)`, async ({ page }) => {
      await page.setViewportSize({ width, height: 844 });
      await page.goto(`${fixture}/`, { waitUntil: "domcontentloaded" });
      await expect(page.locator("body")).toHaveAttribute("data-typesetting", "vertical");

      const facts = await page.evaluate((sel) => {
        const locator = document.querySelector(sel)!;
        const box = (el: Element) => {
          const r = el.getBoundingClientRect();
          return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
        };
        const siblings = [...locator.parentElement!.children]
          .filter((el) => el !== locator)
          .map((el) => ({ name: `${el.tagName.toLowerCase()}.${el.className}`, ...box(el) }))
          .filter((b) => b.right - b.left > 0 && b.bottom - b.top > 0);
        // The body (or the root) is whichever one scrolls; take the wider extent.
        const extents = [document.body, document.documentElement].map((el) => {
          const r = el.getBoundingClientRect();
          const sl = el.scrollLeft;
          return { left: Math.min(r.left, r.right - el.scrollWidth - sl), right: Math.max(r.right, r.right - sl) };
        });
        return {
          locator: box(locator),
          float: getComputedStyle(locator).float,
          siblings,
          extentLeft: Math.min(...extents.map((e) => e.left)),
          extentRight: Math.max(...extents.map((e) => e.right)),
        };
      }, LOCATOR);

      const L = facts.locator;
      for (const s of facts.siblings) {
        const overlaps = L.left < s.right - 0.5 && s.left < L.right - 0.5 && L.top < s.bottom - 0.5 && s.top < L.bottom - 0.5;
        expect(overlaps, `locator ${JSON.stringify(L)} overlaps sibling ${s.name} ${JSON.stringify(s)}`).toBe(false);
      }
      // Reading order: the opening text first (it is to the right in vertical-rl), the map after it.
      const opening = facts.siblings.find((sb) => sb.name.startsWith("p."))!;
      expect(L.right, `locator right ${L.right} vs. opening text left ${opening.left}: the map follows the text`).toBeLessThanOrEqual(opening.left + 2);
      const columnsTop = Math.min(...facts.siblings.map((s) => s.top));
      expect(Math.abs(L.top - columnsTop), `locator top ${L.top} vs. columns' top ${columnsTop}`).toBeLessThanOrEqual(2);
      expect(L.left, `locator left ${L.left} vs. scroll extent ${facts.extentLeft}`).toBeGreaterThanOrEqual(facts.extentLeft - 1);
      expect(L.right, `locator right ${L.right} vs. scroll extent ${facts.extentRight}`).toBeLessThanOrEqual(facts.extentRight + 1);
      expect(facts.float, "the locator is never floated in vertical typesetting").toBe("none");
    });
  }
}
