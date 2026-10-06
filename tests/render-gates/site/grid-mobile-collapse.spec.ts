/**
 * Horizontal grids collapse on phones; vertical grids retain their authored
 * track count because their inline axis follows viewport height. Ratios must
 * remain theme-overridable, and both image syntaxes must fill the same box.
 */
import { test, expect, type Page } from "@playwright/test";

const MOBILE = { width: 390, height: 844 };
const DESKTOP = { width: 1280, height: 900 };

/** Track widths as laid out, read off the computed value. */
async function tracks(page: Page, selector: string): Promise<number[]> {
  return page.$eval(selector, (el: Element) =>
    getComputedStyle(el)
      .gridTemplateColumns.split(" ")
      .filter(Boolean)
      .map((t) => parseFloat(t)),
  );
}

test.describe("grid mobile collapse", () => {
  for (const vertical of [false, true]) {
    for (const width of [390, 740, 800, 1280]) {
      test(`${vertical ? "vertical" : "horizontal"} grids follow the reading axis at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(vertical ? "/vertical/" : "/");
        const collapsed = !vertical && width <= 768;
        expect(await tracks(page, '.moss-grid[data-columns="3"]')).toHaveLength(collapsed ? 1 : 3);
        const ratio = await tracks(page, '.moss-grid[data-columns="2"]:not(.parity)');
        expect(ratio).toHaveLength(collapsed ? 1 : 2);
        if (!collapsed) expect(ratio[1] / ratio[0]).toBeCloseTo(2, 1);
      });
    }
  }

  test("both image syntaxes fill the cell identically on mobile", async ({ page }) => {
    await page.setViewportSize(MOBILE);
    await page.goto("/");

    const boxes = await page.$eval(".moss-grid.parity", (grid) =>
      [...grid.children].map((cell) => {
        const img = cell.querySelector("img") as HTMLImageElement;
        const cellBox = cell.getBoundingClientRect();
        const imgBox = img.getBoundingClientRect();
        return {
          // The wrapper a theme can see. With `implicit_figure = false` both
          // cells must be plain — neither spelling gets a `.moss-image` hook
          // the other lacks.
          hasFigure: !!cell.querySelector("figure.moss-image"),
          cell: cellBox.width,
          img: imgBox.width,
          // Compare each image with its own cell, stacked or side by side.
          leftInCell: imgBox.left - cellBox.left,
        };
      }),
    );

    expect(boxes).toHaveLength(2);
    expect(boxes.map((b) => b.hasFigure)).toEqual([false, false]);
    for (const b of boxes) {
      expect(b.img).toBeCloseTo(b.cell, 0);
      expect(b.leftInCell).toBeCloseTo(0, 0);
    }
    expect(boxes[0].img).toBeCloseTo(boxes[1].img, 0);
  });

  async function cardGeometry(page: Page, selector: string) {
    return page.locator(selector).first().evaluate(async (card) => {
      await Promise.all([...card.querySelectorAll('img')].map((img) => img.decode()));
      const rect = (el: Element) => el.getBoundingClientRect().toJSON();
      return {
        cardBox: rect(card),
        coverBox: rect(card.querySelector('.moss-card-cover')!),
        contentBox: rect(card.querySelector('.moss-card-content')!),
      };
    });
  }

  /** Cover at the card's block-start, band at block-end, one shared rule —
   * checked identically at 390px and at 1280px, because nothing about the
   * COMPOSITION (this box-arithmetic) is a function of viewport width. The
   * cover's own aspect-ratio is a separate claim: on the generated LISTING
   * grid it IS width-dependent (site.css's `@container (max-width: 36rem)`
   * override, research-backed — 390px is narrow enough to trigger it (36rem
   * = 576px), 1280px is not), but on a hand-picked `:::grid` cover it stays
   * the shared landscape default at every width (b47a55ed, 2026-09-25: an
   * author's own choice of image is not the generated listing's uniform
   * box). `coverFollowsWidth` selects which claim this call is checking. */
  async function assertCardComposition(page: Page, selector: string, coverFollowsWidth: boolean) {
    for (const viewport of [MOBILE, DESKTOP]) {
      await page.setViewportSize(viewport);
      const { cardBox, coverBox, contentBox } = await cardGeometry(page, selector);
      expect(coverBox.y + coverBox.height, "cover should sit above content, not beside or inside it")
        .toBeLessThanOrEqual(contentBox.y + 1);
      expect(Math.abs(coverBox.width - cardBox.width), "cover should run the card's full width, not a fixed thumbnail")
        .toBeLessThanOrEqual(1);
      expect(Math.abs(contentBox.width - cardBox.width), "content should run the card's full width, not a fixed sidebar")
        .toBeLessThanOrEqual(1);
      expect(Math.abs(coverBox.height + contentBox.height - cardBox.height), "cover + content should account for the card's full height, not leave a gap or overlap")
        .toBeLessThanOrEqual(1);
      // inline:block = 4:3 above 36rem (width:height under horizontal-tb, a
      // landscape plate), 3/4 below it (portrait — the phone-width
      // research) only when `coverFollowsWidth`; a `:::grid` cover ignores
      // the narrow-container override and stays 4/3 at both widths.
      const expectedRatio = coverFollowsWidth && viewport === MOBILE ? 3 / 4 : 4 / 3;
      expect(coverBox.width / coverBox.height).toBeCloseTo(expectedRatio, 1);
    }
  }

  test("a covered grid card is a column at 390px and at 1280px, same composition at both", async ({
    page,
  }) => {
    await page.goto("/rows/");
    await assertCardComposition(page, '.moss-cards[data-layout="grid"] .moss-card', true);
  });

  test("a :::grid link card (lone wikilinks to covered pages) is also a column at 390px and at 1280px, cover stays landscape at both", async ({
    page,
  }) => {
    // The "Shelf" grid is a DIFFERENT component from the `rows/` listing
    // above — a `:::grid 3` of lone `[[wikilinks]]`, not a folder listing —
    // but grid_cells.rs substitutes the same `<a class="moss-card">` markup
    // into `.moss-grid` that the listing gets into `.moss-cards`, so the
    // shared rule has to reach it through the same selector list, not a copy
    // of the rule scoped to `.moss-grid`. Unlike the listing above, its
    // cover ratio does not follow the narrow-container width (b47a55ed).
    await page.goto("/");
    await assertCardComposition(page, ".moss-grid .moss-card", false);
  });
});

// Optional scripts cannot own whether readers can see a scroll affordance.
test.describe("native scrollbar policy without scripts", () => {
  test.use({ javaScriptEnabled: false });
  for (const vertical of [false, true]) {
    test(`${vertical ? "vertical" : "horizontal"} pages and nested scrollers retain native bars`, async ({ page, browserName }) => {
      await page.goto(vertical ? "/vertical/" : "/");
      const metrics = await page.evaluate(() => {
        const nested = document.createElement("div");
        nested.style.cssText = "overflow:auto;width:128px;height:64px";
        nested.innerHTML = '<div style="width:256px;height:256px">Scrollable content</div>';
        document.body.append(nested);
        nested.scrollTop = 32;
        // Headless Firefox can suppress bars in its native stylesheet. Compare
        // with an unstyled document rather than overriding that browser choice.
        const reference = document.createElement("iframe");
        document.body.append(reference);
        const nativeWidth = reference.contentWindow!.getComputedStyle(reference.contentDocument!.body).scrollbarWidth;
        return {
          defaultWidth: nativeWidth,
          bodyWidth: getComputedStyle(document.body).scrollbarWidth,
          nestedWidth: getComputedStyle(nested).scrollbarWidth,
          nativeWidth: getComputedStyle(nested, "::-webkit-scrollbar").width,
          scrollTop: nested.scrollTop,
        };
      });
      expect(metrics.bodyWidth).toBe(metrics.defaultWidth);
      expect(metrics.nestedWidth).toBe(metrics.defaultWidth);
      if (browserName !== "firefox") expect(metrics.nativeWidth).toBe("auto");
      expect(metrics.scrollTop).toBe(32);
    });
  }
});
