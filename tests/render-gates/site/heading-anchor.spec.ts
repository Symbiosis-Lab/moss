/**
 * Copying a heading copies the heading, not "Heading#".
 *
 * See tests/e2e/helpers/gate-sites.ts → HEADING_ANCHOR_GATE for why this needs
 * a browser, and why it needs both of them.
 *
 * d0b0be5f dropped the `#` permalink anchor from every home-page heading (at
 * any level), not just from level-1 titles — a reader reaches the home page
 * by visiting the site rather than by a deep link into one of its sections.
 * The selection/permalink tests below exercise an author-written `##`
 * heading that still gets an anchor, so they run against `/page/`, an
 * ordinary non-home page; the home page's own heading is checked separately,
 * for the absence the fix is about.
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

  test("the home page's heading carries no permalink at all", async ({
    page,
  }) => {
    // Locks d0b0be5f's home-page half: unlike `/page/` above, the home
    // page's own `##` heading gets no `.moss-heading-anchor`, at any level.
    await page.goto("/");

    const h = page.locator("h2#introduction");
    await expect(h).toHaveCount(1);
    await expect(
      page.locator("h2#introduction .moss-heading-anchor"),
    ).toHaveCount(0);
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
