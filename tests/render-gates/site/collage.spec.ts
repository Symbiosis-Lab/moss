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

  // 48rem is half-open: a page exactly 768px wide keeps the lead column, 767px stacks.
  for (const [width, wide] of [[767, false], [768, true]] as const) {
    test(`at ${width}px the collage ${wide ? "keeps its lead column" : "stacks"}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      const { collage, tiles } = await boxes(page, counted(7));
      if (wide) {
        expect(tiles[0].h).toBeCloseTo(collage.h, 0);
        expect(tiles[0].w / collage.w).toBeLessThan(0.51);
      } else {
        expect(tiles[0].w).toBeCloseTo(collage.w, 0);
      }
    });
  }

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
  for (const path of ["/mixed/", "/full/"])
  for (const width of [1360, 1100, 900, 768, 390]) {
    test(`at ${width}px on ${path} no label or quote overflows its tile`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(path);
      const report = await page.locator(MIXED).evaluate((grid) =>
        [...grid.children].map((tile, i) => {
          const label = tile.querySelector<HTMLElement>(".moss-image figcaption");
          const quote = tile.querySelector<HTMLElement>(".moss-quote");
          const words = quote?.querySelector("blockquote");
          // The widest word, measured as laid out: one range per word.
          let wordRight = 0;
          if (words) {
            const walker = document.createTreeWalker(words, NodeFilter.SHOW_TEXT);
            for (let n = walker.nextNode(); n; n = walker.nextNode()) {
              for (const m of n.textContent!.matchAll(/\S+/g)) {
                const range = document.createRange();
                range.setStart(n, m.index!);
                range.setEnd(n, m.index! + m[0].length);
                const rects = [...range.getClientRects()].filter((r) => r.width > 0);
                for (const r of rects) wordRight = Math.max(wordRight, r.right);
              }
            }
          }
          const box = tile.getBoundingClientRect();
          const contentRight = box.right - parseFloat(getComputedStyle(tile).paddingRight) - parseFloat(getComputedStyle(tile).borderRightWidth);
          return {
            i,
            tileH: tile.clientHeight,
            tileW: tile.clientWidth,
            tileScrollH: tile.scrollHeight,
            labelH: label?.offsetHeight ?? 0,
            labelScrollH: label?.scrollHeight ?? 0,
            labelLines: label
              ? (label.clientHeight - parseFloat(getComputedStyle(label).paddingTop) - parseFloat(getComputedStyle(label).paddingBottom)) /
                parseFloat(getComputedStyle(label.querySelector("strong") ?? label).lineHeight)
              : 0,
            ...(() => {
              const title = label?.querySelector<HTMLElement>(":scope > strong:first-child");
              let titleWordRoom = 0;
              if (title) {
                const walker = document.createTreeWalker(title, NodeFilter.SHOW_TEXT);
                for (let n = walker.nextNode(); n; n = walker.nextNode()) {
                  for (const m of n.textContent!.matchAll(/\S+/g)) {
                    const probe = document.createElement("span");
                    probe.style.cssText = "position:absolute;visibility:hidden;white-space:nowrap";
                    probe.textContent = m[0];
                    title.append(probe);
                    titleWordRoom = Math.max(titleWordRoom, probe.getBoundingClientRect().width - (title.clientWidth - 0));
                    probe.remove();
                  }
                }
              }
              const lb = label?.getBoundingClientRect();
              const tb = title?.getBoundingClientRect();
              const padBottom = label ? parseFloat(getComputedStyle(label).paddingBottom) : 0;
              return {
                titleH: title?.clientHeight ?? 0,
                titleScrollH: title?.scrollHeight ?? 0,
                titleWordRoom,
                // How far the title's last line runs below the label's content box: 0 when whole.
                titleCut: lb && tb ? Math.max(0, tb.bottom - (lb.bottom - padBottom)) : 0,
                factsShown: label ? parseFloat(getComputedStyle(label).fontSize) > 0 : false,
              };
            })(),
            hasLabel: !!label,
            labelDrawn: label ? Math.min(label.getBoundingClientRect().width, label.getBoundingClientRect().height) : 0,
            labelText: label?.textContent?.trim() ?? "",
            labelScrollW: label?.scrollWidth ?? 0,
            labelW: label?.clientWidth ?? 0,
            quote: !!quote,
            wordOver: words ? wordRight - contentRight : 0,
          };
        }),
      );
      for (const r of report) {
        if (width >= 768) {
          expect(r.labelH, `tile ${r.i}: label fits the tile's height`).toBeLessThanOrEqual(r.tileH + 1);
          expect(r.labelScrollW, `tile ${r.i}: label fits the tile's width`).toBeLessThanOrEqual(r.labelW + 1);
          // The cap and the whole-line stop apply only while the facts line is shown.
          if (r.factsShown) {
            expect(r.labelH, `tile ${r.i}: label takes at most 55% of the tile`).toBeLessThanOrEqual(r.tileH * 0.55 + 1);
            expect(Math.abs(r.labelLines - Math.round(r.labelLines)), `tile ${r.i}: the label ends on a whole line`).toBeLessThan(0.05);
          }
          if (width < 900) expect(r.factsShown, `tile ${r.i}: the facts are hidden under 56.25rem`).toBe(false);
          expect(r.titleCut, `tile ${r.i}: no line of the title is cut`).toBeLessThanOrEqual(1);
          // The title is whole: no line of it is cut, and no word is broken to fit.
          expect(r.titleScrollH, `tile ${r.i}: the title is not cut`).toBeLessThanOrEqual(r.titleH + 1);
          expect(r.titleWordRoom, `tile ${r.i}: the title's widest word fits a line whole`).toBeLessThanOrEqual(0.5);
        } else if (r.hasLabel) {
          // Below 48rem no credit is drawn on a picture; it is still in the page.
          expect(r.labelDrawn, `tile ${r.i}: the credit is not drawn`).toBeLessThanOrEqual(2);
          expect(r.labelText, `tile ${r.i}: the credit stays in the markup`).not.toBe("");
        }
        if (r.quote) expect(r.tileScrollH, `tile ${r.i}: quote is not cut off`).toBeLessThanOrEqual(r.tileH + 1);
        if (r.quote) expect(r.wordOver, `tile ${r.i}: no quote word runs into the padding`).toBeLessThanOrEqual(0.5);
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
  test("a credit and a quote's source are roman and start-aligned, with no theme rule", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    await expect(page.locator(".moss-grid:not([data-collage]) .moss-quote")).toHaveCount(1);
    for (const sel of [
      `${MIXED} > :nth-child(4) figcaption`,
      `${MIXED} > :nth-child(3) .moss-quote figcaption`,
      ".moss-grid:not([data-collage]) .moss-quote figcaption",
    ]) {
      await expect(page.locator(sel)).toHaveCSS("font-style", "normal");
      await expect(page.locator(sel)).toHaveCSS("text-align", "start");
    }
  });

  test("the label's title is 500 weight and at most 0.78rem, its facts 0.66rem, and a linked tile's title ends in an arrow", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    // The lead is large enough to show both lines.
    const caption = page.locator(`${MIXED} > :nth-child(1) figcaption`);
    await expect(caption).toHaveCSS("font-size", "10.56px");
    await expect(caption.locator("strong")).toHaveCSS("font-size", "12.48px");
    await expect(caption.locator("strong")).toHaveCSS("font-weight", "500");
    const after = (sel: string) => page.locator(sel).evaluate((el) => getComputedStyle(el, "::after").content);
    // The arrow ends the title's last line, so a longer title never leaves it behind.
    expect(await after(`${MIXED} > :nth-child(1) figcaption > strong`)).toContain("\u2197");
    expect(await after(`${MIXED} > :nth-child(1) figcaption`)).toBe("none");
    expect(await after(`${MIXED} > :nth-child(4) figcaption > strong`)).toBe("none");
    // The arrow is drawn on the title, not written into it.
    await expect(page.locator(`${MIXED} > :nth-child(1) figcaption > strong`)).toHaveText("Harbour at dawn");
  });

  for (const width of [1360, 900, 768])
  test(`at ${width}px every title is shown whole on hover, and a long word is not broken`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/full/");
    const tiles = page.locator(`${MIXED} > [data-tile="picture"]`);
    const n = await tiles.count();
    for (let i = 0; i < n; i++) {
      const tile = tiles.nth(i);
      await tile.scrollIntoViewIfNeeded();
      await tile.hover();
      const title = tile.locator("figcaption > strong:first-child");
      await expect(title).toBeVisible();
      const m = await title.evaluate((el) => {
        const probe = document.createElement("span");
        probe.style.cssText = "position:absolute;visibility:hidden;white-space:nowrap";
        let widest = 0;
        for (const w of el.textContent!.split(/\s+/)) {
          probe.textContent = w;
          el.append(probe);
          widest = Math.max(widest, probe.getBoundingClientRect().width);
        }
        probe.remove();
        const lh = parseFloat(getComputedStyle(el).lineHeight);
        return { clipped: el.scrollHeight - el.clientHeight, widest, width: el.clientWidth, lines: el.getBoundingClientRect().height / lh, lh };
      });
      expect(m.clipped, `tile ${i}: no line of the title is cut`).toBeLessThanOrEqual(1);
      expect(m.widest, `tile ${i}: the widest word fits a line`).toBeLessThanOrEqual(m.width + 0.5);
      // The label is the tile's, so its title is never reduced to an ellipsis.
      expect(await title.evaluate((el) => getComputedStyle(el).textOverflow), `tile ${i}`).not.toBe("ellipsis");
    }
  });

  test("below 48rem a picture's credit is not drawn but stays in the page and the tile still links", async ({ page }) => {
    await page.setViewportSize({ width: 767, height: 900 });
    await page.goto("/mixed/");
    const tile = page.locator(`${MIXED} > :nth-child(1)`);
    const caption = tile.locator("figcaption");
    const box = await caption.boundingBox();
    expect(box!.width).toBeLessThanOrEqual(2);
    expect(box!.height).toBeLessThanOrEqual(2);
    await expect(caption).toContainText("Harbour at dawn");
    expect(await caption.evaluate((el) => getComputedStyle(el).display)).not.toBe("none");
    expect(await caption.evaluate((el) => getComputedStyle(el).visibility)).toBe("visible");
    await expect(tile.locator("figure > a")).toHaveAttribute("href", /\/upright\/$/);
    // A link in the hidden credit stays in the tab order, and focusing it shows the credit.
    const link = page.locator(`${MIXED} > :nth-child(4) figcaption a`);
    await expect(link).not.toHaveAttribute("tabindex", /.*/);
    const credit = page.locator(`${MIXED} > :nth-child(4) figcaption`);
    await expect(credit).toHaveCSS("opacity", "1");
    expect((await credit.boundingBox())!.width).toBeLessThanOrEqual(2);
    await link.focus();
    await expect.poll(async () => (await credit.boundingBox())!.width).toBeGreaterThan(40);
    await expect(credit).toHaveCSS("opacity", "1");
    // Read aloud: the tile link's accessible name includes the credit.
    expect(await tile.locator("figure").evaluate((el) => el.textContent)).toContain("City Museum");
  });

  test("at 1360px a caption is hidden at rest and shown on hover, its bold run a title line", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const tile = page.locator(`${MIXED} > :nth-child(4)`);
    const caption = tile.locator("figcaption");
    await expect(caption).toHaveCSS("opacity", "0");
    await tile.scrollIntoViewIfNeeded();
    await tile.hover();
    await expect(caption).toHaveCSS("opacity", "1");
    const display = await caption.locator("strong").evaluate((el) => getComputedStyle(el).display);
    expect(display, "the title is a line of its own").not.toMatch(/inline/);
  });

  test("a caption shows on keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/");
    const tile = page.locator(`${counted(4)} > :nth-child(3)`);
    await tile.locator("img").focus();
    await expect(tile.locator("figcaption")).toHaveCSS("opacity", "1");
  });

  test("a tile too small for both lines keeps the title and hides the facts to the eye only", async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto("/mixed/");
    const caption = page.locator(`${MIXED} > :nth-child(4) figcaption`);
    await expect(caption).toHaveCSS("font-size", "0px");
    expect(parseFloat(await caption.locator("strong").evaluate((el) => getComputedStyle(el).fontSize))).toBeGreaterThan(8);
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
      return {
        color: cs.color,
        bg: cs.backgroundColor,
        lines: el.getBoundingClientRect().height / parseFloat(cs.lineHeight),
        content: (el.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom)) / parseFloat(cs.lineHeight),
        share: el.getBoundingClientRect().height / el.closest(".moss-grid-card")!.getBoundingClientRect().height,
      };
    });
    expect(m.lines).toBeGreaterThan(2.5);
    // A credit longer than its tile allows stops at 55%; the viewer has the rest.
    expect(m.share).toBeLessThanOrEqual(0.555);
    // …at a whole line, never through one.
    expect(Math.abs(m.content - Math.round(m.content))).toBeLessThan(0.05);
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
    const ring = () => tile.evaluate((t) => { const c = getComputedStyle(t, "::after"); return [c.content, c.outlineStyle, c.outlineWidth]; });
    expect((await ring())[0]).toBe("none");
    await page.keyboard.press("Tab");
    await tile.locator("a").focus();
    expect((await ring()).slice(1)).toEqual(["solid", "2px"]);
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
    // A quote tile reads down the page too, and its row still grows to fit it.
    const quote = grid.locator(":scope > :nth-child(4)");
    await expect(quote).toHaveAttribute("data-tile", "quote");
    const fit = await quote.evaluate((el) => ({ h: [el.scrollHeight, el.clientHeight], w: [el.scrollWidth, el.clientWidth] }));
    expect(fit.h[0], "the quote is not cut off at its foot").toBeLessThanOrEqual(fit.h[1] + 1);
    expect(fit.w[0], "the quote is not cut off at its side").toBeLessThanOrEqual(fit.w[1] + 1);
    const { collage, tiles } = await boxes(page, counted(5));
    for (const t of tiles) {
      expect(t.x).toBeGreaterThanOrEqual(collage.x - EPS);
      expect(t.x + t.w).toBeLessThanOrEqual(collage.x + collage.w + EPS);
    }
  });
});

