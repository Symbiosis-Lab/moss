/**
 * Two defects in one real built page (see
 * tests/e2e/helpers/gate-sites.ts → HERO_OVERLAY_LEGIBILITY_GATE):
 *
 * 1. Overlay text drawn straight onto a busy photo is unreadable wherever the
 *    picture's own lines fall under it. The fix gives it a backing panel,
 *    tinted from the image's own scanned colour, whose contrast against the
 *    text must hold even in the worst case of its own translucency.
 * 2. The hero box used to cap its height and clip overflow, so overlay text
 *    taller than that cap vanished with no warning. The fix lets the box grow
 *    to fit its words instead.
 *
 * Both need a real engine: contrast is read off `getComputedStyle` after the
 * cascade (media queries, the `var(--moss-hero-panel-bg, …)` fallback chain)
 * has resolved, and "is this element clipped" is a layout question jsdom
 * cannot answer.
 */
import { test, expect, Page } from "@playwright/test";
import { inflateSync } from "node:zlib";
import { contrastRatio, parseColor, relativeLuminance } from "../../e2e/helpers/wcag-contrast";

/**
 * The panel is translucent, so its EFFECTIVE colour depends on what is
 * behind it — composite it over pure black and pure white ourselves (the
 * two extremes that bound every actual pixel of the photo underneath;
 * `panel_lightness_for_contrast`'s doc comment in color_extract.rs spells
 * out why those two are enough) rather than trust a screenshot of this one
 * fixture's accidental backdrop.
 */
function worstCaseContrast(panel: string, text: string): { overBlack: number; overWhite: number } {
  const p = parseColor(panel);
  const textLum = relativeLuminance(...(Object.values(parseColor(text)).slice(0, 3) as [number, number, number]));
  const composite = (backdrop: number) => p.a * p.r + (1 - p.a) * backdrop;
  const over = (backdrop: number) =>
    contrastRatio(
      relativeLuminance(composite(backdrop), p.a * p.g + (1 - p.a) * backdrop, p.a * p.b + (1 - p.a) * backdrop),
      textLum,
    );
  return { overBlack: over(0), overWhite: over(255) };
}

async function styleOf(page: Page, selector: string, prop: "color" | "backgroundColor" | "maxWidth") {
  return page.evaluate(
    ([sel, p]) => getComputedStyle(document.querySelector(sel)!)[p as "color"],
    [selector, prop] as const,
  );
}

async function rectOf(page: Page, selector: string) {
  return page.evaluate(
    (sel) => document.querySelector(sel)!.getBoundingClientRect(),
    selector,
  );
}

async function assertNotClipped(page: Page) {
  const hero = await rectOf(page, ".moss-hero");
  for (const sel of ["#overlay-on-a-busy-backdrop", ".moss-hero-content p", ".moss-hero-content a"]) {
    const r = await rectOf(page, sel);
    expect(r.top, `${sel} top is above the hero's own top (clipped)`).toBeGreaterThanOrEqual(hero.top - 1);
    expect(r.bottom, `${sel} bottom falls below the hero's own bottom (clipped)`).toBeLessThanOrEqual(hero.bottom + 1);
  }
}

async function assertPanelContrast(page: Page) {
  const panel = await styleOf(page, ".moss-hero-content", "backgroundColor");
  const text = await styleOf(page, ".moss-hero-content", "color");
  const { overBlack, overWhite } = worstCaseContrast(panel, text);
  expect(overBlack, `panel ${panel} vs text ${text}, composited over black`).toBeGreaterThanOrEqual(4.5);
  expect(overWhite, `panel ${panel} vs text ${text}, composited over white`).toBeGreaterThanOrEqual(4.5);
}

/**
 * A single pixel's RGB from a 1x1 `page.screenshot({ clip })`. Reading the
 * actual painted colour, not `elementFromPoint` (which reports the pseudo-
 * element's HOST node — `body`, here — rather than reflecting what's
 * visually on top, so it can't tell "the theme layer is covering the hero"
 * apart from "the hero's own section is topmost"). A 1x1 image has no
 * left/up neighbour, so every PNG filter type reduces to "none": the bytes
 * right after the per-row filter byte ARE the pixel.
 */
