/**
 * A built locator's water layer resolves `--moss-place-water` to the
 * approved cut-paper palette, in both themes — not the Rust-side
 * `var(..., #fallback)` value a missing site.css declaration would silently
 * fall through to.
 *
 * The water `<rect>` is the emitter's base layer (`fn water` in svg.rs):
 * always present at full canvas size regardless of what land/relief
 * geometry later paints over it, so this gate does not depend on the
 * coastline's own clipping being correct.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACE_MAP_TOKENS_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/place-map-tokens.config.ts
 */
import { test, expect } from "@playwright/test";

const WATER_RECT = '[data-map-layer="water"] rect';

// #e9eff2 / #1b242c — the approved design's shallow-sea endpoint, light
// and dark. Held as literals because that is the point of the assertion: a
// token-name check would pass even if site.css defined the token wrong.
const LIGHT_WATER = "rgb(233, 239, 242)";
const DARK_WATER = "rgb(27, 36, 44)";

async function setTheme(
  page: import("@playwright/test").Page,
  theme: "light" | "dark",
) {
  await page.goto("./", { waitUntil: "domcontentloaded" });
  await page.evaluate((t) => localStorage.setItem("moss-theme", t), theme);
  await page.reload({ waitUntil: "domcontentloaded" });
}

test("a coastal locator's water layer resolves to the approved sea colour, light and dark", async ({
  page,
}) => {
  await setTheme(page, "light");
  expect(
    await page.evaluate(() =>
      document.documentElement.getAttribute("data-theme"),
    ),
  ).not.toBe("dark");
  await expect(
    page.locator(WATER_RECT).first(),
    `light water must resolve to ${LIGHT_WATER} (--moss-place-water: #e9eff2); ` +
      "a stale or missing site.css declaration would instead show the Rust " +
      "fallback baked into the SVG's own var() call",
  ).toHaveCSS("fill", LIGHT_WATER);

  await setTheme(page, "dark");
  expect(
    await page.evaluate(() =>
      document.documentElement.getAttribute("data-theme"),
    ),
  ).toBe("dark");
  await expect(
    page.locator(WATER_RECT).first(),
    `dark water must resolve to ${DARK_WATER} (--moss-place-water: #1b242c), ` +
      "darker than the light value — sea darker than land in the dark theme " +
      "is the chosen design's own rule",
  ).toHaveCSS("fill", DARK_WATER);
});
