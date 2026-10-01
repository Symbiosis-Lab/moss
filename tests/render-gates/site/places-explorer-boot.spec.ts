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
// header, at every width.
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

    // The fixture's own places/index.md carries a real intro paragraph —
    // task 1's composition keeps it above the map, which this file's own
    // layout must not disturb.
    const intro = page.getByText("Every work this site locates, gathered on one map.");
    const introBox = (await intro.boundingBox())!;
    expect(introBox.y + introBox.height).toBeLessThanOrEqual(figureBox.y + 1);
  });
}
