/**
 * A hero that rotates through several pictures needs a way to stop (WCAG 2.2
 * SC 2.2.2). The control is a checkbox and the stylesheet reads it, so every
 * claim here is about the cascade and painted layout:
 *
 *  - checking it pauses the slides' animation, and unchecking resumes it
 *    WHILE the box still holds focus (a `:focus-within` rule once kept the
 *    slides paused until focus left);
 *  - keyboard focus on a link in the overlay text pauses too;
 *  - on a phone with `mobile=overlay` the full-width panel sits at the block
 *    end, so the control must be the top element at its own centre or it
 *    cannot be tapped;
 *  - reduced motion hides it, since nothing moves.
 *
 * The Rust side pins the markup: `rotating_hero_carries_a_labelled_pause_control_and_a_single_image_does_not`.
 *
 * Site: tests/e2e/helpers/gate-sites.ts -> HERO_PAUSE_GATE.
 */
import { test, expect, type Page } from "@playwright/test";

const PAUSE = ".moss-hero-pause";
const INPUT = ".moss-hero-pause input";

const playState = (page: Page) =>
  page
    .locator(".moss-hero-slide")
    .nth(1)
    .evaluate((el) => getComputedStyle(el).animationPlayState);

test.describe("hero pause control", () => {
  test("checking pauses the slides; unchecking resumes while the box is still focused", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    expect(await playState(page)).toBe("running");

    await page.locator(INPUT).focus();
    await page.keyboard.press("Space");
    await expect(page.locator(INPUT)).toBeChecked();
    expect(await playState(page)).toBe("paused");

    await page.keyboard.press("Space");
    await expect(page.locator(INPUT)).not.toBeChecked();
    expect(await page.evaluate(() => document.activeElement?.matches(".moss-hero-pause input"))).toBe(true);
    expect(await playState(page)).toBe("running");
  });

  test("keyboard focus on a link in the overlay pauses the slides", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    await page.keyboard.press("Tab"); // switch to keyboard modality
    await page.locator(".moss-hero-content a").focus();
    expect(await playState(page)).toBe("paused");
    await page.locator("body").evaluate(() => (document.activeElement as HTMLElement).blur());
    expect(await playState(page)).toBe("running");
  });

  test("on a phone with mobile=overlay the control is the top element at its centre", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/");
    const box = (await page.locator(PAUSE).boundingBox())!;
    expect(box.width).toBeGreaterThanOrEqual(44);
    expect(box.height).toBeGreaterThanOrEqual(44);
    const topIsControl = await page.evaluate(
      ([x, y]) => !!document.elementFromPoint(x, y)?.closest(".moss-hero-pause"),
      [box.x + box.width / 2, box.y + box.height / 2],
    );
    expect(topIsControl, "something paints over the pause control at its centre").toBe(true);
  });

  test("align=end moves the control to the inline-start corner", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/align-end/");
    const hero = (await page.locator(".moss-hero").boundingBox())!;
    const box = (await page.locator(PAUSE).boundingBox())!;
    expect(box.x - hero.x).toBeLessThan(hero.width / 2);
  });

  test("reduced motion hides the control", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await expect(page.locator(PAUSE)).toBeHidden();
  });
});
