/**
 * A cover credit has to be readable, and a hero that names its subject has to
 * show the subject.
 *
 * Both are "only true if it is actually painted" questions, which is why they
 * are here rather than in a Rust test:
 *
 *  - `.moss-hero` is a cropping frame — a height cap plus `overflow: hidden` —
 *    and `.moss-hero-content`, the only text slot the hero had before this, is
 *    absolutely positioned ON the image. A credit printed across a photograph
 *    is unreadable and, for a photographer's name, wrong. The emitted markup
 *    cannot tell you whether the caption ended up over the picture, under it,
 *    or clipped away; a layout can.
 *
 *  - `object-fit: cover` crops to fill. On a portrait photo in a wide frame it
 *    keeps the top and throws the bottom away, which is exactly where the
 *    subject a caption names may be. `object-fit` has no meaning until an
 *    engine lays the image out, so no text assertion can check it.
 *
 * The Rust side pins what moss emits: `hero_caption_renders_below_the_image_not_over_it`
 * in typed_renderers.rs. This pins what a reader sees.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → HERO_CAPTION_GATE. The fixture image
 * is portrait (400×800) against a desktop viewport, the shape where cover and
 * contain disagree most.
 */
import { test, expect } from "@playwright/test";

const CAPTION = ".moss-hero-caption";

test.describe("hero caption", () => {
  test("the caption is visible and sits below the photograph", async ({ page }) => {
    await page.goto("/captioned/");

    const caption = page.locator(CAPTION);
    await expect(caption).toBeVisible();
    await expect(caption).toContainText("photo: A. Photographer");

    const hero = (await page.locator(".moss-hero").boundingBox())!;
    const cap = (await caption.boundingBox())!;
    // Below, not on top of. A caption inside the hero's box would overlap it.
    expect(cap.y).toBeGreaterThanOrEqual(hero.y + hero.height - 1);
    // And not collapsed to nothing by the section's `overflow: hidden`.
    expect(cap.height).toBeGreaterThan(0);
  });

  test("a captioned hero shows the whole image instead of cropping it", async ({ page }) => {
    await page.goto("/captioned/");

    const img = page.locator(".moss-hero img");
    expect(await img.evaluate((el) => getComputedStyle(el).objectFit)).toBe("contain");

    // The real claim behind that declaration: nothing is cut off. A 400×800
    // source painted into a wide frame keeps its 1:2 shape.
    const shape = await img.evaluate((el: HTMLImageElement) => {
      const box = el.getBoundingClientRect();
      return { drawn: box.width / box.height, natural: el.naturalWidth / el.naturalHeight };
    });
    expect(shape.drawn).toBeCloseTo(shape.natural, 1);
  });

  test("a hero without a caption keeps the default crop", async ({ page }) => {
    await page.goto("/plain/");

    await expect(page.locator(CAPTION)).toHaveCount(0);
    // Unchanged for every hero that came before: fill the frame, anchored top.
    const img = page.locator(".moss-hero img");
    expect(await img.evaluate((el) => getComputedStyle(el).objectFit)).toBe("cover");
  });
});