test.describe("a collage quote reads as type on paper", () => {
  const quote = (n: number) => `${MIXED} > :nth-child(${n})`;

  for (const [width, short, long] of [
    [1360, [15, 17.5], [13.5, 15]],
    [390, [15, 16], [13, 15]],
  ] as const)
  test(`at ${width}px it sits at the tile's top, small, hyphenated, its source pinned to the foot`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/full/");
    for (const [n, [lo, hi]] of [[3, short], [8, long]] as const) {
      const tile = page.locator(quote(n));
      const m = await tile.evaluate((el) => {
        const bq = el.querySelector("blockquote")!;
        const cap = el.querySelector("figcaption")!;
        const link = cap.querySelector("a") ?? cap;
        const t = el.getBoundingClientRect();
        const b = bq.getBoundingClientRect();
        const c = cap.getBoundingClientRect();
        const bs = getComputedStyle(bq);
        const cs = getComputedStyle(cap);
        const mid = document.elementFromPoint(t.left + t.width / 2, t.top + t.height / 2);
        return {
          size: parseFloat(bs.fontSize),
          hyphens: bs.hyphens,
          wrap: bs.textWrapStyle,
          gapTop: b.top - t.top,
          gapFoot: t.bottom - c.bottom,
          capSize: cs.fontSize,
          capFamily: cs.fontFamily,
          capColor: cs.color,
          secondary: getComputedStyle(document.body).getPropertyValue("--moss-color-text-secondary"),
          decoration: getComputedStyle(link).textDecorationLine,
          linked: !!cap.querySelector("a"),
          rule: cs.borderTopWidth,
          capW: c.width,
          linkW: link.getBoundingClientRect().width,
          wholeTileLinks: mid?.closest("a") === link,
        };
      });
      expect(m.size, `quote ${n}`).toBeGreaterThanOrEqual(lo - 0.01);
      expect(m.size, `quote ${n}`).toBeLessThanOrEqual(hi + 0.01);
      expect(m.hyphens, `quote ${n}: hyphenated by the page's language`).toBe("auto");
      expect(m.gapTop, `quote ${n}: set at the top`).toBeLessThanOrEqual(24);
      expect(m.gapFoot, `quote ${n}: source at the foot`).toBeLessThanOrEqual(20);
      expect(m.capSize).toBe("11px");
      expect(m.capFamily).toMatch(/mono/i);
      expect(m.decoration).toBe("none");
      expect(m.rule).toBe("1px");
      expect(m.capW, `quote ${n}: the hairline is as wide as the source`).toBeCloseTo(m.linkW, -1);
      if (m.linked) expect(m.wholeTileLinks, `quote ${n}: the whole tile follows the source's link`).toBe(true);
    }
  });

  test("a long quote in the narrowest tile steps down, breaks no word and is not cut off", async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 900 });
    await page.goto("/full/");
    const tile = page.locator(quote(8));
    const m = await tile.evaluate((el) => ({
      size: parseFloat(getComputedStyle(el.querySelector("blockquote")!).fontSize),
      cut: el.scrollHeight - el.clientHeight,
      w: el.getBoundingClientRect().width,
    }));
    expect(m.w, "this is a small tile").toBeLessThan(180);
    expect(m.size).toBeLessThan(15);
    expect(m.cut).toBeLessThanOrEqual(1);
  });
});

