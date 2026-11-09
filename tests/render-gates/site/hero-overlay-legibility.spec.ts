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
import { decodePng } from "../../e2e/helpers/decode-png";
import { parseColor, worstCaseContrast } from "../../e2e/helpers/wcag-contrast";

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
 * The RGB pixels of a `page.screenshot()` PNG (8-bit RGB or RGBA, any row
 * filter), decoded here rather than in the page, whose canvas may convert
 * colour. Reading the actual painted colour, not `elementFromPoint` (which
 * reports the pseudo-element's HOST node — `body`, here — rather than what is
 * visually on top, so it can't tell "the theme layer is covering the hero"
 * apart from "the hero's own section is topmost").
 */
/** The one pixel of a 1x1 screenshot. */
function onePixel(buf: Buffer) {
  const [r, g, b] = decodePng(buf).at(0, 0);
  return { r, g, b };
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
  const { r, g, b } = onePixel(buf);
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
  const { r, g, b } = onePixel(buf);
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

  test("align=center sets the panel in the middle of the hero, and focus: sets the crop point", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/align-center/");

    await assertPanelHugsContent(page);
    const hero = await rectOf(page, ".moss-hero");
    const content = await rectOf(page, ".moss-hero-content");
    const mid = (a: number, b: number) => (a + b) / 2;
    expect(mid(content.left, content.right), "panel is not centred horizontally").toBeCloseTo(mid(hero.left, hero.right), 0);
    expect(mid(content.top, content.bottom), "panel is not centred vertically").toBeCloseTo(mid(hero.top, hero.bottom), 0);
    expect(
      await page.evaluate(() => getComputedStyle(document.querySelector(".moss-hero img")!).objectPosition),
    ).toBe("20% 62%");
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
        const p = onePixel(await page.screenshot({ clip: { x, y, width: 1, height: 1 } }));
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

/**
 * The panel's edge fades into the picture (`--moss-hero-panel-feather`) on the
 * sides that face it, instead of cutting the picture with a line. Read off the
 * painted pixels: a strip crossing the edge is shot with the panel set to red at
 * 0.8 and again at 0 (the fade copies the panel, so it follows), and the panel's
 * alpha at each device pixel is solved from the pair, `a = (A − B)·(P − B) / |P − B|²`.
 * The busy picture's own lines cancel out.
 */
test.describe("hero overlay panel edge", () => {
  type Strip = { x: number; y: number; width: number; height: number };

  /** The pixels along a 1px strip, from a shot 32px across it (WebKit at 1.5x paints a mask wrongly into a 1px clip), read along its middle. */
  async function pixels(page: Page, strip: Strip): Promise<number[][]> {
    const along = strip.width >= strip.height;
    const clip = along ? { ...strip, y: strip.y - 16, height: 32 } : { ...strip, x: strip.x - 16, width: 32 };
    const png = decodePng(await page.screenshot({ clip }));
    const mid = Math.floor((along ? png.height : png.width) / 2);
    return Array.from({ length: along ? png.width : png.height }, (_, k) => {
      const [r, g, b] = along ? png.at(k, mid) : png.at(mid, k);
      return [r, g, b];
    });
  }

  /**
   * The panel's alpha along a 1px strip, from the box's inside outward; null where the picture is
   * too close to the probe colour to tell. Only `--moss-hero-panel-bg` changes between the two shots,
   * so the picture, its scrim and the layout are the same in both.
   */
  async function alphaAcross(page: Page, strip: Strip, outwardIsReversed: boolean) {
    const DECLARED = 0.8;
    // Every picture decoded and the style change painted, so the two shots differ by the panel alone.
    const settle = () => page.evaluate(async () => {
      await Promise.all([...document.images].map((img) => img.decode().catch(() => {})));
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    });
    const probe = async (alpha: number) => {
      const tag = await page.addStyleTag({ content: `:root { --moss-hero-panel-bg: rgb(255 0 0 / ${alpha}); }` });
      await settle();
      return tag;
    };
    const on = await probe(DECLARED);
    const built = await pixels(page, strip);
    await on.evaluate((el) => el.remove());
    const off = await probe(0);
    const bare = await pixels(page, strip);
    await off.evaluate((el) => el.remove());
    const p = [255, 0, 0];
    const alphas = built.map((a, i) => {
      const b = bare[i];
      const pb = p.map((v, k) => v - b[k]);
      const norm = pb.reduce((s, v) => s + v * v, 0);
      if (norm < 60 * 60) return null;
      return pb.reduce((s, v, k) => s + (a[k] - b[k]) * v, 0) / norm;
    });
    return { declared: DECLARED, alphas: outwardIsReversed ? alphas.reverse() : alphas };
  }

  /** Where the box's edge falls in a strip read from inside outward: `inside` pixels lie wholly in the box, and on a fractional edge the next one is split by it. */
  type Edge = { inside: number; split: boolean };

  function assertSoftEdge(label: string, run: { declared: number; alphas: (number | null)[] }, edge: Edge, dpr: number) {
    const known = run.alphas.map((a, i) => [i, a] as const).filter((e): e is readonly [number, number] => e[1] !== null);
    const shown = `edge ${edge.inside}${edge.split ? " (split)" : ""}: ${known.map(([i, a]) => `${i}:${a.toFixed(2)}`).join(" ")}`;
    // The box's paint and the mask meet at the split pixel, or the one just inside it when layout rounds the edge across it. The CSS accepts one such pixel, and nowhere else.
    const seams = edge.split ? known.filter(([i, a]) => (i === edge.inside || i === edge.inside - 1) && Math.abs(a - run.declared) >= 0.06) : [];
    expect(seams.length, `${label}: more than one seam pixel (${shown})`).toBeLessThanOrEqual(1);
    for (const [, a] of seams) expect(Math.abs(a - run.declared), `${label}: the seam pixel strays too far (${shown})`).toBeLessThanOrEqual(0.45);
    const seam = seams[0]?.[0];
    const inside = known.filter(([i]) => i < edge.inside && i !== seam);
    expect(inside.length, `${label}: no readable pixels inside the box (${shown})`).toBeGreaterThan(2);
    for (const [, a] of inside) expect(Math.abs(a - run.declared), `${label}: the words' panel keeps alpha ${run.declared} (${shown})`).toBeLessThan(0.06);
    // Outward from the box: never brighter-then-darker (a seam), never a step a hard edge would make.
    const outward = known.filter(([i]) => i >= edge.inside - 1 && i !== seam);
    for (let k = 1; k < outward.length; k++) {
      const [[i0, a0], [i1, a1]] = [outward[k - 1], outward[k]];
      expect(a1 - a0, `${label}: alpha rises outward at ${i1} (${shown})`).toBeLessThan(0.06);
      expect((a0 - a1) / (i1 - i0), `${label}: a hard step at ${i1} (${shown})`).toBeLessThan(0.12 / dpr);
    }
    const tail = known.filter(([i]) => i >= run.alphas.length - 4 * dpr);
    for (const [, a] of tail) expect(a, `${label}: the fade has not ended a feather out (${shown})`).toBeLessThan(0.06);
  }

  const FEATHER = 40; // 2.5rem
  const INSET = 12; // inside the box's padding, clear of the words

  /** An even CSS coordinate is a whole device pixel at 1x, 1.5x and 2x, so a strip that starts on one has its indices on the page's device grid. */
  const onGrid = (v: number) => Math.floor(v / 2) * 2;

  /** One strip per side of the words' box that faces the picture (a feather's room from the hero's edge), from inside the box outward. */
  async function stripsFacingPicture(page: Page, dpr: number) {
    const [hero, box] = [await rectOf(page, ".moss-hero"), await rectOf(page, ".moss-hero-content")];
    const room = FEATHER + 6;
    const midX = onGrid(box.left + box.width / 2), midY = onGrid(box.top + box.height / 2);
    const sides: { side: string; strip: Strip; reversed: boolean; edge: Edge }[] = [];
    const add = (side: string, boxEdge: number, from: number, to: number, reversed: boolean, across: boolean) => {
      const start = onGrid(from);
      const length = onGrid(to - start + 1.999);
      const strip = across ? { x: start, y: midY, width: length, height: 1 } : { x: midX, y: start, width: 1, height: length };
      const offset = (boxEdge - start) * dpr;
      const split = Math.abs(offset - Math.round(offset)) > 1 / 32;
      const inside = reversed ? length * dpr - Math.ceil(split ? offset : Math.round(offset)) : Math.floor(split ? offset : Math.round(offset));
      sides.push({ side, strip, reversed, edge: { inside, split } });
    };
    if (box.top - hero.top >= room) add("top", box.top, box.top - room, box.top + INSET, true, false);
    if (hero.bottom - box.bottom >= room) add("bottom", box.bottom, box.bottom - INSET, box.bottom + room, false, false);
    if (box.left - hero.left >= room) add("start", box.left, box.left - room, box.left + INSET, true, true);
    if (hero.right - box.right >= room) add("end", box.right, box.right - INSET, box.right + room, false, true);
    return sides;
  }

  // The last case moves the box's edges off the pixel grid, where a seam between
  // the panel and its fade would show as a pixel of lower alpha.
  const CASES = [
    { path: "/", width: 1280, css: "" },
    { path: "/align-center/", width: 1280, css: "" },
    { path: "/", width: 390, css: "" },
    { path: "/align-center/", width: 1280, css: ".moss-hero-content { padding: calc(5% + 0.37px); }" },
  ];
  for (const dpr of [1, 1.5, 2]) {
    test(`${dpr}x: every side of the panel that faces the picture fades into it, and the words keep the solid panel`, async ({ browser }) => {
      for (const c of CASES) {
        const context = await browser.newContext({ viewport: { width: c.width, height: 900 }, deviceScaleFactor: dpr });
        const page = await context.newPage();
        await page.goto(c.path);
        if (c.css) await page.addStyleTag({ content: c.css });
        const sides = await stripsFacingPicture(page, dpr);
        expect(sides.length, `${c.path} ${c.width}px ${c.css}: no side of the panel faces the picture`).toBeGreaterThan(0);
        for (const { side, strip, reversed, edge } of sides) {
          assertSoftEdge(`${c.path} ${c.width}px ${side} ${c.css}`, await alphaAcross(page, strip, reversed), edge, dpr);
        }
        await context.close();
      }
    });
  }

  test("a theme that clears the panel gets no fade, and --moss-hero-panel-feather: 0 restores the hard edge", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    const fade = () => page.evaluate(() => {
      const cs = getComputedStyle(document.querySelector(".moss-hero-content")!, "::before");
      return { bg: cs.backgroundColor, pad: parseFloat(cs.paddingTop) };
    });
    expect(parseColor((await fade()).bg).a, "the fade copies the panel").toBeGreaterThan(0.5);
    const cleared = await page.addStyleTag({ content: ".moss-hero-content { background: none; }" });
    expect(parseColor((await fade()).bg).a, "a cleared panel leaves nothing to fade").toBe(0);
    await cleared.evaluate((el) => el.remove());
    await page.addStyleTag({ content: ":root { --moss-hero-panel-feather: 0px; }" });
    expect((await fade()).pad, "no fade at 0").toBe(0);
  });
});
