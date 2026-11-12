/**
 * A country map's levels are painted along one ramp that reads as order, in
 * both themes: lightness moves one way (darker with each level on the light
 * theme, lighter on the dark theme), every level differs from uncoloured
 * land, and the figure fits a phone. A tap on a country or a list row opens
 * the panel; Esc closes it. Under `as_of: today` the stamp shows the reader's
 * date, and the page's date without script.
 *
 * Run via:
 *   npx playwright test -c playwright/country-map.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

/** Relative luminance of a painted fill. A colour mix computes to `oklch(...)` in some engines, so the browser's own canvas converts it to sRGB first. */
async function luminanceOf(page: Page, selector: string): Promise<number> {
  const [r, g, b] = await page.locator(selector).first().evaluate((el) => {
    const ctx = document.createElement("canvas").getContext("2d")!;
    ctx.fillStyle = getComputedStyle(el).fill;
    ctx.fillRect(0, 0, 1, 1);
    return Array.from(ctx.getImageData(0, 0, 1, 1).data.slice(0, 3));
  });
  const [lr, lg, lb] = [r, g, b].map((v) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
}

for (const scheme of ["light", "dark"] as const) {
  test(`${scheme}: levels read as order and differ from land`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto("/");
    const fills = [] as number[];
    for (const slug of ["mexico", "japan", "brazil", "chile"]) fills.push(await luminanceOf(page, `.moss-country[data-slug="${slug}"]`));
    const land = await luminanceOf(page, '.moss-country[data-slug="chad"]');
    for (let i = 1; i < fills.length; i++) {
      expect(scheme === "light" ? fills[i - 1] - fills[i] : fills[i] - fills[i - 1], `step ${i} moves the right way`).toBeGreaterThan(0.01);
    }
    for (const f of fills) {
      const ratio = (Math.max(f, land) + 0.05) / (Math.min(f, land) + 0.05);
      expect(ratio, "every level keeps 3:1 against land").toBeGreaterThanOrEqual(3);
    }
  });
}

test("fits a phone without horizontal scroll", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto("/");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("a tap and a list row from the keyboard open the panel; Esc closes it", async ({ page }) => {
  await page.goto("/");
  await page.locator('.moss-country[data-slug="japan"]').click();
  const panel = page.locator(".moss-country-panel");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText("Japan");
  await expect(panel).toContainText("Calm.");
  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();
  const row = page.locator('.moss-country-list a[href="#mexico"]');
  await row.focus();
  await row.press("Enter");
  await expect(panel).toContainText("Take normal precautions.");
  await page.locator('.moss-country[data-slug="chad"]').dispatchEvent("click");
  await expect(panel).toContainText("Not rated");
  await expect(panel).toContainText("No assessment yet.");
});

test("as_of: today stamps the reader's own date; without script the page's date stands", async ({ page }) => {
  const html = await (await page.request.get("/living/")).text();
  expect(html).toContain("As of November 3, 2026</time>");
  await page.clock.setFixedTime(new Date(2030, 4, 20, 12));
  await page.goto("/living/");
  const stamp = page.locator(".moss-country-map > .moss-country-stamp time");
  await expect(stamp).toHaveText("As of May 20, 2030");
  await expect(stamp).toHaveAttribute("datetime", "2030-05-20");
  await page.locator('.moss-country[data-slug="japan"]').click();
  await expect(page.locator(".moss-country-panel .moss-country-stamp")).toHaveText("As of May 20, 2030");
});
