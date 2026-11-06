/**
 * `pictures:5` on a summary listing: each row's pictures are one strip. Only a
 * real engine can say the pictures are painted in one row and that the strip
 * does not widen the page.
 */
import { test, expect } from "@playwright/test";

const WIDE = { width: 1280, height: 900 };
const PHONE = { width: 390, height: 844 };

const overflow = (page: import("@playwright/test").Page) =>
  page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);

test("on a wide screen the five pictures share one row in equal columns", async ({ page }) => {
  await page.setViewportSize(WIDE);
  await page.goto("/");
  const strip = page.locator(".moss-card-pictures").first();
  await expect(strip.locator(".moss-card-cover")).toHaveCount(5);
  const boxes = await strip.locator(".moss-card-cover").evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    }),
  );
  for (const b of boxes) {
    expect(b.width).toBeGreaterThan(100);
    expect(Math.abs(b.y - boxes[0].y)).toBeLessThanOrEqual(1);
    expect(Math.abs(b.width - boxes[0].width)).toBeLessThanOrEqual(1);
    expect(Math.abs(b.height - boxes[0].height)).toBeLessThanOrEqual(1);
  }
  for (let i = 1; i < boxes.length; i++) expect(boxes[i].x).toBeGreaterThan(boxes[i - 1].x);
  expect(await overflow(page)).toBeLessThanOrEqual(0);
});

test("the words sit above the strip", async ({ page }) => {
  await page.setViewportSize(WIDE);
  await page.goto("/");
  const row = page.locator(".moss-card-row").first();
  const title = (await row.locator(".moss-card-title").boundingBox())!;
  const strip = (await row.locator(".moss-card-pictures").boundingBox())!;
  expect(title.y + title.height).toBeLessThanOrEqual(strip.y);
});

test("a linkblog row stacks its strip under its words too", async ({ page }) => {
  await page.setViewportSize(WIDE);
  await page.goto("/");
  const row = page.locator(".moss-card[data-linkblog] .moss-card-row").first();
  const title = (await row.locator(".moss-card-title").boundingBox())!;
  const strip = (await row.locator(".moss-card-pictures").boundingBox())!;
  expect(title.y + title.height).toBeLessThanOrEqual(strip.y);
  const covers = await row.locator(".moss-card-cover").evaluateAll((els) => els.map((el) => el.getBoundingClientRect().y));
  expect(covers).toHaveLength(5);
  for (const y of covers) expect(Math.abs(y - covers[0])).toBeLessThanOrEqual(1);
});

test("on a phone the strip scrolls sideways and the page does not", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto("/");
  expect(await overflow(page)).toBeLessThanOrEqual(0);
  const strip = page.locator(".moss-card-pictures").first();
  const m = await strip.evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth, overflowX: getComputedStyle(el).overflowX }));
  expect(m.overflowX).toBe("auto");
  expect(m.scroll).toBeGreaterThan(m.client + 10);
  const first = (await strip.locator(".moss-card-cover").first().boundingBox())!;
  // One picture fills the track less the peek of the next one.
  expect(first.width).toBeLessThan(m.client - 20);
  expect(first.width).toBeGreaterThan(m.client * 0.7);
});
