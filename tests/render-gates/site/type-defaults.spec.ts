/**
 * Six defaults a hand-built design pinned down, each measured in a real engine
 * on a site the CLI built. Fixture: tests/e2e/helpers/type-defaults-site.ts.
 * Run via: bash scripts/render-gates.sh type-defaults
 */
import { test, expect, type Page } from '@playwright/test';

const CARD_FIGURE = ".moss-grid-card figure.moss-image";

async function open(page: Page, url: string, width = 1280, height = 900): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(url);
  await page.waitForSelector(".moss-nav-island", { state: "attached" });
  await page.evaluate(() => document.fonts.ready);
}

test.describe("a captioned picture in a grid card", () => {
  test("has no block margin, in a plain card and in a link card", async ({ page }) => {
    await open(page, "/cards/");
    const margins = await page.$$eval(CARD_FIGURE, (figs) =>
      figs.map((f) => {
        const cs = getComputedStyle(f);
        return [cs.marginTop, cs.marginBottom];
      }),
    );
    expect(margins.length).toBe(2);
    for (const m of margins) expect(m).toEqual(["0px", "0px"]);
  });
});

test.describe("link colour", () => {
  const link = (page: Page) => page.locator("article p a").first();
  const color = (page: Page) => link(page).evaluate((a) => getComputedStyle(a).color);

  test("rests in the text colour and takes the accent on hover and focus", async ({ page }) => {
    await open(page, "/links/");
    const text = await page.locator("article p").first().evaluate((p) => getComputedStyle(p).color);
    await expect.poll(() => color(page)).toBe(text);
    await link(page).hover();
    await expect.poll(() => color(page)).toBe("rgb(45, 90, 45)");
    await page.mouse.move(0, 0);
    await link(page).focus();
    await expect.poll(() => color(page)).toBe("rgb(45, 90, 45)");
  });

  test("--moss-link-color and --moss-link-hover-color set the resting and the hover colour", async ({ page }) => {
    await open(page, "/links/");
    await page.addStyleTag({ content: ":root { --moss-link-color: rgb(10, 20, 30); --moss-link-hover-color: rgb(200, 100, 50); }" });
    await expect.poll(() => color(page)).toBe("rgb(10, 20, 30)");
    await link(page).hover();
    await expect.poll(() => color(page)).toBe("rgb(200, 100, 50)");
    await page.mouse.move(0, 0);
    await link(page).focus();
    await expect.poll(() => color(page)).toBe("rgb(200, 100, 50)");
  });

  test("the hover token also wins in the dark theme", async ({ page }) => {
    await open(page, "/links/");
    await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
    const accentHover = await page.evaluate(() => {
      const probe = document.createElement("i");
      probe.style.color = "var(--moss-color-accent-hover)";
      document.body.append(probe);
      return getComputedStyle(probe).color;
    });
    await link(page).hover();
    await expect.poll(() => color(page)).toBe(accentHover);
    await page.addStyleTag({ content: ":root { --moss-link-hover-color: rgb(200, 100, 50); }" });
    await expect.poll(() => color(page)).toBe("rgb(200, 100, 50)");
  });
});

test.describe("a year range in a heading", () => {
  test("is never split across lines, at 390px, under a long name", async ({ page }) => {
    await open(page, "/range/", 390);
    const runs = await page.$$eval("article .moss-nobreak", (spans) =>
      spans.map((s) => ({ text: s.textContent, boxes: s.getClientRects().length, heading: s.closest("h1, h2")!.tagName })),
    );
    expect(runs.map((r) => r.text)).toEqual(["1626–1705", "1626–1705", "1558-1603"]);
    for (const r of runs) expect(r.boxes, `${r.text} in ${r.heading}`).toBe(1);
  });

  test("leaves a heading without a range as it was", async ({ page }) => {
    await open(page, "/range/", 390);
    expect(await page.locator("h2#plain-heading .moss-nobreak").count()).toBe(0);
  });

  test("the hidden anchor takes no room, so a heading wraps as it would without it", async ({ page }) => {
    // The anchor's room only matters on the width where the last line is just
    // full, so sweep every width a phone-to-tablet heading wraps at.
    await open(page, "/range/", 390);
    const heading = page.locator("h2").first();
    for (let width = 240; width <= 440; width += 2) {
      await page.setViewportSize({ width, height: 900 });
      const heights = await heading.evaluate((h) => {
        const anchor = h.querySelector<HTMLElement>(".moss-heading-anchor")!;
        const withAnchor = h.getBoundingClientRect().height;
        anchor.style.display = "none";
        const without = h.getBoundingClientRect().height;
        anchor.style.display = "";
        return { withAnchor, without };
      });
      expect(heights.withAnchor, `${width}px`).toBe(heights.without);
    }
  });

  test("the anchor still shows a # on hover", async ({ page }) => {
    await open(page, "/range/", 390);
    const h = page.locator("h2").first();
    await h.hover();
    const anchor = h.locator(".moss-heading-anchor");
    await expect.poll(() => anchor.evaluate((a) => getComputedStyle(a).opacity)).toBe("1");
    const shown = await anchor.evaluate((a) => ({ glyph: getComputedStyle(a, "::after").content, box: a.getBoundingClientRect().width }));
    expect(shown.glyph).toBe('"#"');
    expect(shown.box).toBeGreaterThan(0);
  });
});

