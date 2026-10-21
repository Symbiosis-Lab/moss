/**
 * `:::grid {collage}`: a lead tile and smaller tiles packed into one box.
 *
 * The regression this guards is a hand-placed seven-tile grid whose lead
 * overran its row on a phone. Only a real engine answers whether tiles stay in
 * the collage, whether any two overlap, and whether a credit is reachable by
 * touch.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → COLLAGE_GATE (collages of 4, 7 with
 * one passage tile, and 9 tiles).
 */
import { test, expect, type Page } from "@playwright/test";
import { contrastRatio, parseColor, relativeLuminance } from "../../e2e/helpers/wcag-contrast";

const COUNTS = [4, 7, 9];

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

async function boxes(page: Page, count: number) {
  return page.locator(`.moss-grid[data-collage][data-count="${count}"]`).evaluate((grid) => {
    const rect = (el: Element): Box => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    };
    const tiles = [...grid.children];
    return {
      collage: rect(grid),
      tiles: tiles.map(rect),
      // The deepest box any tile's picture reaches, to assert nothing spills out of its cell.
      inner: tiles.map((t) => [...t.querySelectorAll("figure, img")].map(rect)),
    };
  });
}

const EPS = 0.75;

for (const [name, width, height] of [
  ["desktop", 1360, 900],
  ["phone", 390, 844],
] as const) {
  test.describe(`collage at ${name}`, () => {
    test.use({ viewport: { width, height } });

    for (const count of COUNTS) {
      test(`${count} tiles stay inside the collage, do not overlap, and the lead is the largest`, async ({ page }) => {
        await page.goto("/");
        await page.waitForLoadState("load");
        const { collage, tiles, inner } = await boxes(page, count);
        expect(tiles).toHaveLength(count);

        tiles.forEach((t, i) => {
          expect(t.w, `tile ${i} has width`).toBeGreaterThan(20);
          expect(t.h, `tile ${i} has height`).toBeGreaterThan(20);
          expect(t.x, `tile ${i} left`).toBeGreaterThanOrEqual(collage.x - EPS);
          expect(t.y, `tile ${i} top`).toBeGreaterThanOrEqual(collage.y - EPS);
          expect(t.x + t.w, `tile ${i} right`).toBeLessThanOrEqual(collage.x + collage.w + EPS);
          expect(t.y + t.h, `tile ${i} bottom`).toBeLessThanOrEqual(collage.y + collage.h + EPS);
          for (const b of inner[i]) {
            expect(b.x).toBeGreaterThanOrEqual(t.x - EPS);
            expect(b.y).toBeGreaterThanOrEqual(t.y - EPS);
            expect(b.x + b.w).toBeLessThanOrEqual(t.x + t.w + EPS);
            expect(b.y + b.h).toBeLessThanOrEqual(t.y + t.h + EPS);
          }
        });

        for (let i = 0; i < tiles.length; i++) {
          for (let j = i + 1; j < tiles.length; j++) {
            const a = tiles[i];
            const b = tiles[j];
            const overlapW = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
            const overlapH = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
            expect(overlapW > EPS && overlapH > EPS, `tiles ${i} and ${j} overlap`).toBe(false);
          }
        }

        const area = tiles.map((t) => t.w * t.h);
        expect(area[0]).toBe(Math.max(...area));
        expect(area[0]).toBeGreaterThan(Math.max(...area.slice(1)));
      });
    }

    test("the passage tile fills with the page background and clips its text", async ({ page }) => {
      await page.goto("/");
      const tile = page.locator('.moss-grid[data-collage][data-count="7"] > :nth-child(4)');
      const style = await tile.evaluate((el) => {
        const cs = getComputedStyle(el);
        return {
          overflow: cs.overflow,
          background: cs.backgroundColor,
          page: getComputedStyle(document.body).backgroundColor,
          mask: cs.maskImage || cs.webkitMaskImage,
        };
      });
      expect(style.overflow).toBe("hidden");
      expect(style.background).toBe(style.page);
      expect(style.mask).toContain("linear-gradient");
    });
  });
}

