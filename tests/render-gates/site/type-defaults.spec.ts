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

