/**
 * Render gate: progressive enhancement. The places namespace root's static
 * map figure is replaced by the interactive layer once the world SVG and
 * the places data file have both loaded; with JavaScript off, or when a
 * fetch fails, the static figure is exactly what stays on screen.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-boot.config.ts
 */
import { test, expect } from "@playwright/test";

const FIGURE = ".moss-place-map[data-moss-places-explorer]";

test("the static figure is replaced by the interactive layer once ready", async ({ page }) => {
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  const figure = page.locator(FIGURE);
  await expect(figure).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  await expect(figure.locator("> svg")).toHaveCount(0);
  await expect(figure.locator(".moss-places-viewport")).toHaveCount(1);
  await expect(figure.locator(".moss-places-status")).toHaveCount(1);
  const bars = await figure.locator(".moss-places-cards").evaluate((row) => {
    const reference = document.createElement("div");
    document.body.append(reference);
    const native = getComputedStyle(reference).scrollbarWidth;
    reference.remove();
    return { actual: getComputedStyle(row).scrollbarWidth, native };
  });
  expect(bars.actual, "the timeline should retain the browser's native scrollbar").toBe(bars.native);
});

// Tile rasters are cut from one continuous map: a rounded corner on any of
// them (the article's own `article img` radius) leaves a gap where four meet.
test("the map's raster images keep square corners", async ({ page }) => {
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  await expect(page.locator(FIGURE)).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  const images = page.locator(".moss-places-viewport img");
  expect(await images.count()).toBeGreaterThan(0);
  const radii = await images.evaluateAll((els) => els.map((el) => getComputedStyle(el).borderRadius));
  expect(radii.every((radius) => radius === "0px"), `border radii: ${radii.join(", ")}`).toBe(true);
});

// The fixture's own "Faro Notes" (gate-sites.ts's `placesExplorerSparseWork`)
// carries none of byline/date/description/cover — `places_data.rs` omits all
// four from the wire for a work with none of them, which once crashed the
// explorer's very first render (`work.byline.length` on an `undefined`
// byline) before any card painted, leaving `data-moss-places-explorer-ready`
// stuck at "pending" for the WHOLE site, not just this one work's card.
test("a work missing byline, cover, date and description still reaches ready and renders its card", async ({ page }) => {
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  const figure = page.locator(FIGURE);
  await expect(figure).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  await expect(figure.locator(".moss-card-title", { hasText: "Faro Notes" })).toHaveCount(1);
});

test("without JavaScript, the static figure remains and nothing swaps in", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(`${baseURL}places/`, { waitUntil: "domcontentloaded" });
  const figure = page.locator(FIGURE);
  await expect(figure.locator("> svg")).toHaveCount(1);
  await expect(figure.locator(".moss-places-viewport")).toHaveCount(0);
  await expect(figure).not.toHaveAttribute("data-moss-places-explorer-ready", "ready");
  await context.close();
});

test("a failed fetch leaves the static figure untouched and throws nothing", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const warned = page.waitForEvent("console", (msg) => msg.text().includes("places explorer could not load"));
  await page.route("**/world.svg", (route) => route.abort());
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  await warned;

  const figure = page.locator(FIGURE);
  await expect(figure).toHaveAttribute("data-moss-places-explorer-ready", "pending");
  await expect(figure.locator("> svg")).toHaveCount(1);
  await expect(figure.locator(".moss-places-viewport")).toHaveCount(0);
  expect(errors, `unexpected page errors: ${errors.join("; ")}`).toEqual([]);
});

// Design decision 7: "the map is the page" — on the places root the figure
// breaks out of the article's text column and fills the viewport below the
// header, at every width, with no page heading of its own and no gap or
// double line where the header's own bottom edge meets the map's top.
for (const width of [1440, 390]) {
  test(`the map fills the viewport width and reaches the viewport bottom at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    const figure = page.locator(FIGURE);
    await expect(figure).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });

    const figureBox = (await figure.boundingBox())!;
    const viewportSize = page.viewportSize()!;
    // A few px of engine-specific scrollbar-gutter slack, not a hairline
    // check: `100cqi` resolves against `main`'s own content box, which a
    // classic (non-overlay) scrollbar narrows by an amount that measures
    // differently across engines — the same `clientWidth`-vs-`100vw` gap
    // site.css's own `--moss-sidenote-inset` comment documents (a few px in
    // WebKit there too). The map still reads as full-bleed well within that.
    const SLACK = 8;
    expect(figureBox.x).toBeLessThanOrEqual(SLACK);
    expect(figureBox.x + figureBox.width).toBeGreaterThanOrEqual(viewportSize.width - SLACK);
    expect(figureBox.y + figureBox.height).toBeGreaterThanOrEqual(viewportSize.height - SLACK);

    // The header's own bottom rule disappears under the map (site.css's
    // `.nav-content` border, removed by `header:has(+ main article.container
    // > figure[data-moss-places-explorer]) .nav-content` in
    // places-explorer.css) — computed style, not just source text, so a
    // specificity fight elsewhere would be caught here too.
    const navBorder = await page.locator(".nav-content").first().evaluate((el) => getComputedStyle(el).borderBottomStyle);
    expect(navBorder).toBe("none");

    // No VISIBLE page heading of moss's own on an explorer root, but
    // screen-reader navigation still gets exactly one `<h1>`, carrying the
    // page's own title, clipped to a 1px box by `.visually-hidden`
    // (`render/html.rs`'s `is_explorer_root`, `place_map::context`'s own
    // doc for the condition).
    const h1 = page.locator("main h1");
    await expect(h1).toHaveCount(1);
    await expect(h1).toHaveClass(/visually-hidden/);
    await expect(h1).toHaveText("Places");
    const h1Box = await h1.evaluate((el) => el.getBoundingClientRect());
    expect(h1Box.width).toBeLessThanOrEqual(1);
    expect(h1Box.height).toBeLessThanOrEqual(1);

    // The header's bottom edge and the map's top edge coincide: no gap
    // (a stray margin/padding) and no double line (the border check above
    // already rules out a second rule drawn at a different y).
    const headerBottom = await page.locator("header").first().evaluate((el) => el.getBoundingClientRect().bottom);
    expect(Math.abs(figureBox.y - headerBottom)).toBeLessThanOrEqual(1);

    // The fixture's own places/index.md carries a real intro paragraph —
    // it renders BELOW the map now, not above (the map leads).
    const intro = page.getByText("Every work this site locates, gathered on one map.");
    const introBox = (await intro.boundingBox())!;
    expect(introBox.y).toBeGreaterThanOrEqual(figureBox.y + figureBox.height - 1);
  });
}

