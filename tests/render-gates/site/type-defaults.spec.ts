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

