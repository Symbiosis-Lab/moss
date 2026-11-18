/**
 * Render gate for Task 2.1: the @layer cascade contract.
 *
 * Asserts in BOTH chromium and webkit that a user's `.moss/theme/style.css`
 * wins over moss's own rules — both for a token override
 * (--moss-color-bg: #abcdef) and for a plain selector (.main-nav a).
 *
 * Only a real engine can answer this: jsdom does not implement @layer
 * precedence. Anything provable from the emitted text belongs in a Rust test
 * instead — see `user_theme_link_is_loaded_into_the_themes_layer`.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts, built by the
 * playwright config at parse time and served by its webServer.
 *
 * Run via:
 *   npx playwright test -c playwright/customization-cascade.config.ts
 */

import { test, expect } from "@playwright/test";

// Expected values — both browsers must agree.
const EXPECTED_BG = "rgb(171, 205, 239)"; // #abcdef
const EXPECTED_NAV_COLOR = "rgb(255, 0, 0)";

// Both overrides, one render. The token override and the plain-selector
// override are two readings of the same cascade resolution on the same page;
// loading it twice to take them separately proved nothing extra.
//
// A third test used to assert that the theme <link> carries layer="themes".
// That is a string the emitter either writes or does not, so it moved down to
// a Rust unit test — running it here meant booting chromium AND webkit to
// read one attribute off one element.
test("user CSS in @layer themes wins, for tokens and for plain selectors", async ({
  page,
}) => {
  await page.goto("./", { waitUntil: "domcontentloaded" });
  const computed = await page.evaluate(() => ({
    bodyBg: window.getComputedStyle(document.body).backgroundColor,
    navColor: (() => {
      const el = document.querySelector(".main-nav a");
      return el ? window.getComputedStyle(el).color : null;
    })(),
  }));
  expect(computed.bodyBg, "body background must take the user's :root token override").toBe(
    EXPECTED_BG,
  );
  expect(
    computed.navColor,
    ".main-nav a must take the user's non-token rule over any moss base/layout rule",
  ).toBe(EXPECTED_NAV_COLOR);
});

// A theme that changes the accent gets its selection tint with it. The probe
// is painted with the expected mix, so each engine compares its own spelling
// of the colour.
test("selected text is tinted with the theme's accent", async ({ page }) => {
  await page.goto("./", { waitUntil: "domcontentloaded" });
  const { selection, expected } = await page.evaluate(() => {
    const probe = document.createElement("i");
    probe.style.background = "color-mix(in srgb, #183058 15%, transparent)";
    document.body.append(probe);
    const paragraph = document.querySelector("article p") ?? document.body;
    return {
      selection: getComputedStyle(paragraph, "::selection").backgroundColor,
      expected: getComputedStyle(probe).backgroundColor,
    };
  });
  expect(selection).toBe(expected);
});
