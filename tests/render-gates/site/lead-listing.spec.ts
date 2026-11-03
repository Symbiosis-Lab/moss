/**
 * `children_style: lead`: the first card leads (picture beside its words on a
 * wide screen, stacked on a phone) and the rest form a register. Only a real
 * engine can say that the picture is painted at all: WebKit draws a grid item
 * that has an aspect ratio and only absolutely placed content 0 px wide unless
 * it has an explicit inline-size.
 */
import { test, expect } from "@playwright/test";

const WIDE = { width: 1440, height: 900 };
const PHONE = { width: 390, height: 844 };

for (const [name, viewport] of [["wide", WIDE], ["phone", PHONE]] as const) {
  test.describe(`lead listing at ${name} width`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize(viewport);
      await page.goto("/notes/");
    });

    test("the lead picture is painted", async ({ page }) => {
      const cover = page.locator(".moss-cards[data-lead] > .moss-card:first-child .moss-card-cover");
      const box = (await cover.boundingBox())!;
      expect(box.width).toBeGreaterThan(100);
      expect(box.height).toBeGreaterThan(50);
    });

    test("the register shows no pictures and the page does not overflow sideways", async ({ page }) => {
      await expect(page.locator(".moss-cards[data-lead] > .moss-card:not(:first-child) .moss-card-cover").first()).toBeHidden();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(overflow).toBeLessThanOrEqual(0);
    });
  });
}

test("on a wide screen the lead's date line starts on the picture's top edge", async ({ page }) => {
  await page.setViewportSize(WIDE);
  await page.goto("/notes/");
  const card = page.locator(".moss-cards[data-lead] > .moss-card:first-child");
  const cover = (await card.locator(".moss-card-cover").boundingBox())!;
  const title = (await card.locator(".moss-card-title").boundingBox())!;
  const metaLine = card.locator(".moss-card-body > .moss-card-meta").first();
  const meta = (await metaLine.boundingBox())!;
  expect(meta.y).toBeLessThan(title.y);
  // The date line's box is pulled up by its half-leading, so the box top plus
  // the half-leading meets the picture's top. Untrimmed, this is off by the
  // half-leading itself (about 3.5px), so the tolerance must stay below it.
  const halfLeading = await metaLine.evaluate((el) => {
    const cs = getComputedStyle(el);
    return (parseFloat(cs.lineHeight) - parseFloat(cs.fontSize)) / 2;
  });
  expect(Math.abs(meta.y + halfLeading - cover.y)).toBeLessThanOrEqual(1);
});

test("on a phone the lead's picture sits above its words", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto("/notes/");
  const card = page.locator(".moss-cards[data-lead] > .moss-card:first-child");
  const cover = (await card.locator(".moss-card-cover").boundingBox())!;
  const title = (await card.locator(".moss-card-title").boundingBox())!;
  expect(cover.y + cover.height).toBeLessThanOrEqual(title.y);
});

test("on a wide screen the register is three equal columns", async ({ page }) => {
  await page.setViewportSize(WIDE);
  await page.goto("/notes/");
  const boxes = await page
    .locator(".moss-cards[data-lead] > .moss-card:not(:first-child)")
    .evaluateAll((els) => els.map((e) => e.getBoundingClientRect()));
  expect(new Set(boxes.map((b) => Math.round(b.x))).size).toBe(3);
  const widths = boxes.map((b) => b.width);
  expect(Math.max(...widths) - Math.min(...widths)).toBeLessThanOrEqual(1);
});