test.describe("a picture shown whole", () => {
  test("keeps paper around it instead of filling the tile edge to edge", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const img = page.locator(`${MIXED} > :nth-child(2) img`);
    await expect(img).toHaveCSS("padding-top", "12px");
    await expect(img).toHaveCSS("padding-left", "12px");
    const [i, t] = await Promise.all([img.boundingBox(), page.locator(`${MIXED} > :nth-child(2)`).boundingBox()]);
    expect(i!.height).toBeCloseTo(t!.height, 0);
    await expect(page.locator(`${MIXED} > :nth-child(4) img`)).toHaveCSS("padding-top", "0px");
  });
});

test.describe("nothing widens the page", () => {
  for (const width of [390, 360]) {
    test(`at ${width}px a long address in a grid card and a linked picture in a list stay inside the page`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto("/cards/");
      const m = await page.evaluate(() => ({ scroll: document.documentElement.scrollWidth, client: document.documentElement.clientWidth }));
      expect(m.scroll).toBeLessThanOrEqual(m.client);
    });
  }
});

test.describe("the phone gutter and the section face", () => {
  test("--moss-container-padding-narrow sets the gutter below 48rem and leaves 48rem and up alone", async ({ page }) => {
    const gutter = () => page.locator("article").evaluate((el) => getComputedStyle(el).paddingLeft);
    await page.setViewportSize({ width: 390, height: 800 });
    await page.goto("/narrow-note/");
    expect(await gutter()).toBe("16px");
    await page.goto("/mixed/");
    expect(await gutter(), "an untouched page keeps 1.5rem").toBe("24px");
    await page.setViewportSize({ width: 1360, height: 800 });
    await page.goto("/narrow-note/");
    expect(await gutter(), "the desktop gutter is not the narrow one").toBe("32px");
  });

  test("--moss-font-section sets section headings and the page title keeps the heading face", async ({ page }) => {
    await page.goto("/narrow-note/");
    const face = (sel: string) => page.locator(sel).evaluate((el) => getComputedStyle(el).fontFamily);
    expect(await face("article h2")).toMatch(/monospace/);
    // The page title keeps the heading face: the property is set on this very page.
    const title = await face(".moss-article-title");
    expect(title).not.toMatch(/monospace/);
    expect(title).toMatch(/serif/i);
  });
});
test.describe("a quote inside a tile does not read the page's column against the tile", () => {
  test("a full-width page with a contents lane sets its quotes as wide as their tiles at 1360px", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/contents-full/");
    const r = await page.locator(MIXED).evaluate((grid) => ({
      height: grid.getBoundingClientRect().height,
      quotes: [...grid.querySelectorAll<HTMLElement>(".moss-quote")].map((q) => {
        const p = q.querySelector("p")!;
        const tile = q.closest<HTMLElement>("[data-tile]")!;
        return { cap: getComputedStyle(p).maxInlineSize, tile: tile.getBoundingClientRect().width, p: p.getBoundingClientRect().width, lines: p.getClientRects().length };
      }),
    }));
    expect(r.quotes.length).toBe(2);
    for (const q of r.quotes) {
      expect(q.p, `paragraph ${q.p}px in a ${q.tile}px tile (cap ${q.cap})`).toBeGreaterThan(q.tile / 2);
    }
    expect(r.height, "the collage keeps its height").toBeLessThan(1200);
  });
});