test.describe("a subtitle beside the title", () => {
  const shapes = ["inline-block"] as const;

  async function geometry(page: Page) {
    return page.evaluate(() => {
      const title = document.querySelector("article > h1")!;
      const sub = document.querySelector(".moss-subtitle")!;
      const range = document.createRange();
      range.selectNodeContents(title);
      const text = range.getBoundingClientRect();
      const controls = document.querySelector(".nav-icons")!.getBoundingClientRect();
      const lines = [...title.getClientRects(), ...sub.getClientRects(), ...range.getClientRects()];
      return {
        gap: sub.getBoundingClientRect().left - text.right,
        sameLine: Math.abs(sub.getBoundingClientRect().top - text.top) < 40,
        controls: { left: controls.left, right: controls.right, top: controls.top, bottom: controls.bottom },
        lines: lines.map((r) => ({ left: r.left, right: r.right, top: r.top, bottom: r.bottom })),
      };
    });
  }

  for (const shape of shapes) {
    test(`a short title and its ${shape} subtitle sit together, with no gap for the controls`, async ({ page }) => {
      for (const width of [390, 1360]) {
        await open(page, "/short/", width);
        await page.addStyleTag({ content: `.moss-folder-title, .moss-article-title, .moss-subtitle { display: ${shape}; }` });
        const g = await geometry(page);
        expect(g.sameLine, `${shape} at ${width}px`).toBe(true);
        expect(g.gap, `${shape} at ${width}px`).toBeLessThan(16);
      }
    });

    test(`a long title and its ${shape} subtitle wrap before the controls`, async ({ page }) => {
      for (const width of [390, 1360]) {
        await open(page, "/long/", width);
        await page.addStyleTag({ content: `.moss-folder-title, .moss-article-title, .moss-subtitle { display: ${shape}; }` });
        const g = await geometry(page);
        const c = g.controls;
        for (const r of g.lines) {
          const sharesRow = r.bottom > c.top && r.top < c.bottom;
          if (sharesRow && r.right > 0) expect(r.right, `${shape} at ${width}px, line at y=${r.top}`).toBeLessThanOrEqual(c.left + 0.5);
        }
      }
    });
  }
});

test.describe("the room kept for the controls", () => {
  for (const width of [1280, 390]) {
    test(`a hero page's hidden title keeps none, at ${width}px`, async ({ page }) => {
      await open(page, "/hero/", width);
      const w = await page.evaluate(() => {
        const grid = document.querySelector(".moss-grid")!.getBoundingClientRect().width;
        const after = [...document.querySelectorAll("article > p")].pop()!.getBoundingClientRect().width;
        return { grid, after };
      });
      expect(w.grid).toBeCloseTo(w.after, 0);
    });
  }
});

