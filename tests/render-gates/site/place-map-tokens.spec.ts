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

// The last <g data-map-band> child of a band layer is always the band
// stretched to position 1 on its ramp (svg.rs's own
// `band_tints_span_the_whole_ramp_within_each_frame` test pins this), which
// `band_tint()` renders as a 100% color-mix — i.e. exactly the ramp's top
// endpoint, with no interpolation arithmetic for this test to duplicate.
const TOP_RELIEF_BAND = '[data-map-layer="relief"] > g[data-map-band]';
const DEEPEST_SEAFLOOR_BAND = '[data-map-layer="seafloor"] > g[data-map-band]';
const PLACE_MAP_FIGURE = ".moss-place-map";

// #e9eff2 / #1b242c — the approved design's shallow-sea endpoint, light
// and dark. Held as literals because that is the point of the assertion: a
// token-name check would pass even if site.css defined the token wrong.
const LIGHT_WATER = "rgb(233, 239, 242)";
const DARK_WATER = "rgb(27, 36, 44)";

// --moss-place-land-high and --moss-place-sea-deep, light and dark, as the
// hex site.css declares. The light pair is IDENTICAL to the Rust-side
// `var(.., #fallback)` fallback baked into the SVG (band_tint() in
// palette.rs) — by design, the fallback was copied from the approved light
// palette — so a fill-only check can't tell "resolved from site.css" apart
// from "site.css defines nothing and the browser used the baked fallback"
// in the light theme. Only reading the custom property's own computed
// value catches a missing light declaration; the dark pair differs from
// the fallback, so the fill check alone already catches a missing dark
// declaration, but this test reads the property there too, for the same
// reason and so a failure says which half broke.
const LIGHT_LAND_HIGH_HEX = "#f6f1e4";
const DARK_LAND_HIGH_HEX = "#565b57";
const LIGHT_SEA_DEEP_HEX = "#bfd0dc";
const DARK_SEA_DEEP_HEX = "#10161c";

/** The `.moss-place-map` figure's own resolved custom-property value — proof the cascade defined it, independent of what it painted. */
async function customPropertyValue(
  page: import("@playwright/test").Page,
  property: string,
): Promise<string | null> {
  return page.evaluate(
    ({ selector, property }) => {
      const el = document.querySelector(selector);
      if (!el) return null;
      return getComputedStyle(el).getPropertyValue(property).trim();
    },
    { selector: PLACE_MAP_FIGURE, property },
  );
}

/**
 * A band group's painted fill, as a normalised `#rrggbb`.
 *
 * `getComputedStyle(...).fill` on a `color-mix()`-derived fill serialises as
 * `color(srgb r g b)` (0–1 components) in both Chromium and WebKit here,
 * even at a 100% mix where the result is mathematically exactly one
 * endpoint colour — never the legacy `rgb(...)` the plain water `<rect>`
 * below gets, so this parses both forms into the same 0–255 hex rather than
 * asserting against one literal string that would only match one of the
 * two shapes an engine might hand back.
 */
async function paintedFillHex(
  locator: import("@playwright/test").Locator,
): Promise<string> {
  const raw = await locator.evaluate((el) => getComputedStyle(el).fill);
  const legacy = raw.match(/^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/);
  if (legacy) {
    const [r, g, b] = legacy.slice(1, 4).map(Number);
    return toHex(r, g, b);
  }
  const wideGamut = raw.match(/^color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)/);
  if (wideGamut) {
    const [r, g, b] = wideGamut.slice(1, 4).map((n) => Math.round(Number(n) * 255));
    return toHex(r, g, b);
  }
  throw new Error(`unrecognised computed fill colour: ${raw}`);
}

function toHex(r: number, g: number, b: number): string {
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

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

test("a coastal locator's relief and sea-floor bands resolve their tint from the theme tokens, not the Rust fallback, light and dark", async ({
  page,
}) => {
  await setTheme(page, "light");
  expect(
    await customPropertyValue(page, "--moss-place-land-high"),
    "the light figure must itself declare --moss-place-land-high; a missing " +
      "declaration would leave the property empty even though the painted " +
      "fill looks right, because the light value and the Rust fallback are " +
      "the same colour",
  ).toBe(LIGHT_LAND_HIGH_HEX);
  expect(
    await customPropertyValue(page, "--moss-place-sea-deep"),
    "the light figure must itself declare --moss-place-sea-deep, for the " +
      "same reason: its value equals the Rust fallback, so only reading the " +
      "property back proves site.css defines it",
  ).toBe(LIGHT_SEA_DEEP_HEX);
  expect(
    await paintedFillHex(page.locator(TOP_RELIEF_BAND).last()),
    `the highest relief band must paint --moss-place-land-high (${LIGHT_LAND_HIGH_HEX})`,
  ).toBe(LIGHT_LAND_HIGH_HEX);
  expect(
    await paintedFillHex(page.locator(DEEPEST_SEAFLOOR_BAND).last()),
    `the deepest sea-floor band must paint --moss-place-sea-deep (${LIGHT_SEA_DEEP_HEX})`,
  ).toBe(LIGHT_SEA_DEEP_HEX);

  await setTheme(page, "dark");
  expect(
    await customPropertyValue(page, "--moss-place-land-high"),
    "the dark figure must itself declare --moss-place-land-high; without " +
      "it the property inherits the light value from the unconditional " +
      "`.moss-place-map` block instead of falling to the Rust fallback",
  ).toBe(DARK_LAND_HIGH_HEX);
  expect(
    await customPropertyValue(page, "--moss-place-sea-deep"),
    "the dark figure must itself declare --moss-place-sea-deep, same reason",
  ).toBe(DARK_SEA_DEEP_HEX);
  expect(
    await paintedFillHex(page.locator(TOP_RELIEF_BAND).last()),
    `the highest relief band must paint --moss-place-land-high (${DARK_LAND_HIGH_HEX}), ` +
      "not the light theme's paler top tint",
  ).toBe(DARK_LAND_HIGH_HEX);
  expect(
    await paintedFillHex(page.locator(DEEPEST_SEAFLOOR_BAND).last()),
    `the deepest sea-floor band must paint --moss-place-sea-deep (${DARK_SEA_DEEP_HEX}), ` +
      "not the light theme's paler deep tint",
  ).toBe(DARK_SEA_DEEP_HEX);
});