test.describe("a card listing keeps the page column as its reading cap", () => {
  for (const [path, width] of [["/mixed/", 1360], ["/contents-full/", 1360]] as const) {
    test(`at ${width}px on ${path} a list layout inside a wider container is capped at the column`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(path);
      const r = await page.evaluate(() => {
        const article = document.querySelector("article")!;
        const box = document.createElement("div");
        box.className = "moss-cards-container";
        box.style.inlineSize = "1500px";
        box.innerHTML = '<div class="moss-cards" data-layout="list"></div>';
        article.append(box);
        const list = box.firstElementChild as HTMLElement;
        return { cap: getComputedStyle(list).maxInlineSize, column: getComputedStyle(article).maxInlineSize };
      });
      expect(r.column).toMatch(/px$/);
      expect(r.cap).toBe(r.column);
    });
  }
});

test.describe("the contents lane's column is measured against main, not the window", () => {
  test("a paragraph's cap follows main's width when the body gives some of it up", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/contents-full/");
    // Room taken from the right of `main` (a sidenote reserve or a scrollbar does the same):
    // 100vw still says 1360px, `main`'s content box says less.
    await page.evaluate(() => { document.body.style.paddingRight = "80px"; });
    const r = await page.evaluate(() => {
      const main = document.querySelector("main")!;
      const p = main.querySelector("p")!;
      return { main: main.getBoundingClientRect().width, cap: parseFloat(getComputedStyle(p).maxInlineSize), column: parseFloat(getComputedStyle(main.querySelector("article")!).maxInlineSize) };
    });
    expect(r.main, "main gave up the padding").toBeLessThan(1300);
    expect(r.column, "the column is main's width less two lanes (92px each), not the window's").toBeCloseTo(Math.min(1200, r.main - 184), 0);
  });
});

