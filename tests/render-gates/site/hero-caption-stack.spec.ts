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

/**
 * Layout gates measure boxes, not playback. Decoding a hero clip on a headless
 * WebKit with no GPU (a hosted macOS runner) wedges the page, so these tests
 * load the page without the clips' media; the tests that need a clip to play
 * use page.goto directly.
 */
async function goLayoutOnly(page: Page, path: string): Promise<void> {
  await page.route(/\.(m4s|mp4)(\?|$)/, (route) => route.abort());
  await page.goto(path);
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
      await goLayoutOnly(page, path);
      const b = await boxes(page);
      expect(b.caption.width).toBeGreaterThan(0);
      expect(overlap(b.caption, b.content), "caption vs content").toBe(false);
      expect(b.pause, "the pause control is rendered").not.toBeNull();
      expect(overlap(b.pause!, b.caption), "pause vs caption").toBe(false);
      expect(overlap(b.pause!, b.content), "pause vs content").toBe(false);
    });
  }
}

test("stacked phone: the caption is a credit between the picture and the text block", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await goLayoutOnly(page, "/stacked/");
  const b = await boxes(page);
  const media = (await page.locator(".moss-hero video").first().boundingBox())!;
  expect(b.caption.y).toBeGreaterThanOrEqual(media.y + media.height - 1);
  expect(b.caption.y + b.caption.height).toBeLessThanOrEqual(b.content.y + 1);
  const bg = await page.locator(".moss-hero-captions").evaluate((el) => getComputedStyle(el).backgroundImage);
  expect(bg).toBe("none");
});

test("phone overlay: the caption sits directly above the content, over the picture", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await goLayoutOnly(page, "/start/");
  const b = await boxes(page);
  expect(b.caption.y + b.caption.height).toBeLessThanOrEqual(b.content.y + 1);
  expect(b.caption.y).toBeGreaterThanOrEqual(b.hero.y);
});

test("phone overlay honours --moss-hero-max-height as its floor", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await goLayoutOnly(page, "/start/");
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
      await goLayoutOnly(page, path);
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
  for (const [path, size] of [["/start/", { width: 390, height: 844 }], ["/start/", { width: 1280, height: 800 }], ["/end/", { width: 1280, height: 800 }], ["/end/", { width: 390, height: 844 }]] as const) {
    await page.setViewportSize(size);
    await goLayoutOnly(page, path);
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
    await goLayoutOnly(page, path);
    const pause = (await page.locator(".moss-hero-pause").boundingBox())!;
    const buttons = page.locator("header button, header a");
    for (let i = 0; i < (await buttons.count()); i++) {
      const b = await buttons.nth(i).boundingBox();
      if (b && b.width > 0) expect(overlap(pause, b), `${path} pause vs header control ${i}`).toBe(false);
    }
  }
});

// ── The sequence advances, honours reduced motion, and a pale cover keeps a scrim ──

test("a sequence moves to the next clip and caption when a video ends", async ({ page }) => {
  await page.goto("/start/");
  const hero = page.locator(".moss-hero");
  await expect(page.locator('.moss-hero-caption[data-slide="0"]')).toBeVisible();
  await page.locator('.moss-hero-slide[data-slide="0"] video').evaluate((v: HTMLVideoElement) => {
    v.currentTime = Math.max(0, v.duration - 0.3);
  });
  await expect(page.locator('.moss-hero-caption[data-slide="1"]')).toBeVisible({ timeout: 10000 });
  await expect(page.locator('.moss-hero-caption[data-slide="0"]')).toBeHidden();
  await expect(page.locator('.moss-hero-slide[data-slide="1"]')).toHaveAttribute("data-current", "");
  await expect(page.locator('.moss-hero-slide[data-slide="0"]')).not.toHaveAttribute("data-current", "");
  await expect(hero).toBeVisible();
});

test("reduced motion: the first clip and caption stay, nothing plays, no pause control", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await goLayoutOnly(page, "/start/");
  await page.waitForLoadState("load");
  await expect(page.locator('.moss-hero-caption[data-slide="0"]')).toBeVisible();
  await expect(page.locator('.moss-hero-caption[data-slide="1"]')).toBeHidden();
  await expect(page.locator(".moss-hero-pause")).toBeHidden();
  for (const v of await page.locator(".moss-hero video").all()) {
    expect(await v.evaluate((el: HTMLVideoElement) => el.paused)).toBe(true);
  }
});

