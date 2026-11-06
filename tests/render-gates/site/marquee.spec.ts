/**
 * A picture strip that slides sideways in a loop: a listing row's
 * (`motion:marquee`) and a gallery's (`:::gallery marquee`). Only a real
 * engine can say the track moves, that the copy lines up with the real set,
 * and that the loop never widens the page.
 */
import { test, expect, type Page } from "@playwright/test";

const SIZES = [
  { width: 1280, height: 900 },
  { width: 390, height: 844 },
  { width: 360, height: 800 },
];

/** The strips that actually loop (a strip too short to fill its box is still). */
const strips = (page: Page) => page.locator("[data-motion='marquee'][data-marquee-ready]:not([data-marquee-still])");
const trackOf = (strip: ReturnType<Page["locator"]>) => strip.locator("> .moss-marquee-track");

/** The track's horizontal translation, in pixels. */
const shift = (track: ReturnType<Page["locator"]>) =>
  track.evaluate((el) => new DOMMatrixReadOnly(getComputedStyle(el).transform).m41);

async function ready(page: Page) {
  await page.goto("/");
  await expect(strips(page)).toHaveCount(4);
}

test("the track slides, linearly, at the speed the token names", async ({ page }) => {
  await ready(page);
  const track = trackOf(strips(page).first());
  const a = await shift(track);
  await page.waitForTimeout(1000);
  const b = await shift(track);
  // 60 px a second by default; engines schedule loosely, so a wide band.
  expect(a - b).toBeGreaterThan(30);
  expect(a - b).toBeLessThan(120);
  const timing = await track.evaluate((el) => getComputedStyle(el).animationTimingFunction);
  expect(timing).toBe("linear");
});

test("the copy is as wide as the real set, so the loop has no seam", async ({ page }) => {
  await ready(page);
  const count = await strips(page).count();
  for (let i = 0; i < count; i++) {
    const strip = strips(page).nth(i);
    const [real, copy] = await strip.locator(".moss-marquee-set").evaluateAll((els) => els.map((el) => el.getBoundingClientRect().width));
    expect(real).toBeGreaterThan(200);
    expect(Math.abs(real - copy)).toBeLessThanOrEqual(1);
    const track = await trackOf(strip).evaluate((el) => el.getBoundingClientRect().width);
    expect(Math.abs(track - 2 * real)).toBeLessThanOrEqual(2);
  }
});

test("adjacent strips run opposite ways", async ({ page }) => {
  await ready(page);
  const deltas: number[] = [];
  const tracks = [0, 1, 2, 3].map((i) => trackOf(strips(page).nth(i)));
  const before = await Promise.all(tracks.map(shift));
  await page.waitForTimeout(700);
  const after = await Promise.all(tracks.map(shift));
  tracks.forEach((_, i) => deltas.push(after[i] - before[i]));
  for (let i = 1; i < deltas.length; i++) expect(Math.sign(deltas[i])).toBe(-Math.sign(deltas[i - 1]));
  expect(deltas[0]).toBeLessThan(0);
});

test("hovering pauses the loop by default", async ({ page }) => {
  await page.setViewportSize(SIZES[0]);
  await ready(page);
  const strip = strips(page).first();
  await strip.hover();
  const track = trackOf(strip);
  expect(await track.evaluate((el) => getComputedStyle(el).animationPlayState)).toBe("paused");
  const a = await shift(track);
  await page.waitForTimeout(400);
  expect(await shift(track)).toBe(a);
});

test("a theme can keep the loop running under the pointer", async ({ page }) => {
  await page.setViewportSize(SIZES[0]);
  await ready(page);
  await page.addStyleTag({ content: ":root { --moss-marquee-hover: running; }" });
  const strip = strips(page).first();
  await strip.hover();
  const track = trackOf(strip);
  expect(await track.evaluate((el) => getComputedStyle(el).animationPlayState)).toBe("running");
  const a = await shift(track);
  await page.waitForTimeout(400);
  expect(await shift(track)).not.toBe(a);
});

test("focus inside a strip pauses it whatever the theme says", async ({ page }) => {
  await page.setViewportSize(SIZES[0]);
  await ready(page);
  await page.addStyleTag({ content: ":root { --moss-marquee-hover: running; }" });
  // The first stop is the first row's card link, which holds its strip.
  for (let i = 0; i < 12; i++) {
    await page.keyboard.press("Tab");
    if (await page.evaluate(() => document.activeElement?.matches("a.moss-card") ?? false)) break;
  }
  const track = trackOf(strips(page).first());
  expect(await track.evaluate((el) => getComputedStyle(el).animationPlayState)).toBe("paused");
});

