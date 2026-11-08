/**
 * `place = "end"` on a page field: the linked page is a card at the end of the
 * article. Only a real engine can say it is painted after the body, its
 * pictures loaded, and that it does not widen the page.
 */
import { test, expect } from "@playwright/test";

const PAGE = "/work/mill-house/";

for (const width of [1280, 390, 360]) {
  test(`at ${width}px the card ends the article, its pictures are painted, and the page does not scroll sideways`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto(PAGE);
    const section = page.locator("article section.moss-field-end[data-field='next']");
    await expect(section).toHaveCount(1);
    await section.scrollIntoViewIfNeeded();
    await expect(section.locator("h2.moss-field-end-label")).toHaveText("next project");

    const card = section.locator("a.moss-card");
    await expect(card).toHaveAttribute("href", "/work/harbour-pavilion/");
    await expect(card.locator(".moss-card-title")).toHaveText("Can a pier be a room?");
    await expect(card).toBeVisible();

    const pictures = card.locator(".moss-card-pictures img");
    await expect(pictures).toHaveCount(2);
    for (let i = 0; i < 2; i++) {
      const img = pictures.nth(i);
      await expect(img).toBeVisible();
      expect(await img.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth > 0)).toBe(true);
    }

    // Below the last paragraph of the body, and inside the viewport's width.
    const last = (await page.locator("article p", { hasText: "Paragraph 12" }).boundingBox())!;
    const box = (await card.boundingBox())!;
    expect(box.y).toBeGreaterThan(last.y + last.height);
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width + 1);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  });
}

test("a page without a value draws no section", async ({ page }) => {
  await page.goto("/work/harbour-pavilion/");
  await expect(page.locator(".moss-field-end")).toHaveCount(0);
});
