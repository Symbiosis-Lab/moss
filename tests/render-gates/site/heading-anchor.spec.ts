/**
 * Copying a heading copies the heading, not "Heading#".
 *
 * See tests/e2e/helpers/gate-sites.ts → HEADING_ANCHOR_GATE for why this needs
 * a browser, and why it needs both of them.
 *
 * A page carries permalinks only when two of its sections hold 80 words each;
 * the fixture's pages are written to fire, and `/short/` to fall under it.
 * Every `##` section of a page that fires, the home page's included, carries
 * one; a level-1 title never does. The selection tests run against `/page/`;
 * the home page is checked separately.
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
    // this is what stops the fix above from being "delete the anchor".
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
    expect(painted).toBe('"\u2060#"');
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
        return { style: cs.outlineStyle, width: parseFloat(cs.outlineWidth), left: r.left, right: r.right };
      });
      expect(ring.style, `${width}px: a focus ring`).not.toBe("none");
      expect(ring.width).toBeGreaterThan(0);
      expect(ring.left, `${width}px: the mark's start is in the window`).toBeGreaterThanOrEqual(0);
      expect(ring.right, `${width}px: the mark's end is in the window`).toBeLessThanOrEqual(width);
      await anchor.blur();
    }
    await heading.hover();
    await expect(anchor).toHaveCSS("opacity", "1");
  });

  test("the mark trails the heading text on the same baseline, wrapped or not", async ({ page }) => {
    // A zero-size inline-block's bottom edge is its line's baseline, so a
    // probe after the heading's last word and one inside the permalink give
    // the two baselines without reading glyph metrics.
    const baselines = (selector: string) =>
      page.locator(selector).evaluateAll((headings) =>
        headings.map((h) => {
          const probe = () => {
            const p = document.createElement("span");
            p.style.cssText = "display:inline-block;width:0;height:0";
            return p;
          };
          const a = h.querySelector(".moss-heading-anchor")!;
          const text = probe();
          const mark = probe();
          h.insertBefore(text, a);
          a.insertBefore(mark, a.firstChild);
          const out = {
            text: text.getBoundingClientRect().bottom,
            mark: mark.getBoundingClientRect().bottom,
            markLeft: a.getBoundingClientRect().left,
            textRight: text.getBoundingClientRect().right,
          };
          text.remove();
          mark.remove();
          return out;
        }),
      );

    // A ruled heading, as a theme draws one: padding and a border above the text.
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/page/");
    await page.addStyleTag({
      content: "h2{padding-block-start:1.25rem;border-block-start:2px solid #888;line-height:1.4}",
    });
    for (const b of await baselines("h2#introduction")) {
      expect(Math.abs(b.mark - b.text), "ruled h2").toBeLessThanOrEqual(1);
      expect(b.markLeft, "the mark follows the text").toBeGreaterThanOrEqual(b.textRight);
    }

    // Headings that wrap on a phone: the mark stays on the last line.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/edge/");
    const wrapped = await baselines("article h2:has(> .moss-heading-anchor)");
    expect(wrapped.length).toBeGreaterThan(3);
    for (const b of wrapped) {
      expect(Math.abs(b.mark - b.text), "wrapped h2").toBeLessThanOrEqual(1);
    }
  });

  test("a CJK heading keeps its mark on the last line, beside the last character", async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 800 });
    await page.goto("/edge/");
    const heading = page.locator("article h2", { hasText: "港口" });
    const lines = await heading.evaluate((h) => {
      const a = h.querySelector(".moss-heading-anchor")!.getBoundingClientRect();
      const range = document.createRange();
      range.selectNodeContents(h.firstChild!);
      const text = [...range.getClientRects()];
      const last = text[text.length - 1];
      return { lines: new Set(text.map((r) => Math.round(r.top))).size, markTop: a.top, lastTop: last.top, lastBottom: last.bottom };
    });
    expect(lines.lines, "the heading wraps at 360px").toBeGreaterThan(1);
    expect(lines.markTop).toBeGreaterThanOrEqual(lines.lastTop - 1);
    expect(lines.markTop).toBeLessThanOrEqual(lines.lastBottom);
  });

  test("revealing the mark changes no heading's box", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/edge/");
    const boxes = () =>
      page.evaluate(() =>
        [...document.querySelectorAll("article h2")].map((h) => {
          const r = h.getBoundingClientRect();
          const a = h.querySelector(".moss-heading-anchor")!.getBoundingClientRect();
          return [r.width, r.height, a.left - r.left, a.top - r.top, a.width, a.height];
        }),
      );
    await page.mouse.move(0, 0);
    const rest = await boxes();
    const headings = page.locator("article h2");
    for (let i = 0; i < (await headings.count()); i++) {
      await headings.nth(i).hover();
      expect((await boxes())[i], `heading ${i} hovered`).toEqual(rest[i]);
    }
  });

  test("the mark's hit area is at least 24 by 24 px, whatever the glyph's size", async ({ page }) => {
    await page.goto("/page/");
    const anchor = page.locator("h2#introduction .moss-heading-anchor");
    const target = await anchor.evaluate((a) => {
      const cs = getComputedStyle(a, "::before");
      const r = a.getBoundingClientRect();
      const cx = r.left + r.width / 2;
      const cy = r.top + r.height / 2;
      const hit = (x: number, y: number) => document.elementFromPoint(x, y) === a;
      return {
        width: parseFloat(cs.width),
        height: parseFloat(cs.height),
        edges: [hit(cx - 11, cy), hit(cx + 11, cy), hit(cx, cy - 11), hit(cx, cy + 11)],
      };
    });
    expect(target.width).toBeGreaterThanOrEqual(24);
    expect(target.height).toBeGreaterThanOrEqual(24);
    expect(target.edges, "points 11px from the glyph's centre reach the link").toEqual([true, true, true, true]);
  });

  test("a page under the length rule has heading ids and no permalinks", async ({ page }) => {
    await page.goto("/short/");
    await expect(page.locator("h2#introduction")).toHaveCount(1);
    await expect(page.locator("h2#background")).toHaveCount(1);
    await expect(page.locator(".moss-heading-anchor")).toHaveCount(0);
  });

  test.describe("on a touch screen", () => {
    test.use({ hasTouch: true, isMobile: true });

    test("the mark is visible at rest, faintly", async ({ page }) => {
      await page.goto("/page/");
      expect(await page.evaluate(() => matchMedia("(hover: none)").matches)).toBe(true);
      const opacity = await page
        .locator("h2#introduction .moss-heading-anchor")
        .evaluate((a) => parseFloat(getComputedStyle(a).opacity));
      expect(opacity).toBeGreaterThan(0.2);
      expect(opacity).toBeLessThan(0.7);
    });
  });
});
