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
});