function decodeSinglePixelPng(buf: Buffer): { r: number; g: number; b: number } {
  let i = 8; // past the 8-byte PNG signature
  let idat = Buffer.alloc(0);
  while (i < buf.length) {
    const len = buf.readUInt32BE(i);
    const type = buf.toString("ascii", i + 4, i + 8);
    if (type === "IDAT") idat = Buffer.concat([idat, buf.subarray(i + 8, i + 8 + len)]);
    i += 12 + len; // length + type + data + crc
  }
  const raw = inflateSync(idat);
  return { r: raw[1], g: raw[2], b: raw[3] }; // raw[0] is the filter byte
}

/**
 * The fixture's theme (gate-sites.ts) paints a full-bleed, opaque,
 * body-level layer — the shape a real theme's background treatment takes.
 * The hero's grow-to-fit media sits at `z-index: -1` so it stays behind the
 * hero's own overlay text; without `isolation: isolate` on `.moss-hero`
 * containing that, -1 is relative to the WHOLE page rather than just the
 * hero, and the theme's opaque layer paints over the hero's image entirely.
 * Probed at the hero's top-right — outside the bottom-left text panel, over
 * plain busy-image pixels (never white) when the image is on top.
 */
async function assertHeroPaintsAboveThemeLayers(page: Page) {
  const hero = await rectOf(page, ".moss-hero");
  const x = Math.round(hero.x + hero.width - 10);
  const y = Math.round(hero.y + 10);
  const buf = await page.screenshot({ clip: { x, y, width: 1, height: 1 } });
  const { r, g, b } = decodeSinglePixelPng(buf);
  expect(
    [r, g, b],
    `pixel at the hero's top-right (${x}, ${y}) is rgb(${r}, ${g}, ${b}) — the fixture theme's opaque white layer, not the busy image, so it is painting over the hero`,
  ).not.toEqual([255, 255, 255]);
}

/**
 * The scrim (`.moss-hero::before`, `position: absolute`) must stay BEHIND
 * the overlay panel, not paint over it and the text sitting on it.
 * `getComputedStyle` can't see this — it reports the panel's own declared
 * background, not what a sibling paints on top of it afterward — so this
 * forces both layers to an unmistakable opaque color and samples the actual
 * rendered pixel, the same technique `assertHeroPaintsAboveThemeLayers`
 * above uses for the theme-layer case. Run last: it permanently overrides
 * both colors for the rest of this page's assertions.
 */
async function assertPanelPaintsAboveScrim(page: Page) {
  await page.addStyleTag({
    content: ".moss-hero::before { background: red !important; } .moss-hero-content { background: blue !important; }",
  });
  const content = await rectOf(page, ".moss-hero-content");
  const x = Math.round(content.x + content.width / 2);
  const y = Math.round(content.y + Math.min(10, content.height / 2));
  const buf = await page.screenshot({ clip: { x, y, width: 1, height: 1 } });
  const { r, g, b } = decodeSinglePixelPng(buf);
  expect(
    [r, g, b],
    `pixel inside the overlay panel is rgb(${r}, ${g}, ${b}) — the scrim (forced red) is painting over the panel (forced blue) instead of behind it`,
  ).toEqual([0, 0, 255]);
}

/**
 * The panel must hug its own content — anchored to the hero's bottom, its
 * height just its content plus padding — not stretch to the hero's full
 * height, which would wash out whatever the picture shows above a short
 * line or two of text. `.moss-hero-content`'s `flex: none` (site.css) is
 * what stops that stretch; see that rule's comment for why it's explicit
 * rather than left to flexbox's own (coincidentally matching) default.
 */
async function assertPanelHugsContent(page: Page) {
  const hero = await rectOf(page, ".moss-hero");
  const content = await rectOf(page, ".moss-hero-content");
  expect(
    content.height,
    `panel height ${content.height} is >= half the hero's own height ${hero.height} — it is stretching to fill the frame instead of hugging its content`,
  ).toBeLessThan(hero.height * 0.5);
  expect(
    content.top,
    `panel top ${content.top} is at or above the hero's own top ${hero.top} — expected it well below, anchored to the bottom by its own short content`,
  ).toBeGreaterThan(hero.top + hero.height * 0.3);
}

