/**
 * `:::grid {collage}`: a lead tile and smaller tiles, laid out from what they hold.
 *
 * The build computes each tile's spans; only a real engine answers whether the
 * tiles stay in the collage without overlapping, whether a row grows for a
 * quote and never for a picture, whether a label or a quote is ever cut off,
 * whether a focal point reaches the computed style, and whether a credit and a
 * linked picture's page are reachable by touch.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → COLLAGE_GATE (collages of 3 to 9
 * pictures on `/`, a mixed collage with a linked lead, a `contain` picture and
 * a short and a long quote on `/mixed/`, an upright lead on `/upright/`).
 */
import { test, expect, type Page } from "@playwright/test";
import { contrastRatio, parseColor, relativeLuminance } from "../../e2e/helpers/wcag-contrast";

const COUNTS = [3, 4, 5, 6, 7, 8, 9];

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

async function boxes(page: Page, selector: string) {
  return page.locator(selector).evaluate((grid) => {
    const rect = (el: Element): Box => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    };
    const tiles = [...grid.children];
    return {
      collage: rect(grid),
      tiles: tiles.map(rect),
      rows: getComputedStyle(grid).gridTemplateRows.split(" ").map(parseFloat),
    };
  });
}

const counted = (n: number) => `.moss-grid[data-collage][data-count="${n}"]`;
const MIXED = ".moss-grid[data-collage]";
const EPS = 0.75;

function expectPacked(collage: Box, tiles: Box[]) {
  tiles.forEach((t, i) => {
    expect(t.w, `tile ${i} has width`).toBeGreaterThan(20);
    expect(t.h, `tile ${i} has height`).toBeGreaterThan(20);
    expect(t.x, `tile ${i} left`).toBeGreaterThanOrEqual(collage.x - EPS);
    expect(t.y, `tile ${i} top`).toBeGreaterThanOrEqual(collage.y - EPS);
    expect(t.x + t.w, `tile ${i} right`).toBeLessThanOrEqual(collage.x + collage.w + EPS);
    expect(t.y + t.h, `tile ${i} bottom`).toBeLessThanOrEqual(collage.y + collage.h + EPS);
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
}

for (const [name, width, height] of [
  ["desktop", 1360, 900],
  ["phone", 390, 844],
] as const) {
  test.describe(`collage at ${name}`, () => {
    test.use({ viewport: { width, height } });

    for (const count of COUNTS) {
      test(`${count} tiles stay inside the collage, do not overlap, and the lead is the largest`, async ({ page }) => {
        await page.goto("/");
        const { collage, tiles } = await boxes(page, counted(count));
        expect(tiles).toHaveLength(count);
        expectPacked(collage, tiles);
        const area = tiles.map((t) => t.w * t.h);
        expect(area[0]).toBeGreaterThan(Math.max(...area.slice(1)));
      });
    }

    test("the mixed collage packs its quotes and pictures without overlap", async ({ page }) => {
      await page.goto("/mixed/");
      const { collage, tiles } = await boxes(page, MIXED);
      expect(tiles).toHaveLength(9);
      expectPacked(collage, tiles);
    });
  });
}

test.describe("collage geometry", () => {
  test("at 1360px the lead takes half the collage at full height, and every row fills the right half", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    for (const count of COUNTS) {
      const { collage, tiles } = await boxes(page, counted(count));
      expect(tiles[0].h, `${count}`).toBeCloseTo(collage.h, 0);
      expect(tiles[0].w / collage.w, `${count}`).toBeGreaterThan(0.45);
      expect(tiles[0].w / collage.w, `${count}`).toBeLessThan(0.51);
      const right = collage.x + collage.w;
      const rowEnds = new Map<number, number>();
      for (const t of tiles.slice(1)) rowEnds.set(Math.round(t.y), Math.max(rowEnds.get(Math.round(t.y)) ?? 0, t.x + t.w));
      for (const end of rowEnds.values()) expect(end, `${count}: a row ends at the collage's edge`).toBeCloseTo(right, 0);
    }
  });

  test("a one-row collage gets the taller row", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    expect((await boxes(page, counted(3))).rows).toEqual([300]);
    expect((await boxes(page, counted(5))).rows).toEqual([216, 216]);
    expect((await boxes(page, counted(9))).rows).toEqual([160, 160, 160]);
  });

  test("an upright lead takes a narrower column", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/upright/");
    await expect(page.locator(`${MIXED} > :first-child`)).toHaveAttribute("data-upright", "");
    const { collage, tiles } = await boxes(page, MIXED);
    expect(tiles[0].w / collage.w).toBeLessThan(0.42);
    expect(tiles[0].h).toBeCloseTo(collage.h, 0);
  });

  test("at 390px the lead is full width at 4:3 and pictures sit in rows of three squares", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    const { collage, tiles } = await boxes(page, counted(7));
    expect(tiles[0].w).toBeCloseTo(collage.w, 0);
    expect(tiles[0].w / tiles[0].h).toBeCloseTo(4 / 3, 1);
    expect(new Set(tiles.slice(1).map((t) => Math.round(t.y))).size).toBe(2);
    for (const t of tiles.slice(1)) expect(t.w).toBeCloseTo(t.h, 0);
  });

  test("at 390px a short last row is widened: two at 4:3, one at 3:2", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    const nine = await boxes(page, counted(9));
    const [a, b] = nine.tiles.slice(-2);
    expect(a.y).toBeCloseTo(b.y, 0);
    expect(a.w / a.h).toBeCloseTo(4 / 3, 1);
    expect(a.w + b.w).toBeGreaterThan(nine.collage.w - 12);
    const five = await boxes(page, counted(5));
    const last = five.tiles[4];
    expect(last.w).toBeCloseTo(five.collage.w, 0);
    expect(last.w / last.h).toBeCloseTo(3 / 2, 1);
  });

  test("at 390px a quote takes a full row at its own height", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/mixed/");
    const { collage, tiles } = await boxes(page, MIXED);
    for (const i of [2, 7]) {
      expect(tiles[i].w, `quote ${i}`).toBeCloseTo(collage.w, 0);
    }
  });
});

