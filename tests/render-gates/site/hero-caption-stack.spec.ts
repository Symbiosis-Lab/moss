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

// Every slide fills the hero when a site makes it taller than the video's own
// 16:9 shape (`--moss-hero-max-height: 100svh`): no band without a picture.
for (const path of ["/end/", "/start/", "/images/"]) {
  for (const size of [{ width: 1280, height: 800 }, { width: 1440, height: 900 }, { width: 390, height: 844 }]) {
    test(`slides fill a 100svh hero (${path}, ${size.width})`, async ({ page }) => {
      await page.setViewportSize(size);
      await page.goto(path);
      await page.addStyleTag({ content: ":root{--moss-hero-max-height:100svh}" });
      const hero = (await page.locator(".moss-hero").boundingBox())!;
      for (const sel of [".moss-hero-slide", ".moss-hero-slide :is(video, img)"]) {
        const el = page.locator(sel).first();
        const b = (await el.boundingBox())!;
        expect(b.height, `${sel} height`).toBeGreaterThanOrEqual(hero.height - 1);
        expect(b.width, `${sel} width`).toBeGreaterThanOrEqual(hero.width - 1);
      }
    });
  }
}

test("in-flow captions paint no scrim of their own; the hero's scrim and a text shadow carry them", async ({ page }) => {
  for (const [path, size] of [["/start/", { width: 390, height: 844 }], ["/start/", { width: 1280, height: 800 }]] as const) {
    await page.setViewportSize(size);
    await page.goto(path);
    const cs = await page.locator(".moss-hero-captions").evaluate((el) => getComputedStyle(el).backgroundImage);
    expect(cs, `${path} ${size.width}`).toBe("none");
    const shadow = await page.locator('.moss-hero-caption[data-slide="0"]').evaluate((el) => getComputedStyle(el).textShadow);
    expect(shadow).not.toBe("none");
    const before = await page.locator(".moss-hero").evaluate((el) => getComputedStyle(el, "::before").content);
    expect(before).not.toBe("none");
  }
});

test("phone, header=over: the pause control is never under the header's buttons", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  for (const path of ["/end/", "/start/", "/stacked/"]) {
    await page.goto(path);
    await page.locator(".moss-hero-pause").waitFor({ state: "attached", timeout: 5000 });
    const pause = (await page.locator(".moss-hero-pause").boundingBox())!;
    const buttons = page.locator("header button, header a");
    for (let i = 0; i < (await buttons.count()); i++) {
      const b = await buttons.nth(i).boundingBox();
      if (b && b.width > 0) expect(overlap(pause, b), `${path} pause vs header control ${i}`).toBe(false);
    }
  }
});
