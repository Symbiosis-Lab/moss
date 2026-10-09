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
 * The same locator's lighting is checked for each theme's own strengths,
 * and its place dot is measured on screen: the locator floats
 * at about 350 CSS px, half the width it is drawn for, and only a browser
 * can say how large its dot comes out.
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

// #dbe7ea / #1b242c — the approved design's shallow-sea endpoint, light
// and dark. Held as literals because that is the point of the assertion: a
// token-name check would pass even if site.css defined the token wrong.
const LIGHT_WATER = "rgb(219, 231, 234)";
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
const LIGHT_LAND_HIGH_HEX = "#f7f2e6";
const DARK_LAND_HIGH_HEX = "#565b57";
const LIGHT_SEA_DEEP_HEX = "#b9cdd6";
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
    `light water must resolve to ${LIGHT_WATER} (--moss-place-water: #dbe7ea); ` +
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

// The lighting filter carries one set of gains; each theme scales its
// highlight and shadow through flood-opacity tokens to its own. Only a
// browser resolves those tokens inside a filter.
test("the terrain lighting takes each theme's own highlight and shadow strength", async ({ page }) => {
  const strengths = () =>
    page.evaluate(() =>
      [...document.querySelectorAll(".moss-place-locator feFlood")].map((flood) => getComputedStyle(flood).floodOpacity),
    );
  await setTheme(page, "light");
  expect(await strengths()).toEqual(["1", "0.8889"]);
  await setTheme(page, "dark");
  expect(await strengths()).toEqual(["0.8182", "1"]);
});

// The approved dot is 8 px across (a 4 px radius) on a map shown 720 px
// wide. Sized in viewBox units it shrank with the floated locator, to
// about 4 px on a desktop and on a phone alike.
test.describe("static locator without JavaScript", () => {
  test.use({ javaScriptEnabled: false });
  for (const [device, viewport] of [
    ["desktop", { width: 1440, height: 900 }],
    ["phone", { width: 390, height: 844 }],
  ] as const) {
    test(`the locator's place dot stays 8 px across on a ${device}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await page.goto("./", { waitUntil: "domcontentloaded" });
      await page.locator(".moss-place-locator .moss-place-map > svg").scrollIntoViewIfNeeded();
      const dot = page.locator(".moss-place-locator [data-map-marker]:not([data-map-globe-marker])").first();
      const { x, y, color, mapWidth } = await dot.evaluate((element) => {
        const marker = element as SVGGraphicsElement;
        const svg = marker.ownerSVGElement!;
        const box = marker.getBBox();
        const point = svg.createSVGPoint();
        point.x = box.x + box.width / 2;
        point.y = box.y + box.height / 2;
        const screen = point.matrixTransform(svg.getScreenCTM()!);
        const style = getComputedStyle(marker);
        const color = style.stroke && style.stroke !== "none" ? style.stroke : style.fill;
        return { x: screen.x, y: screen.y, color, mapWidth: svg.getBoundingClientRect().width };
      });
      expect(mapWidth, "the locator is shown well under the 720 px it is drawn for").toBeLessThan(400);
      const shot = await page.screenshot({ clip: { x: x - 12, y: y - 12, width: 24, height: 24 } });
      const matching = await page.evaluate(
        async ([png, target]) => {
          const image = new Image();
          image.src = `data:image/png;base64,${png}`;
          await image.decode();
          const canvas = document.createElement("canvas");
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext("2d")!;
          context.drawImage(image, 0, 0);
          const [r, g, b] = target.match(/\d+/g)!.map(Number);
          const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
          let count = 0;
          for (let index = 0; index < data.length; index += 4) {
            if (Math.abs(data[index] - r) + Math.abs(data[index + 1] - g) + Math.abs(data[index + 2] - b) < 48) count += 1;
          }
          return count / (image.width / 24) ** 2;
        },
        [shot.toString("base64"), color] as const,
      );
      const diameter = 2 * Math.sqrt(matching / Math.PI);
      expect(diameter, `the dot is ${diameter.toFixed(1)} px across`).toBeGreaterThan(7);
      expect(diameter, `the dot is ${diameter.toFixed(1)} px across`).toBeLessThan(11);
    });
  }

});

// Below the 48rem mobile breakpoint the locator un-floats and is documented
// to run the column's own full width (authoring.md), the same rule every
// other floated embed follows (float-mobile-collapse's own gate). The
// figure's own UA-stylesheet margin (`figure { margin: 1em 40px }`) used to
// survive site.css's `margin-block` reset, leaving 80px of blank inline
// margin the outer `.moss-place-locator` had already made room for — this
// compares the map figure against a same-column paragraph rather than a
// hardcoded px width, since the column width itself depends on viewport and
// theme.
test("the locator's map runs the full column width below the mobile breakpoint", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("./", { waitUntil: "domcontentloaded" });
  const widths = await page.evaluate(() => {
    const figure = document.querySelector(".moss-place-map")!;
    const column = document.querySelector("article p")!;
    return { figure: figure.getBoundingClientRect().width, column: column.getBoundingClientRect().width };
  });
  expect(widths.figure / widths.column, `map ${widths.figure}px vs. column ${widths.column}px`).toBeCloseTo(1, 1);
});

// Taveuni sits on the antimeridian, the world map's right edge. A dot is a
// non-scaling stroke in screen px, so on a phone's narrow column its casing
// reaches furthest past a clamp measured in viewBox units.
test("an antimeridian place's dot is drawn whole on the phone-width world map", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  const { map, dot } = await page.evaluate(() => {
    const svg = document.querySelector(".moss-place-map svg") as SVGSVGElement;
    const marker = svg.querySelector('[data-map-marker="places/taveuni"]') as SVGGraphicsElement;
    const casing = marker.previousElementSibling as SVGGraphicsElement;
    const box = casing.getBBox();
    const point = svg.createSVGPoint();
    point.x = box.x + box.width / 2;
    point.y = box.y + box.height / 2;
    const centre = point.matrixTransform(svg.getScreenCTM()!);
    const radius = Number(casing.getAttribute("stroke-width")) / 2;
    const frame = svg.getBoundingClientRect();
    return {
      map: [frame.left, frame.top, frame.right, frame.bottom],
      dot: [centre.x - radius, centre.y - radius, centre.x + radius, centre.y + radius],
    };
  });
  expect(dot[0], `dot ${dot} inside map ${map}`).toBeGreaterThanOrEqual(map[0]);
  expect(dot[1], `dot ${dot} inside map ${map}`).toBeGreaterThanOrEqual(map[1]);
  expect(dot[2], `dot ${dot} inside map ${map}`).toBeLessThanOrEqual(map[2]);
  expect(dot[3], `dot ${dot} inside map ${map}`).toBeLessThanOrEqual(map[3]);
});
