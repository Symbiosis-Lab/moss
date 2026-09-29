/**
 * `.moss-card-cover`'s default `aspect-ratio` is a landscape plate (4/3)
 * under horizontal-tb at container widths above 36rem, and a research-backed
 * portrait plate (3/4) below it for the generated LISTING grid — not a
 * phone-only special case grafted onto every width. a9db42542d6 (2026-09-15)
 * took the 3/4 value measured at phone width and promoted it into the
 * unscoped base rule while deleting the `@container (max-width: 36rem)`
 * override that used to hold it, so every grid card and hand-picked
 * `:::grid` card cover turned portrait at every width, not just narrow
 * ones. b47a55ed (2026-09-25) then narrowed the reversal further: a
 * hand-picked `:::grid` cover is the author's own deliberate choice of
 * image, not a uniform box generated from arbitrary notes, so it stays
 * landscape at every width and only the generated listing grid
 * (`.moss-cards[data-layout="grid"]`) goes portrait below 36rem. This
 * records both reversals: the phone research still stands, scoped to the
 * listing it was measured on. Golden-string Rust tests (shell_tests.rs) and
 * the reciprocal check
 * (`test_css_vertical_card_cover_ratio_is_reciprocal_of_horizontal_default`)
 * both stayed green through the original regression: 3/4 and its vertical.css
 * transpose 4/3 are still exact reciprocals, just applied at the wrong widths.
 * Only a real engine, measuring the box an actual page paints, can tell
 * landscape from portrait at a given width — hence this gate rather than
 * another string match.
 *
 * Two contexts share the token and are checked here, at both a desktop
 * (wide) and a narrow-container width:
 *  - a hand-picked `:::grid` card on the home page (`.moss-grid .moss-card-cover`)
 *    — landscape at BOTH widths (b47a55ed)
 *  - a folder's own generated GRID listing (`children_style: grid`,
 *    `.moss-cards[data-layout="grid"] .moss-card-cover`) — landscape at
 *    desktop, portrait in a narrow container
 *
 * A SUMMARY/list-layout card (`children_style: summary`,
 * `.moss-cards[data-layout="list"] .moss-card-cover`) is deliberately not
 * checked here: that selector overrides `aspect-ratio: none` and sizes the
 * cover from a fixed 120px inline-size plus flex stretch, so it never reads
 * `--moss-card-cover-ratio` at any width and this regression cannot reach it
 * — asserting either shape there would not go red under an ablation of
 * either literal below.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → GRID_MOBILE_COLLAPSE_GATE (shared
 * with grid-mobile-collapse.spec.ts, which already exercises this same box at
 * both viewport widths as part of a broader mobile-collapse story; this gate
 * exists so the landscape-vs-portrait claim has one small, dedicated place to
 * fail and be read on its own).
 */
import { test, expect } from "@playwright/test";

const DESKTOP = { width: 1280, height: 900 };
// Well under the 36rem (576px) container-query breakpoint, matching the
// MOBILE viewport grid-mobile-collapse.spec.ts already uses.
const NARROW = { width: 390, height: 844 };

test.describe("card cover ratio", () => {
  test("a hand-picked :::grid card cover is landscape at desktop width", async ({ page }) => {
    await page.setViewportSize(DESKTOP);
    await page.goto("/");
    const cover = page.locator(".moss-grid .moss-card-cover").first();
    const box = (await cover.boundingBox())!;
    expect(box.width, "cover should be wider than it is tall (a landscape plate)").toBeGreaterThan(
      box.height,
    );
    expect(box.width / box.height).toBeCloseTo(4 / 3, 1);
  });

  test("a folder's generated grid-listing card cover is landscape at desktop width", async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await page.goto("/rows/");
    const cover = page.locator('.moss-cards[data-layout="grid"] .moss-card-cover').first();
    const box = (await cover.boundingBox())!;
    expect(box.width, "cover should be wider than it is tall (a landscape plate)").toBeGreaterThan(
      box.height,
    );
    expect(box.width / box.height).toBeCloseTo(4 / 3, 1);
  });

  test("a hand-picked :::grid card cover stays landscape in a narrow container", async ({ page }) => {
    // b47a55ed: unlike the generated listing below, an author-placed
    // `:::grid` cover does not follow the listing's narrow-container
    // portrait override at all — it keeps the shared 4/3 default here too.
    await page.setViewportSize(NARROW);
    await page.goto("/");
    const cover = page.locator(".moss-grid .moss-card-cover").first();
    const box = (await cover.boundingBox())!;
    expect(box.width, "cover should be wider than it is tall (a landscape plate)").toBeGreaterThan(
      box.height,
    );
    expect(box.width / box.height).toBeCloseTo(4 / 3, 1);
  });

  test("a folder's generated grid-listing card cover is portrait in a narrow container", async ({
    page,
  }) => {
    await page.setViewportSize(NARROW);
    await page.goto("/rows/");
    const cover = page.locator('.moss-cards[data-layout="grid"] .moss-card-cover').first();
    const box = (await cover.boundingBox())!;
    expect(box.height, "cover should be taller than it is wide (a portrait plate)").toBeGreaterThan(
      box.width,
    );
    expect(box.width / box.height).toBeCloseTo(3 / 4, 1);
  });
});