test.describe("equal rows", () => {
  const tops = (page: Page, grid: number) =>
    page.locator(".moss-grid").nth(grid).evaluate((g) => {
      const read = (sel: string) => [...g.querySelectorAll(`:scope > .moss-grid-card ${sel}`)].map((e) => Math.round(e.getBoundingClientRect().top));
      return {
        title: [...g.querySelectorAll(":scope > .moss-grid-card")].map((c) => Math.round(c.querySelector("h3")!.getBoundingClientRect().top)),
        text: read("> p:not(:has(img))"),
        height: [...g.querySelectorAll(":scope > .moss-grid-card")].map((c) => Math.round(c.getBoundingClientRect().height)),
      };
    });

  test("{rows=equal} lines the titles and the texts of cards with different shapes up across the row", async ({ page }) => {
    await open(page, "/rows/", 1280);
    const grid = await page.locator(".moss-grid").first().getAttribute("data-rows");
    expect(grid).toBe("equal");
    const t = await tops(page, 0);
    // Card two's title runs to several lines, so without shared tracks its text starts lower than card one's.
    expect(t.title[0]).toBe(t.title[1]);
    expect(t.text[0]).toBe(t.text[1]);
    expect(t.height[0]).toBe(t.height[1]);
    expect(t.height[1]).toBe(t.height[2]);
  });

  test("a collection card among prose cards shares the row's tracks", async ({ page }) => {
    await open(page, "/mixed/", 1280);
    const g = await page.locator(".moss-grid").evaluate((grid) => {
      const prose = grid.querySelector(":scope > .moss-grid-card")!;
      const card = grid.querySelector(":scope > .moss-card")!;
      const top = (e: Element | null) => Math.round(e!.getBoundingClientRect().top);
      return {
        display: getComputedStyle(card).display,
        cover: top(card.querySelector(".moss-card-cover")),
        picture: top(prose.querySelector("figure")),
        content: top(card.querySelector(".moss-card-content")),
        title: top(prose.querySelector("h3")) - parseFloat(getComputedStyle(prose.querySelector("h3")!).marginTop),
        heights: [...grid.children].map((c) => Math.round(c.getBoundingClientRect().height)),
      };
    });
    expect(g.display).toBe("grid");
    expect(Math.abs(g.content - g.title)).toBeLessThanOrEqual(1);
    expect(new Set(g.heights).size).toBe(1);
  });

  test("counts the children each card renders, so a wrapped card adds no empty tracks", async ({ page }) => {
    await open(page, "/wrapped/", 1280);
    const parts = await page.locator(".moss-grid").evaluate((g) => g.style.getPropertyValue("--moss-grid-parts"));
    expect(parts).toBe("3");
  });

  test("the parts of a card sit 10px apart and the grid's row gap is the space between cards", async ({ page }) => {
    await open(page, "/wrapped/", 1280);
    await page.addStyleTag({ content: ".moss-grid { row-gap: 30px; }" });
    const m = await page.locator(".moss-grid").evaluate((g) => {
      const cards = [...g.querySelectorAll(":scope > .moss-grid-card")];
      // The second row, so the first card's wrapped neighbour does not set the track heights.
      const parts = [...cards[2].querySelectorAll(":scope > .part")].map((e) => e.getBoundingClientRect());
      return {
        inside: [parts[1].top - parts[0].bottom, parts[2].top - parts[1].bottom],
        between: cards[2].getBoundingClientRect().top - cards[0].getBoundingClientRect().bottom,
      };
    });
    expect(m.inside).toEqual([10, 10]);
    expect(m.between).toBe(30);
  });

  test("a grid without it keeps each card's own height", async ({ page }) => {
    await open(page, "/rows/", 1280);
    const t = await tops(page, 1);
    expect(t.text[0], `texts ${t.text}`).not.toBe(t.text[1]);
  });

  test("the rows stay equal on a phone, one card per row", async ({ page }) => {
    await open(page, "/rows/", 390);
    const rects = await page.locator(".moss-grid").first().evaluate((g) =>
      [...g.querySelectorAll(":scope > .moss-grid-card")].map((c) => {
        const r = c.getBoundingClientRect();
        return { top: r.top, bottom: r.bottom };
      }),
    );
    for (let i = 1; i < rects.length; i++) expect(rects[i].top).toBeGreaterThanOrEqual(rects[i - 1].bottom - 0.5);
  });
});

test.describe("a collage quote in Chinese or Japanese", () => {
  const quote = (page: Page, text: string) =>
    page.locator(".moss-quote blockquote", { hasText: text }).evaluate((b) => {
      const cs = getComputedStyle(b);
      return { size: parseFloat(cs.fontSize), spacing: cs.letterSpacing, line: parseFloat(cs.lineHeight), face: cs.fontFamily, lang: b.closest("[lang]")?.getAttribute("lang") ?? "" };
    });

  test("is set in the CJK face at the design's size, line height and tracking", async ({ page }) => {
    await open(page, "/quotes/", 1360);
    for (const text of ["海天一色", "朝の港に"]) {
      const q = await quote(page, text);
      expect(q.size).toBeCloseTo(Math.min(21, Math.max(17, 13.6 * 1.5)), 1);
      expect(parseFloat(q.spacing)).toBeCloseTo(q.size * 0.08, 1);
      expect(q.line).toBeCloseTo(q.size * 1.6, 1);
      expect(q.face).toContain("Source Han Serif");
    }
    expect((await quote(page, "朝の港に")).lang).toBe("ja");
    expect((await quote(page, "朝の港に")).face).toMatch(/^"?Hiragino Mincho ProN/);
  });

  test("a long one keeps the smaller size, scaling with its tile", async ({ page }) => {
    await open(page, "/quotes/", 1360);
    const short = await quote(page, "海天一色，燈塔獨明。");
    const long = await page.locator(".moss-quote[data-length='long'] blockquote").evaluate((b) => parseFloat(getComputedStyle(b).fontSize));
    expect(long).toBeLessThan(short.size);
    expect(long).toBeLessThanOrEqual(15);
  });

  test("is 16px on a phone", async ({ page }) => {
    await open(page, "/quotes/", 390);
    expect((await quote(page, "海天一色")).size).toBe(16);
  });

  test("--moss-font-cjk sets the face, and a Latin quote is left alone", async ({ page }) => {
    await open(page, "/quotes/", 1360);
    const before = await quote(page, "Communication is health");
    expect(before.face).not.toContain("Source Han Serif");
    expect(before.spacing).toBe("normal");
    await page.addStyleTag({ content: ":root { --moss-font-cjk: 'Invented Face', serif; }" });
    expect((await quote(page, "海天一色")).face).toContain("Invented Face");
    expect((await quote(page, "Communication is health")).face).toBe(before.face);
  });
});
