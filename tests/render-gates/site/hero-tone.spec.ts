/**
 * A pale hero keeps its picture and flips its text, instead of being darkened
 * until white type survives.
 *
 * See tests/e2e/helpers/gate-sites.ts → HERO_TONE_GATE for why these run in a
 * browser rather than as Rust assertions on emitted markup.
 */
import { test, expect, Page } from "@playwright/test";
import { luminanceOfCss as luminance } from "../../e2e/helpers/wcag-contrast";

const PROBE = "/hero-tone.html";

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

  test("a pale hero keeps dark text when the reader is in dark mode", async ({
    page,
  }) => {
    // The image does not repaint when the reader flips to dark mode, so the
    // text colour that sits on it must not either. `var(--moss-color-text)`
    // does: it is `#2c2825` in light mode and `#d4cbba` in dark (moss-core's
    // token table), so the dark-mode value is itself pale — printing pale
    // type on the same pale photograph the light-mode fix was written for.
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(PROBE);
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));

    expect(luminance(await colorOf(page, "light-heading"))).toBeLessThan(0.2);
    expect(luminance(await colorOf(page, "light-para"))).toBeLessThan(0.2);
  });

  test("a pale hero overlaid on mobile keeps dark text in dark mode", async ({
    page,
  }) => {
    // Same failure, the other overlaid layout: mobile `data-mobile="overlay"`
    // shares the desktop rule's dependence on the theme text token.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(PROBE);
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));

    expect(
      luminance(await colorOf(page, "light-overlay-heading")),
    ).toBeLessThan(0.2);
  });

  test("a pale hero stacked on mobile keeps white text on its colour band in dark mode too", async ({
    page,
  }) => {
    // The control for the dark-mode case: stacked text sits on
    // --moss-cover-color, not on the theme text token, so it must stay
    // unaffected by both the tone flip and the colour scheme.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(PROBE);
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));

    expect(
      luminance(await colorOf(page, "light-stacked-heading")),
    ).toBeGreaterThan(0.9);
  });
});