test.describe("collage rows grow only for words", () => {
  test("a row grows for a quote and not for a tall picture", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const { tiles, rows } = await boxes(page, MIXED);
    expect(rows).toHaveLength(3);
    // Row 2 holds two pictures taller than wide and no quote; row 3 holds the long quote.
    expect(rows[1]).toBe(160);
    expect(rows[2]).toBeGreaterThan(160);
    expect(tiles[7].h).toBeCloseTo(rows[2], 0);
  });
});

test.describe("nothing is cut off", () => {
  for (const width of [1360, 1100, 900, 768, 390]) {
    test(`at ${width}px no label or quote overflows its tile`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/mixed/");
      const report = await page.locator(MIXED).evaluate((grid) =>
        [...grid.children].map((tile, i) => {
          const label = tile.querySelector<HTMLElement>(".moss-image figcaption");
          const quote = tile.querySelector<HTMLElement>(".moss-quote");
          return {
            i,
            tileH: tile.clientHeight,
            tileW: tile.clientWidth,
            tileScrollH: tile.scrollHeight,
            labelH: label?.offsetHeight ?? 0,
            labelScrollW: label?.scrollWidth ?? 0,
            labelW: label?.clientWidth ?? 0,
            quote: !!quote,
          };
        }),
      );
      for (const r of report) {
        expect(r.labelH, `tile ${r.i}: label fits the tile's height`).toBeLessThanOrEqual(r.tileH + 1);
        expect(r.labelScrollW, `tile ${r.i}: label fits the tile's width`).toBeLessThanOrEqual(r.labelW + 1);
        if (r.quote) expect(r.tileScrollH, `tile ${r.i}: quote is not cut off`).toBeLessThanOrEqual(r.tileH + 1);
      }
    });
  }
});

test.describe("picture words reach the page", () => {
  test("a focal point and contain survive into the computed style", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const lead = page.locator(`${MIXED} > :nth-child(1) img`);
    await expect(lead).toHaveCSS("object-position", "20% 70%");
    await expect(page.locator(`${MIXED} > :nth-child(2) img`)).toHaveCSS("object-fit", "contain");
    // The lead links to its page, around the picture only.
    await expect(page.locator(`${MIXED} > :nth-child(1) figure.moss-image > a`)).toHaveAttribute("href", /\/upright\/$/);
  });

  test("a quote is a quote figure whose source alone links", async ({ page }) => {
    await page.goto("/mixed/");
    const short = page.locator(`${MIXED} > :nth-child(3) figure.moss-quote`);
    await expect(short).not.toHaveAttribute("data-length", /.*/);
    await expect(short.locator("figcaption a")).toHaveAttribute("href", "https://example.org/essays/");
    await expect(short.locator("blockquote a")).toHaveCount(0);
    await expect(page.locator(`${MIXED} > :nth-child(8) figure.moss-quote`)).toHaveAttribute("data-length", "long");
  });
});