test.describe("a collage quote keeps its reading size at tablet widths", () => {
  for (const width of [768, 1024, 1100])
    test(`at ${width}px a quote is not set smaller than its floor`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/pair/");
      const sizes = await page.locator(`${MIXED} blockquote`).evaluateAll((all) =>
        all.map((b) => parseFloat(getComputedStyle(b).fontSize)));
      expect(sizes[0], "a short quote").toBeGreaterThanOrEqual(15 - 0.01);
      expect(sizes[1], "a long quote").toBeGreaterThanOrEqual(13.5 - 0.01);
    });
});

test.describe("a quote's long word is hyphenated, not cut", () => {
  test("a word that wraps inside a narrow tile breaks at a syllable, as the page's language says", async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto("/full/");
    const widths = (hyphens: string) =>
      page.locator(`${MIXED} [data-tile="quote"] blockquote`).first().evaluate((bq, mode) => {
        bq.style.hyphens = mode;
        const out: Record<string, number> = {};
        const walker = document.createTreeWalker(bq, NodeFilter.SHOW_TEXT);
        for (let n = walker.nextNode(); n; n = walker.nextNode()) {
          for (const m of n.textContent!.matchAll(/\S+/g)) {
            const range = document.createRange();
            range.setStart(n, m.index!);
            range.setEnd(n, m.index! + m[0].length);
            const rects = range.getClientRects();
            if (rects.length > 1) out[m[0]] = Math.round(rects[0].width);
          }
        }
        return out;
      }, hyphens);
    const auto = await widths("auto");
    const raw = await widths("manual");
    const words = Object.keys(raw);
    expect(words.length, "the tile is narrow enough to wrap a word").toBeGreaterThan(0);
    // A capitalised word may be cut unhyphenated by some engines (the accepted last resort); a lowercase one must break at a syllable.
    const lower = words.filter((w) => /^[a-z]/.test(w));
    expect(lower.length, "a lowercase word wraps too").toBeGreaterThan(0);
    for (const w of lower) expect(auto[w], `${w} breaks at a syllable`).not.toBe(raw[w]);
  });
});

test.describe("a collage tile's keyboard focus ring", () => {
  test("shows over the picture, and the picture is untouched when focus leaves", async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 900 });
    await page.goto("/mixed/");
    const tile = page.locator(`${MIXED} > :nth-child(1)`);
    await tile.scrollIntoViewIfNeeded();
    const box = (await tile.boundingBox())!;
    // The left edge, clear of the caption at the foot.
    const strip = { x: box.x, y: box.y + box.height * 0.2, width: 6, height: box.height * 0.4 };
    const before = await page.screenshot({ clip: strip });
    await page.keyboard.press("Tab");
    await tile.locator("a").focus();
    expect(await tile.evaluate((t) => t.matches(":has(:focus-visible)")), "Tab reached the tile").toBe(true);
    const ring = await page.screenshot({ clip: strip });
    expect(ring.equals(before), "the ring is painted on the picture's edge").toBe(false);
    await page.evaluate(() => (document.activeElement as HTMLElement).blur());
    await page.mouse.move(0, 0);
    expect((await page.screenshot({ clip: strip })).equals(before), "nothing stays over the picture").toBe(true);
  });
});
