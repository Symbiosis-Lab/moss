/**
 * A pale hero keeps its picture and flips its text, instead of being darkened
 * until white type survives.
 *
 * See tests/e2e/helpers/gate-sites.ts → HERO_TONE_GATE for why these run in a
 * browser rather than as Rust assertions on emitted markup.
 */
import { test, expect, Page } from "@playwright/test";

const PROBE = "/hero-tone.html";

/** WCAG relative luminance of a computed `rgb(...)` / `rgba(...)` string. */
function luminance(css: string): number {
  const nums = css.match(/[\d.]+/g);
  if (!nums || nums.length < 3) throw new Error(`not a colour: ${css}`);
  const [r, g, b] = nums.slice(0, 3).map((n) => {
    const c = Number(n) / 255;
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

const scrimContent = (page: Page, id: string) =>
  page.evaluate(
    (sel) => getComputedStyle(document.querySelector(sel)!, "::before").content,
    `#${id}`,
  );

const colorOf = (page: Page, id: string) =>
  page.evaluate(
    (sel) => getComputedStyle(document.querySelector(sel)!).color,
    `#${id}`,
  );

const shadowOf = (page: Page, id: string) =>
  page.evaluate(
    (sel) => getComputedStyle(document.querySelector(sel)!).textShadow,
    `#${id}`,
  );

test.describe("hero tone", () => {
  test("a pale hero paints no scrim and sets dark text", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(PROBE);

    // `none` is the whole fix: the pastel artwork arrives unaltered.
    expect(await scrimContent(page, "light-hero")).toBe("none");

    // Dark enough that the removed scrim is not missed. The threshold is well
    // clear of both possible outcomes — moss's body text sits near 0.02, the
    // white it replaces at 1.0 — so this cannot pass on a near-miss.
    expect(luminance(await colorOf(page, "light-heading"))).toBeLessThan(0.2);
    expect(luminance(await colorOf(page, "light-para"))).toBeLessThan(0.2);

    // The shadow existed to hold white type off a photograph. Dark type on a
    // pale image does not need it, and it reads as grime at this size.
    expect(await shadowOf(page, "light-heading")).toBe("none");
  });

  test("a hero with no tone attribute keeps the scrim and white text", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(PROBE);

    // The default is unchanged — this gate exists as much to pin that.
    expect(await scrimContent(page, "dark-hero")).not.toBe("none");
    expect(luminance(await colorOf(page, "dark-heading"))).toBeGreaterThan(0.9);
  });

  test("a pale hero overlaid on mobile is still dark text on a clean image", async ({
    page,
  }) => {
    // 390px: inside the 48rem stacking breakpoint, but `mobile=overlay` keeps
    // the text on the picture. Both rules that decide this win on source order
    // against an exactly-equal specificity, which is what makes it worth a
    // gate rather than a reading of the file.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(PROBE);

    expect(await scrimContent(page, "light-overlay-hero")).toBe("none");
    expect(
      luminance(await colorOf(page, "light-overlay-heading")),
    ).toBeLessThan(0.2);
  });

  test("a pale hero stacked on mobile keeps white text on its colour band", async ({
    page,
  }) => {
    // The case the tone flip must NOT reach. Stacked, the text sits below the
    // image on `--moss-cover-color` — the WCAG-darkened extract — so flipping
    // it dark would print near-black on near-black. A blanket
    // `[data-hero-tone="light"] .moss-hero-content { color: dark }` does
    // exactly that, which is why the real rules are scoped to the overlaid
    // layouts.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(PROBE);

    expect(
      luminance(await colorOf(page, "light-stacked-heading")),
    ).toBeGreaterThan(0.9);
  });
});