test.describe("collage credits", () => {
  test("at 1360px a caption is hidden at rest and shown on hover, its bold run a title line", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const tile = page.locator(`${MIXED} > :nth-child(4)`);
    const caption = tile.locator("figcaption");
    await expect(caption).toHaveCSS("opacity", "0");
    await tile.scrollIntoViewIfNeeded();
    await tile.hover();
    await expect(caption).toHaveCSS("opacity", "1");
    await expect(caption.locator("strong")).toHaveCSS("display", "block");
  });

  test("a caption shows on keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const tile = page.locator(`${counted(4)} > :nth-child(3)`);
    await tile.locator("img").focus();
    await expect(tile.locator("figcaption")).toHaveCSS("opacity", "1");
  });

  test("a tile too small for both lines keeps the title and hides the facts to the eye only", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/mixed/");
    const caption = page.locator(`${MIXED} > :nth-child(4) figcaption`);
    await expect(caption).toHaveCSS("font-size", "0px");
    expect(parseFloat(await caption.locator("strong").evaluate((el) => getComputedStyle(el).fontSize))).toBeGreaterThan(10);
    expect(await caption.evaluate((el) => getComputedStyle(el).display)).not.toBe("none");
  });

  for (const scheme of ["light", "dark"] as const)
  test(`a three-line credit keeps 4.5:1 contrast on its band (${scheme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const tile = page.locator(`${counted(5)} > :nth-child(3)`);
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
    expect(bg.a).toBe(1);
    const ratio = contrastRatio(relativeLuminance(fg.r, fg.g, fg.b), relativeLuminance(bg.r, bg.g, bg.b));
    expect(ratio).toBeGreaterThanOrEqual(4.5);
  });

  test.describe("on a touch phone", () => {
    test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

    test("tapping a picture opens the viewer with its caption", async ({ page }) => {
      await page.goto("/");
      const tile = page.locator(`${counted(7)} > :nth-child(5)`);
      await tile.scrollIntoViewIfNeeded();
      await tile.locator("img").tap();
      const viewer = page.locator("#lightbox");
      await expect(viewer).toBeVisible();
      await expect(viewer.locator(".lightbox-caption")).toContainText("Credit for picture 5.");
      await expect(viewer.locator(".lightbox-image")).toHaveAttribute("src", /t5\.svg/);
    });

    test("tapping a linked picture opens the viewer with its credit and a link to its page", async ({ page }) => {
      await page.goto("/mixed/");
      expect(await page.evaluate(() => matchMedia("(hover: none)").matches)).toBe(true);
      await page.locator(`${MIXED} > :nth-child(1) img`).tap();
      const viewer = page.locator("#lightbox");
      await expect(viewer).toBeVisible();
      await expect(page).toHaveURL(/\/mixed\/$/);
      await expect(viewer.locator(".lightbox-caption")).toContainText("Harbour at dawn");
      const link = viewer.locator(".lightbox-article-link");
      await expect(link).toBeVisible();
      // The label is the build's, in the page's language.
      await expect(link).toHaveText(/^Open its page/);
      expect(await link.textContent()).toBe(await link.getAttribute("data-page-label"));
      await expect(link).toHaveAttribute("href", /\/upright\/$/);
    });
  });
});

test.describe("a picture shown whole", () => {
  test("is not zoomed on hover, after its placeholder is cleared, while a cropped one is", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const whole = page.locator(`${MIXED} > :nth-child(2)`);
    const img = whole.locator("img");
    await expect(img).toHaveAttribute("data-fit", "contain");
    await whole.scrollIntoViewIfNeeded();
    // The raster's blurred placeholder is removed once it loads, which rewrites its style attribute.
    await expect.poll(() => img.evaluate((el: HTMLImageElement) => el.complete && el.style.backgroundImage === "")).toBe(true);
    await whole.hover();
    await page.waitForTimeout(400);
    expect(await img.evaluate((el) => getComputedStyle(el).transform)).toBe("none");
    const cropped = page.locator(`${MIXED} > :nth-child(4)`);
    await cropped.hover();
    await expect.poll(() => cropped.locator("img").evaluate((el) => getComputedStyle(el).transform)).not.toBe("none");
  });
});

test.describe("keyboard focus", () => {
  test("a focused linked picture rings its tile", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const tile = page.locator(`${MIXED} > :nth-child(1)`);
    await expect(tile).toHaveCSS("outline-style", "none");
    await page.keyboard.press("Tab");
    await tile.locator("a").focus();
    await expect(tile).toHaveCSS("outline-style", "solid");
    await expect(tile).toHaveCSS("outline-width", "2px");
    // The link fills the tile, so its own box is the tile's.
    const [a, t] = await Promise.all([tile.locator("a").boundingBox(), tile.boundingBox()]);
    expect(a!.width).toBeCloseTo(t!.width, 0);
    expect(a!.height).toBeCloseTo(t!.height, 0);
  });
});

test.describe("collage text tiles", () => {
  test("a quote is set on the page's paper with no fade mask", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const tile = page.locator(`${MIXED} > :nth-child(8)`);
    const style = await tile.evaluate((el) => {
      const cs = getComputedStyle(el);
      return { background: cs.backgroundColor, page: getComputedStyle(document.body).backgroundColor, mask: cs.maskImage || cs.webkitMaskImage };
    });
    expect(style.background).toBe(style.page);
    expect(style.mask === "none" || style.mask === "" || style.mask === undefined).toBe(true);
  });
});

test.describe("collage on a vertical page", () => {
  test("keeps horizontal tracks and gives a text tile the page's vertical writing mode", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/vertical/");
    const grid = page.locator(".moss-grid[data-collage]");
    await expect(grid).toHaveCSS("writing-mode", "horizontal-tb");
    await expect(grid.locator(":scope > :nth-child(3)")).toHaveCSS("writing-mode", "vertical-rl");
    const { collage, tiles } = await boxes(page, counted(5));
    for (const t of tiles) {
      expect(t.x).toBeGreaterThanOrEqual(collage.x - EPS);
      expect(t.x + t.w).toBeLessThanOrEqual(collage.x + collage.w + EPS);
    }
  });
});
