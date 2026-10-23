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

  test("the permalink adds no sideways scroll and moves no line, hidden, on hover and on focus", async ({
    page,
  }) => {
    // Headings that end in a year range at a large size fill their last line at
    // some width, so sweep phone widths rather than trusting two lucky ones.
    await page.goto("/edge/");
    await page.addStyleTag({ content: "h2 { font-size: 2.6rem; }" });
    const heading = page.locator("h2").nth(1);
    const anchor = heading.locator(".moss-heading-anchor");
    const state = () =>
      page.evaluate(() => ({
        scroll: document.documentElement.scrollWidth,
        inner: window.innerWidth,
        height: document.querySelectorAll("h2")[1].getBoundingClientRect().height,
      }));
    for (let width = 340; width <= 420; width += 1) {
      await page.setViewportSize({ width, height: 800 });
      await page.mouse.move(0, 0);
      const rest = await state();
      expect(rest.scroll, `${width}px at rest`).toBeLessThanOrEqual(rest.inner);

      await heading.hover();
      const hovered = await state();
      expect(hovered.scroll, `${width}px hovered`).toBeLessThanOrEqual(hovered.inner);
      expect(hovered.height, `${width}px: hover moves no line`).toBe(rest.height);

      await page.mouse.move(0, 0);
      await page.keyboard.press("Tab");
      await anchor.focus();
      const focused = await state();
      expect(focused.scroll, `${width}px focused`).toBeLessThanOrEqual(focused.inner);
      expect(focused.height, `${width}px: focus moves no line`).toBe(rest.height);
      const ring = await anchor.evaluate((a) => {
        const cs = getComputedStyle(a);
        const r = a.getBoundingClientRect();
        return { style: cs.outlineStyle, width: parseFloat(cs.outlineWidth), offset: parseFloat(cs.outlineOffset), left: r.left, right: r.right };
      });
      expect(ring.style, `${width}px: a focus ring`).not.toBe("none");
      expect(ring.width).toBeGreaterThan(0);
      expect(ring.offset, `${width}px: the ring is inset`).toBeLessThan(0);
      expect(ring.left, `${width}px: the mark's start is in the window`).toBeGreaterThanOrEqual(0);
      expect(ring.right, `${width}px: the mark's end is in the window`).toBeLessThanOrEqual(width);
      await anchor.blur();
    }
    await heading.hover();
    await expect(anchor).toHaveCSS("opacity", "1");
  });
});