test("a pale cover has no hero scrim, so the captions keep their own gradient", async ({ page }) => {
  for (const size of [{ width: 1280, height: 800 }, { width: 390, height: 844 }]) {
    await page.setViewportSize(size);
    await goLayoutOnly(page, "/start/");
    await page.locator(".moss-hero").evaluate((el) => (el.dataset.heroTone = "light"));
    const bg = await page.locator(".moss-hero-captions").evaluate((el) => getComputedStyle(el).backgroundImage);
    expect(bg, `${size.width}`).toContain("linear-gradient");
    const before = await page.locator(".moss-hero").evaluate((el) => getComputedStyle(el, "::before").content);
    expect(before).toBe("none");
  }
});

// ── Clip indicator of a sequence hero ────────────────────────────────────────
const SEGMENT = ".moss-hero-indicator-segment";

for (const path of ["/end/", "/start/", "/stacked/", "/three/"]) {
  for (const size of SIZES) {
    test(`the clip indicator overlaps nothing (${path}, ${size.name})`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await goLayoutOnly(page, path);
      await page.locator(SEGMENT).first().waitFor({ state: "attached", timeout: 5000 });
      const n = await page.locator(SEGMENT).count();
      expect(n).toBe(path === "/three/" ? 3 : 2);
      const group = (await page.locator(".moss-hero-indicator").boundingBox())!;
      const b = await boxes(page);
      expect(overlap(group, b.caption), "indicator vs caption").toBe(false);
      expect(overlap(group, b.content), "indicator vs content").toBe(false);
      if (b.pause) expect(overlap(group, b.pause), "indicator vs pause").toBe(false);
      const header = await page.locator("header").first().boundingBox();
      if (header) expect(overlap(group, header), "indicator vs header").toBe(false);
      // Above the caption, at its inline start.
      expect(group.y + group.height).toBeLessThanOrEqual(b.caption.y + 1);
      const seg = (await page.locator(SEGMENT).first().boundingBox())!;
      expect(seg.width).toBeGreaterThanOrEqual(24);
      expect(seg.height).toBeGreaterThanOrEqual(24);
      expect(Math.abs(seg.x - b.caption.x)).toBeLessThan(8);
    });
  }
}

test("the segments are labelled and the playing one is aria-current", async ({ page }) => {
  await goLayoutOnly(page, "/three/");
  const segs = page.locator(SEGMENT);
  await expect(segs).toHaveCount(3);
  await expect(segs.nth(1)).toHaveAttribute("aria-label", /^Clip 2 of 3: Title two/);
  await expect(segs.nth(0)).toHaveAttribute("aria-current", "true");
  await expect(segs.nth(1)).not.toHaveAttribute("aria-current", "true");
});

test("clicking segment 3 plays clip 3, shows caption 3 and moves aria-current", async ({ page }) => {
  await page.goto("/three/");
  const segs = page.locator(SEGMENT);
  await segs.nth(2).click();
  await expect(page.locator('.moss-hero-slide[data-slide="2"]')).toHaveAttribute("data-current", "");
  await expect(page.locator('.moss-hero-caption[data-slide="2"]')).toBeVisible();
  await expect(page.locator('.moss-hero-caption[data-slide="0"]')).toBeHidden();
  await expect(segs.nth(2)).toHaveAttribute("aria-current", "true");
  await expect(segs.nth(0)).not.toHaveAttribute("aria-current", "true");
  await expect
    .poll(() => page.locator('.moss-hero-slide[data-slide="2"] video').evaluate((v: HTMLVideoElement) => !v.paused))
    .toBe(true);
});

test("the keyboard jumps too, and a paused sequence stays paused on the chosen clip", async ({ page }) => {
  await page.goto("/three/");
  const segs = page.locator(SEGMENT);
  await segs.nth(1).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator('.moss-hero-slide[data-slide="1"]')).toHaveAttribute("data-current", "");
  await page.locator(".moss-hero-pause").click();
  await segs.nth(2).focus();
  await page.keyboard.press("Space");
  await expect(page.locator('.moss-hero-slide[data-slide="2"]')).toHaveAttribute("data-current", "");
  await expect(segs.nth(2)).toHaveAttribute("aria-current", "true");
  expect(await page.locator('.moss-hero-slide[data-slide="2"] video').evaluate((v: HTMLVideoElement) => v.paused)).toBe(true);
  await expect(page.locator(".moss-hero-pause input")).toBeChecked();
});

test("the playing segment's bar fills from the video's time", async ({ page }) => {
  await page.goto("/three/");
  await expect
    .poll(() => page.locator(SEGMENT).first().evaluate((el) => Number(el.style.getPropertyValue("--moss-hero-progress"))))
    .toBeGreaterThan(0);
});

test("reduced motion: no indicator (nothing plays, so nothing to jump between)", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await goLayoutOnly(page, "/three/");
  await page.waitForLoadState("load");
  await expect(page.locator(SEGMENT)).toHaveCount(0);
});
