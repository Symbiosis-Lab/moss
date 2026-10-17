/**
 * Copying a heading copies the heading, not "Heading#".
 *
 * See tests/e2e/helpers/gate-sites.ts → HEADING_ANCHOR_GATE for why this needs
 * a browser, and why it needs both of them.
 *
 * A home-page `##` section carries the same permalink as any page's, so a
 * site owner can link to a section of the home page; only a level-1 title
 * goes without one. The selection tests run against `/page/`; the home page
 * is checked separately.
 */
import { test, expect } from "@playwright/test";

test.describe("heading anchor", () => {
  test("selecting a heading does not select its permalink", async ({
    page,
  }) => {
    await page.goto("/page/");

    const selected = await page.evaluate(() => {
      const h = document.querySelector<HTMLElement>("h2#introduction")!;
      const range = document.createRange();
      range.selectNodeContents(h);
      const sel = getSelection()!;
      sel.removeAllRanges();
      sel.addRange(range);
      return sel.toString();
    });

    // The whole point. Before the glyph moved into `::after` this read
    // "Introduction#" in WebKit.
    expect(selected.trim()).toBe("Introduction");
    expect(selected).not.toContain("#");
  });

  test("a drag across the heading and the body below it copies neither", async ({
    page,
  }) => {
    // selectNodeContents above is the tidy case. A reader drags, and a drag
    // that starts above the heading and ends in the paragraph produces a range
    // the engine serializes differently — this is the case the original
    // `user-select: none` was written for and the one WebKit got wrong.
    await page.goto("/page/");

    const selected = await page.evaluate(() => {
      const h = document.querySelector<HTMLElement>("h2#introduction")!;
      const p = h.nextElementSibling!;
      const range = document.createRange();
      range.setStartBefore(h);
      range.setEndAfter(p);
      const sel = getSelection()!;
      sel.removeAllRanges();
      sel.addRange(range);
      return sel.toString();
    });

    expect(selected).toContain("Introduction");
    expect(selected).not.toContain("#");
  });

  test("the permalink is still there, and still paints a #", async ({
    page,
  }) => {
    // Removing the glyph from the document must not remove the affordance —
    // this is what stops the fix above from being "delete the anchor". A
    // non-home page, not the home page: d0b0be5f strips the anchor from
    // every home-page heading, so this claim only holds elsewhere.
    await page.goto("/page/");

    const anchor = page.locator("h2#introduction .moss-heading-anchor");
    await expect(anchor).toHaveCount(1);
    await expect(anchor).toHaveAttribute("href", "#introduction");

    const painted = await page.evaluate(() =>
      getComputedStyle(
        document.querySelector("h2#introduction .moss-heading-anchor")!,
        "::after",
      ).content,
    );
    expect(painted).toBe('"#"');
  });

  test("a home-page section carries a permalink, like any page's", async ({
    page,
  }) => {
    await page.goto("/");

    const h = page.locator("h2#introduction");
    await expect(h).toHaveCount(1);
    await expect(
      page.locator("h2#introduction .moss-heading-anchor"),
    ).toHaveCount(1);
  });

  test("a grid cell's heading is a card title and carries no permalink", async ({
    page,
  }) => {
    await page.goto("/");

    const cellHeadings = page.locator(".moss-grid-card h3");
    await expect(cellHeadings).toHaveCount(2);
    await expect(
      page.locator(".moss-grid-card .moss-heading-anchor"),
    ).toHaveCount(0);
  });
});