/**
 * `align=end` (site.css's `[data-align="end"]`) moves the panel to the
 * hero's INLINE-END edge instead of the default start, for a subject the
 * crop can't move out from under the words (a portrait image already
 * spanning the full width of a wide frame cannot move sideways).
 */
async function assertPanelAtInlineEnd(page: Page) {
  const hero = await rectOf(page, ".moss-hero");
  const content = await rectOf(page, ".moss-hero-content");
  expect(
    content.right,
    `panel right edge ${content.right} is not at the hero's inline-end ${hero.right}`,
  ).toBeCloseTo(hero.right, 0);
  expect(
    content.left,
    `panel left edge ${content.left} is not right of the hero's horizontal centre ${hero.left + hero.width / 2}`,
  ).toBeGreaterThan(hero.left + hero.width / 2);
}

/**
 * Under `mobile=overlay` below the breakpoint, `.moss-hero`'s
 * `padding-block-start: var(--moss-hero-mobile-band, …)` (site.css) reserves
 * a band of the image above the panel, read here off the hero's own computed
 * padding rather than recomputed from the token's `min(45vb, 320px)`
 * fallback — the hero's padding box is what actually bounds the panel, so
 * this holds regardless of what a theme sets the token to.
 */
async function assertMobileBandVisible(page: Page) {
  const hero = await rectOf(page, ".moss-hero");
  const content = await rectOf(page, ".moss-hero-content");
  const bandHeight = await page.evaluate(
    () => parseFloat(getComputedStyle(document.querySelector(".moss-hero")!).paddingTop),
  );
  expect(bandHeight, "the mobile band token resolved to 0 — nothing is reserved").toBeGreaterThan(0);
  expect(
    content.top - hero.top,
    `panel top is only ${content.top - hero.top}px below the hero's own top, less than the ${bandHeight}px band — the panel is covering the reserved image strip`,
  ).toBeGreaterThanOrEqual(bandHeight - 1);
}

test.describe("hero overlay legibility", () => {
  for (const theme of ["light", "dark"] as const) {
    for (const viewport of [
      { width: 1280, height: 900 },
      { width: 390, height: 844 },
    ]) {
      test(`${viewport.width}px / ${theme}: text is not clipped and clears panel contrast`, async ({
        page,
      }) => {
        await page.setViewportSize(viewport);
        await page.goto("/");
        if (theme === "dark") {
          await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
        }

        await assertNotClipped(page);
        await assertPanelContrast(page);
        await assertHeroPaintsAboveThemeLayers(page);
        // The fixture's own hero carries mobile=overlay, so only the
        // narrow viewport exercises the reserved image band.
        if (viewport.width === 390) await assertMobileBandVisible(page);
        // Last: overrides both layers' colors for the rest of this page.
        await assertPanelPaintsAboveScrim(page);
      });
    }
  }
});