test.describe("collage geometry", () => {
  test("at 1360px the lead takes the left half at full height", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const { collage, tiles } = await boxes(page, 7);
    expect(tiles[0].h).toBeCloseTo(collage.h, 0);
    expect(tiles[0].w).toBeGreaterThan(collage.w * 0.45);
    expect(tiles[0].w).toBeLessThan(collage.w * 0.5 + 1);
    expect(collage.w / collage.h).toBeCloseTo(9 / 4, 1);
  });

  test("at 390px the lead is full width and the 7-tile collage stays compact", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    const { collage, tiles } = await boxes(page, 7);
    expect(tiles[0].w).toBeCloseTo(collage.w, 0);
    expect(tiles[0].w / tiles[0].h).toBeCloseTo(1.5, 1);
    expect(collage.h).toBeLessThan(collage.w * 1.4);
    // Rows of three: tiles 2 to 7 share two rows.
    expect(new Set(tiles.slice(1).map((t) => Math.round(t.y))).size).toBe(2);
    for (const t of tiles.slice(1)) expect(t.w).toBeCloseTo(t.h, 0);
  });

  test("a last row of fewer tiles shares the width", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    const { collage, tiles } = await boxes(page, 9);
    const last = tiles.slice(-2);
    expect(last[0].y).toBeCloseTo(last[1].y, 0);
    expect(last[0].w).toBeCloseTo(last[1].w, 0);
    expect(last[0].w + last[1].w).toBeGreaterThan(collage.w - 12);
  });
});

test.describe("collage credits", () => {
  test("at 1360px a caption is hidden at rest and shown on hover", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const tile = page.locator('.moss-grid[data-collage][data-count="4"] > :nth-child(2)');
    const caption = tile.locator("figcaption");
    await expect(caption).toHaveText("Credit for picture 2.");
    await expect(caption).toHaveCSS("opacity", "0");
    await tile.scrollIntoViewIfNeeded();
    await tile.hover();
    await expect(caption).toHaveCSS("opacity", "1");
  });

  test("a caption shows on keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const tile = page.locator('.moss-grid[data-collage][data-count="4"] > :nth-child(3)');
    await tile.locator("img").focus();
    await expect(tile.locator("figcaption")).toHaveCSS("opacity", "1");
  });

  test.describe("on a touch phone", () => {
    test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

    test("tapping a picture opens the viewer with its caption", async ({ page }) => {
      await page.goto("/");
      const tile = page.locator('.moss-grid[data-collage][data-count="7"] > :nth-child(5)');
      await tile.scrollIntoViewIfNeeded();
      await tile.locator("img").tap();
      const viewer = page.locator("#lightbox");
      await expect(viewer).toBeVisible();
      await expect(viewer.locator(".lightbox-caption")).toContainText("Credit for picture 5.");
      await expect(viewer.locator(".lightbox-image")).toHaveAttribute("src", /t5\.svg/);
    });
  });
});

test.describe("collage credit legibility", () => {
  test("a three-line credit over a white picture keeps 4.5:1 contrast", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const tile = page.locator('.moss-grid[data-collage][data-count="5"] > :nth-child(3)');
    await tile.scrollIntoViewIfNeeded();
    await tile.hover();
    const caption = tile.locator("figcaption");
    await expect(caption).toHaveCSS("opacity", "1");
    const m = await caption.evaluate((el) => {
      const cs = getComputedStyle(el);
      return { color: cs.color, bg: cs.backgroundColor, lines: el.getBoundingClientRect().height / parseFloat(cs.lineHeight) };
    });
    expect(m.lines).toBeGreaterThan(2.5);
    const fg = parseColor(m.color);
    const bg = parseColor(m.bg);
    // The picture under the scrim is white.
    const base = {
      r: bg.a * bg.r + (1 - bg.a) * 255,
      g: bg.a * bg.g + (1 - bg.a) * 255,
      b: bg.a * bg.b + (1 - bg.a) * 255,
    };
    const ratio = contrastRatio(
      relativeLuminance(fg.r, fg.g, fg.b),
      relativeLuminance(base.r, base.g, base.b),
    );
    expect(ratio).toBeGreaterThanOrEqual(4.5);
  });
});

test.describe("collage on a vertical page", () => {
  test("keeps horizontal tracks and gives a text tile the page's vertical writing mode", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/vertical/");
    const grid = page.locator(".moss-grid[data-collage]");
    await expect(grid).toHaveCSS("writing-mode", "horizontal-tb");
    await expect(grid.locator(":scope > :nth-child(3)")).toHaveCSS("writing-mode", "vertical-rl");
    const { collage, tiles } = await boxes(page, 5);
    for (const t of tiles) {
      expect(t.x).toBeGreaterThanOrEqual(collage.x - EPS);
      expect(t.x + t.w).toBeLessThanOrEqual(collage.x + collage.w + EPS);
    }
  });
});
