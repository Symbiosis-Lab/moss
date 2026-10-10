/**
 * Tokens alone state a theme's header band, its card text and a hero-only home.
 *
 * Painted colours and laid-out boxes: the rules involved tie on specificity or
 * source order against moss's own `header=over` and cover-tint rules, which
 * jsdom does not resolve.
 *
 * See tests/e2e/helpers/gate-sites.ts → THEME_FLOOR_HOOKS_GATE.
 */
import { test, expect, Page } from "@playwright/test";

const css = (page: Page, sel: string, prop: "color" | "backgroundColor") =>
  page.evaluate(
    ([s, p]) => getComputedStyle(document.querySelector(s)!)[p as "color"],
    [sel, prop] as const,
  );

test.describe("theme floor hooks", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
  });

  test("a normal page paints the header band and link colour from the tokens", async ({ page }) => {
    await page.goto("/about/");
    expect(await css(page, "body > header", "backgroundColor")).toBe("rgb(200, 0, 0)");
    expect(await css(page, "body > header .site-name", "color")).toBe("rgb(0, 0, 255)");
  });

  test("a header=over page stays transparent with light links", async ({ page }) => {
    await page.goto("/over/");
    expect(await css(page, "body > header", "backgroundColor")).toBe("rgba(0, 0, 0, 0)");
    expect(await css(page, "body > header .site-name", "color")).toBe("rgb(255, 255, 255)");
  });

  test("a hero-only home has no empty article and no blank page above the footer", async ({ page }) => {
    await page.goto("/");
    expect(await page.locator("main#main-content").count()).toBe(1);
    expect(await page.locator("main > article").count()).toBe(0);

    const m = await page.evaluate(() => {
      const box = (s: string) => document.querySelector(s)!.getBoundingClientRect();
      const hero = document.querySelector(".moss-hero")!;
      return {
        main: box("main").height,
        gap: box("footer").top - box(".moss-hero").bottom,
        heroMargin: parseFloat(getComputedStyle(hero).marginBlockEnd),
        footerMargin: parseFloat(getComputedStyle(document.querySelector("footer")!).marginBlockStart),
      };
    });
    // Only the two margins moss gives the hero and the footer remain between them.
    expect(m.main).toBe(0);
    expect(m.gap).toBe(m.heroMargin + m.footerMargin);
  });

  test("a page with body text keeps its article", async ({ page }) => {
    await page.goto("/about/");
    expect(await page.locator("main > article.container").count()).toBe(1);
  });
});