test("the keyboard never reaches the copy, and a screen reader meets it never", async ({ page }) => {
  await ready(page);
  const copies = await strips(page).locator(".moss-marquee-set[inert]").evaluateAll((els) =>
    els.map((el) => ({ hidden: el.getAttribute("aria-hidden"), alts: [...el.querySelectorAll("img")].map((i) => i.getAttribute("alt")) })),
  );
  expect(copies).toHaveLength(4);
  for (const c of copies) {
    expect(c.hidden).toBe("true");
    expect(c.alts.length).toBeGreaterThan(1);
    expect(c.alts.every((alt) => alt === "")).toBe(true);
  }
  const reached: boolean[] = [];
  for (let i = 0; i < 25; i++) {
    await page.keyboard.press("Tab");
    reached.push(await page.evaluate(() => !!document.activeElement?.closest("[inert]")));
  }
  expect(reached.some(Boolean)).toBe(false);
});

test("a strip narrower than its box does not animate, and its copy is not drawn", async ({ page }) => {
  for (const size of [SIZES[0], SIZES[2]]) {
    await page.setViewportSize(size);
    await ready(page);
    const short = page.locator("[data-motion='marquee'][data-marquee-still]");
    await expect(short).toHaveCount(1);
    const track = trackOf(short);
    expect(await track.evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
    expect(await short.locator(".moss-marquee-set[inert]").evaluate((el) => getComputedStyle(el).display)).toBe("none");
    expect(await short.locator(".moss-marquee-set").first().evaluate((el) => el.getBoundingClientRect().width)).toBeLessThan(
      await short.evaluate((el) => el.clientWidth),
    );
  }
});

test("the copy carries the same captions as the real set", async ({ page }) => {
  await ready(page);
  const gallery = page.locator(".moss-gallery[data-motion='marquee']").first();
  const [real, copy] = await gallery.locator(".moss-marquee-set").evaluateAll((sets) =>
    sets.map((set) => [...set.querySelectorAll(".moss-gallery-caption")].map((c) => c.textContent)),
  );
  expect(real).toEqual(["First caption", "Second caption"]);
  expect(copy).toEqual(real);
});

test("pictures that slide into view load", async ({ page }) => {
  await page.setViewportSize(SIZES[0]);
  await ready(page);
  const strip = strips(page).first();
  // Jump the loop to where the copy has just come on screen.
  await trackOf(strip).evaluate((el) => {
    const animation = el.getAnimations()[0];
    animation.pause();
    animation.currentTime = Number(animation.effect!.getComputedTiming().duration) * 0.9;
  });
  await page.waitForFunction(
    (root) => {
      const visible = [...root!.querySelectorAll("img")].filter((img) => {
        const r = img.getBoundingClientRect();
        return r.right > 0 && r.left < innerWidth;
      });
      return visible.length > 2 && visible.every((img) => img.complete && img.naturalWidth > 0);
    },
    await strip.elementHandle(),
    { timeout: 5000 },
  );
});

for (const size of SIZES.slice(1)) {
  test(`the page does not scroll sideways at ${size.width}px`, async ({ page }) => {
    await page.setViewportSize(size);
    await ready(page);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
    await page.waitForTimeout(500);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  });
}

test.describe("for a reader who asks for less motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("nothing moves, the copy is not drawn and the strip is the still one", async ({ page }) => {
    await page.setViewportSize(SIZES[0]);
    await page.goto("/");
    const strip = page.locator(".moss-card-pictures").first();
    await expect(strip.locator(".moss-card-cover").first()).toBeVisible();
    expect(await strip.locator(".moss-marquee-track").evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
    expect(await strip.locator(".moss-marquee-set[inert]").evaluate((el) => getComputedStyle(el).display)).toBe("none");
    // Five equal columns in one row, as the still strip is.
    const boxes = await strip.locator(".moss-card-cover:visible").evaluateAll((els) => els.map((el) => el.getBoundingClientRect()));
    expect(boxes).toHaveLength(5);
    for (const b of boxes) expect(Math.abs(b.width - boxes[0].width)).toBeLessThanOrEqual(1);
    const gallery = page.locator(".moss-gallery").first();
    expect(await gallery.locator(".moss-marquee-set[inert]").evaluate((el) => getComputedStyle(el).display)).toBe("none");
  });

  test("on a phone the still strip scrolls sideways and the page does not", async ({ page }) => {
    await page.setViewportSize(SIZES[1]);
    await page.goto("/");
    const strip = page.locator(".moss-card-pictures").first();
    const m = await strip.evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth, overflowX: getComputedStyle(el).overflowX }));
    expect(m.overflowX).toBe("auto");
    expect(m.scroll).toBeGreaterThan(m.client + 10);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  });
});