// Design decision 7 also means no children/term listing below the root's
// map: the reader finds places through the map and its own breadcrumb
// menu, never a list of place links under it.
test("the root has no children/term listing below the map", async ({ page }) => {
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  const figure = page.locator(FIGURE);
  await expect(figure).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  const main = page.locator("main");
  await expect(main.locator(".moss-cards-container")).toHaveCount(0);
  await expect(main.locator(".moss-place-children")).toHaveCount(0);
});

// A nested place's own page (not the namespace root) is an ordinary term
// page, unchanged by design decision 7: it keeps its heading and its
// article list below its own map, same as before.
test("a nested place page still has its own h1, above its map, and still lists its articles", async ({ page }) => {
  await page.goto("places/kyoto/", { waitUntil: "domcontentloaded" });
  const h1 = page.locator("main h1").first();
  await expect(h1).toHaveCount(1);
  const h1Box = (await h1.boundingBox())!;
  const map = page.locator(".moss-place-map").first();
  const mapBox = (await map.boundingBox())!;
  expect(h1Box.y).toBeLessThan(mapBox.y);
  await expect(page.locator("main .moss-cards-container")).not.toHaveCount(0);
});

// The fixture ships the side-note stylesheet (a footnote on /notes/), so body
// carries `padding-right: var(--moss-sidenote-inset)` from 76rem until the
// centred column has room. The full-width map must still run from the
// window's left edge to its right edge at every width, the insetted band
// (about 1216-1316px) included, with no horizontal scroll.
for (const width of [1024, 1180, 1216, 1241, 1280, 1300, 1316, 1366, 1440, 1920]) {
  test(`the full-width map spans the window edge to edge at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    await expect(page.locator(FIGURE)).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
    const m = await page.evaluate(() => {
      const r = document.querySelector(".moss-places-viewport")!.getBoundingClientRect();
      const root = document.documentElement;
      return { left: r.left, right: r.right, clientWidth: root.clientWidth, scrollWidth: root.scrollWidth };
    });
    expect(m.left, "left edge").toBeCloseTo(0, 0);
    expect(Math.abs(m.right - m.clientWidth), `right edge ${m.right} vs ${m.clientWidth}`).toBeLessThanOrEqual(0.5);
    expect(m.scrollWidth, "no horizontal scroll").toBe(m.clientWidth);
  });
}

// The inset is a physical padding-right, so in a right-to-left page the band
// must still end at the window's right edge, and start at the left one: the
// insetted widths, with and without the footnote gutter's reserve.
for (const [page_, selector] of [["places/", FIGURE], ["hero/", '[data-width="screen"]']] as const) {
  for (const width of [1241, 1280]) {
    test(`${page_} spans the window edge to edge at ${width}px under dir="rtl"`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(page_, { waitUntil: "domcontentloaded" });
      await page.evaluate(() => { document.documentElement.dir = "rtl"; });
      const m = await page.evaluate((sel) => {
        const r = document.querySelector(sel)!.getBoundingClientRect();
        const root = document.documentElement;
        return { left: r.left, right: r.right, clientWidth: root.clientWidth, scrollWidth: root.scrollWidth };
      }, selector);
      expect(Math.abs(m.left), `left edge ${m.left}`).toBeLessThanOrEqual(0.5);
      expect(Math.abs(m.right - m.clientWidth), `right edge ${m.right} vs ${m.clientWidth}`).toBeLessThanOrEqual(0.5);
      expect(m.scrollWidth, "no horizontal scroll").toBe(m.clientWidth);
    });
  }
}
