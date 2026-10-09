/**
 * A multi-slide hero's captions, overlay content and pause control never
 * overlap, whatever the width, `align` or `mobile` mode — and a site's
 * `--moss-hero-max-height` lifts the phone overlay floor too.
 *
 * Boxes are only known once the engine lays them out, so this is a render
 * gate. Site: gate-sites.ts → heroCaptionStackGate (end / start / stacked).
 */
import { test, expect, type Page } from "@playwright/test";

type Box = { x: number; y: number; width: number; height: number };

async function boxes(page: Page) {
  const box = async (sel: string): Promise<Box | null> => page.locator(sel).first().boundingBox();
  return {
    hero: (await box(".moss-hero"))!,
    caption: (await box('.moss-hero-caption[data-slide="0"]'))!,
    content: (await box(".moss-hero-content"))!,
    pause: await box(".moss-hero-pause"),
  };
}

function overlap(a: Box, b: Box) {
  return a.x < b.x + b.width - 0.5 && b.x < a.x + a.width - 0.5 && a.y < b.y + b.height - 0.5 && b.y < a.y + a.height - 0.5;
}

const SIZES = [
  { name: "desktop", width: 1280, height: 800 },
  { name: "phone", width: 390, height: 844 },
];

for (const path of ["/end/", "/start/", "/stacked/"]) {
  for (const size of SIZES) {
    test(`caption, content and pause do not overlap (${path}, ${size.name})`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await page.goto(path);
      await page.locator(".moss-hero-pause").waitFor({ state: "attached", timeout: 5000 }).catch(() => {});
      const b = await boxes(page);
      expect(b.caption.width).toBeGreaterThan(0);
      expect(overlap(b.caption, b.content), "caption vs content").toBe(false);
      if (b.pause) {
        expect(overlap(b.pause, b.caption), "pause vs caption").toBe(false);
        expect(overlap(b.pause, b.content), "pause vs content").toBe(false);
      }
    });
  }
}

test("stacked phone: the caption is a credit between the picture and the text block", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/stacked/");
  const b = await boxes(page);
  const media = (await page.locator(".moss-hero video").first().boundingBox())!;
  expect(b.caption.y).toBeGreaterThanOrEqual(media.y + media.height - 1);
  expect(b.caption.y + b.caption.height).toBeLessThanOrEqual(b.content.y + 1);
  const bg = await page.locator(".moss-hero-captions").evaluate((el) => getComputedStyle(el).backgroundImage);
  expect(bg).toBe("none");
});

test("phone overlay: the caption sits directly above the content, over the picture", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/start/");
  const b = await boxes(page);
  expect(b.caption.y + b.caption.height).toBeLessThanOrEqual(b.content.y + 1);
  expect(b.caption.y).toBeGreaterThanOrEqual(b.hero.y);
});

test("phone overlay honours --moss-hero-max-height as its floor", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/start/");
  await page.addStyleTag({ content: ":root{--moss-hero-max-height:100svh}" });
  const hero = (await page.locator(".moss-hero").boundingBox())!;
  expect(hero.height).toBeGreaterThanOrEqual(844 - 1);
});