test.describe("hero overlay panel sizing and placement", () => {
  /**
   * `.moss-hero[data-mobile="overlay"]`'s rules used to be gated
   * `@media (max-width: 48rem)` against the desktop grow-to-fit rules'
   * `@media (min-width: 48rem)` — both inclusive, so at exactly 768px a
   * mobile=overlay hero matched both and the mobile rule's own
   * `max-inline-size: none` on `.moss-hero-content` (more specific than the
   * unconditional default) won outright, stretching the panel full width
   * even though the width sits on the desktop side of the documented
   * boundary. Site.css now uses `width >= 48rem` / `width < 48rem`, which
   * cannot both match the same width.
   */
  test("at exactly 768px a mobile=overlay hero still gets the desktop panel width, not the full-width mobile one", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 768, height: 900 });
    await page.goto("/");

    // This fixture page is taller than the 900px viewport, so both engines
    // show a vertical scrollbar — but only WebKit reserves its track out of
    // `document.documentElement.clientWidth`, which is what the `width`
    // media feature resolves against. Measured directly: Chromium's
    // clientWidth stays 768 (overlay scrollbar, no space taken); WebKit's
    // drops to 762, one engine's `(width >= 48rem)` and the other's
    // `(width < 48rem)` winning the identical boundary for a reason that has
    // nothing to do with the cascade under test. Hiding the scrollbar makes
    // clientWidth equal the viewport width Playwright actually asked for, in
    // both engines, so this assertion is about the breakpoint, not about
    // which engine charges a scrollbar against the viewport.
    await page.addStyleTag({ content: "html { overflow-y: hidden !important; }" });

    // Polled rather than a single read: a hero that is still settling its
    // just-navigated stylesheet on a cold browser process can report a
    // transient value for one tick. A real regression here is permanent
    // (it's the cascade, not a timing issue), so it still fails once the
    // poll's own timeout runs out.
    await expect
      .poll(async () => styleOf(page, ".moss-hero-content", "maxWidth"), {
        timeout: 2000,
        message: `.moss-hero-content's computed max-width is "none" at 768px — the mobile=overlay rule (gated max-width: 48rem) is still winning at the desktop boundary`,
      })
      .not.toBe("none");
  });

  test("a short overlay's panel hugs its content and sits at align=end's inline-end edge", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/align-end/");

    await assertPanelHugsContent(page);
    await assertPanelAtInlineEnd(page);
  });

  /**
   * `align=end` moves the panel away from the start side specifically so
   * the subject there stays unobscured — a scrim still darkening the start
   * side under `align=end` washes exactly the thing the attribute exists
   * to keep clear. Sampled in the hero's start quarter (outside the panel
   * either way) and compared against `/plain/` — same image, same crop,
   * no overlay text so no scrim at all — the ground truth for "undarkened".
   */
  test("align=end moves the scrim's dark side with it, leaving the start quarter unscrimmed", async ({
    page,
    browserName,
  }) => {
    // Chromium only: this samples real pixels against the busy fixture's
    // fine 8px grid, which aliases badly under WebKit's headless image
    // compositing even averaged over ten points -- noisy enough there to
    // swing on channels a black scrim cannot plausibly touch (blue, on a
    // black-grid-on-yellow image). The CSS itself is not engine-specific
    // (plain gradients, no vendor prefixes); a getComputedStyle check
    // during development confirmed WebKit resolves the same
    // `to left` direction under [data-align="end"] that Chromium does.
    test.skip(browserName === "webkit", "pixel sampling vs. the fixture's fine grid is too noisy under WebKit's compositing");
    await page.setViewportSize({ width: 1280, height: 900 });
    const hero0 = await (async () => {
      await page.goto("/align-end/");
      return rectOf(page, ".moss-hero");
    })();
    const y = Math.round(hero0.y + 10);
    // Averaged over several x offsets spanning the start quarter, each its
    // own 1x1 sample (correct for any PNG filter type — with no left/up
    // neighbour a single pixel's filtered byte always equals its raw
    // value) — the busy fixture's 8px grid aliases badly against a single
    // sample point, especially across the two pages' slightly different
    // crops, so one point alone is too noisy to compare reliably.
    const offsets = [0.04, 0.06, 0.08, 0.1, 0.12, 0.14, 0.16, 0.18, 0.2, 0.22];
    const average = async (hero: { x: number; width: number }) => {
      let r = 0, g = 0, b = 0;
      for (const frac of offsets) {
        const x = Math.round(hero.x + hero.width * frac);
        const p = decodeSinglePixelPng(await page.screenshot({ clip: { x, y, width: 1, height: 1 } }));
        r += p.r; g += p.g; b += p.b;
      }
      return { r: r / offsets.length, g: g / offsets.length, b: b / offsets.length };
    };
    const scrimmed = await average(hero0);

    await page.goto("/plain/");
    const heroPlain = await rectOf(page, ".moss-hero");
    const plain = await average(heroPlain);

    const delta =
      Math.abs(scrimmed.r - plain.r) + Math.abs(scrimmed.g - plain.g) + Math.abs(scrimmed.b - plain.b);
    expect(
      delta,
      `start-quarter pixel under align=end is rgb(${scrimmed.r},${scrimmed.g},${scrimmed.b}), the undarkened image there is rgb(${plain.r},${plain.g},${plain.b}) -- the scrim is still darkening the start side`,
    ).toBeLessThan(10);
  });
});
