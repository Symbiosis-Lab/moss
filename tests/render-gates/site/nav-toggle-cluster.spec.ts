/**
 * Render gate: the nav toggle cluster is one optical family.
 *
 * Four assertions, each of which needs a real engine and cannot be read off
 * the emitted HTML:
 *
 *   1. The three `.nav-icons` children (search button, language toggle, theme
 *      button) lay out at the SAME height — the 2.25rem box. The heights come
 *      from three different rules in two different stylesheets, so only
 *      layout can confirm they agree.
 *
 *   2. `.nav-search-btn` has NO resting background — the pill is a hover
 *      affordance only. `background: none` computing to transparent is a
 *      cascade question across two stylesheets, so it needs the engine; the
 *      radius that makes it a pill does not, and is asserted in Rust.
 *
 *   3. `.moss-search__seam` — the hairline under the search input — paints
 *      `--moss-border-light`, NOT the accent: a green rule there would sit
 *      under a field that is always focused and read as a focus underline.
 *      Custom-property resolution through `var()` is exactly what jsdom
 *      cannot do, and the seam only exists after the overlay is opened by
 *      the runtime.
 *
 *   4. When a long breadcrumb wraps the masthead with scripts off, the
 *      toggle cluster sits at the END edge of the last row and nothing
 *      overlaps or overflows. Whether it wrapped depends on measured text
 *      width, not a breakpoint, so only a layout engine can tell.
 *
 * The fixture turns search on via `[site].search`.
 *
 * Run via:
 *   npx playwright test -c playwright/nav-toggle-cluster.config.ts
 */

import { test, expect, type Page, newPage } from "../../support/browser-test";

/** Computed background-color of a throwaway element painted with `value`. */
async function resolveColor(page: Page, value: string): Promise<string> {
  return page.evaluate((v: string) => {
    const probe = document.createElement("div");
    probe.style.backgroundColor = v;
    document.body.appendChild(probe);
    const computed = window.getComputedStyle(probe).backgroundColor;
    probe.remove();
    return computed;
  }, value);
}

test.beforeEach(async ({ page }) => {
  // Wide enough that nav-right stays on row 1 with the links beside the cluster.
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/", { waitUntil: "load" });
});

test("the three nav toggles share one 2.25rem box", async ({ page }) => {
  const heights = await page.evaluate(() => {
    const ruler = document.createElement("div");
    ruler.style.height = "2.25rem";
    document.body.appendChild(ruler);
    const expected = ruler.getBoundingClientRect().height;
    ruler.remove();
    const children = [...document.querySelectorAll(".nav-icons > *")];
    return {
      expected,
      classes: children.map((el) => el.className),
      measured: children.map((el) => el.getBoundingClientRect().height),
    };
  });

  // All three must be present, or the gate is measuring a nav that never had
  // the problem. Their ORDER is provable from the emitted HTML and is pinned in
  // a Rust test (`test_icon_cluster_order_is_search_language_theme`), so it is
  // deliberately not re-asserted here.
  expect([...heights.classes].sort()).toEqual([
    "nav-lang-toggle",
    "nav-search-btn",
    "nav-theme-btn",
  ]);
  for (const [i, h] of heights.measured.entries()) {
    expect(h, `${heights.classes[i]} must be the 2.25rem box`).toBeCloseTo(
      heights.expected,
      1,
    );
  }
});

test("the search button has no resting fill", async ({ page }) => {
  const resting = await page
    .locator(".nav-search-btn")
    .evaluate((el) => window.getComputedStyle(el).backgroundColor);
  expect(resting, "search must be transparent at rest, like its siblings").toBe(
    "rgba(0, 0, 0, 0)",
  );

  // That the pill still EXISTS — `border-radius: 18px` — is provable from the
  // emitted text and is asserted in `test_css_search_button_keeps_its_pill`.
  // Reading it back through getComputedStyle here would just echo a literal
  // declaration with no cascade or var() resolution involved, which is the
  // definition of an assertion at the wrong level.
});

test("the search overlay seam is a neutral hairline, not an accent underline", async ({
  page,
}) => {
  await page.locator(".nav-search-btn").click();
  // The seam divides results from the field, so it shows only once a row follows it.
  await page.locator(".moss-search__results").evaluate((ul) =>
    ul.insertAdjacentHTML("beforeend", '<li class="moss-search__row"><a class="moss-search__link" href="#">x</a></li>'),
  );
  const seam = page.locator(".moss-search__seam");
  await expect(seam).toBeVisible();

  const painted = await seam.evaluate(
    (el) => window.getComputedStyle(el).backgroundColor,
  );
  const borderLight = await resolveColor(page, "var(--moss-border-light)");
  const accent = await resolveColor(page, "var(--moss-color-accent)");
  const accentQuiet = await resolveColor(page, "var(--moss-color-accent-quiet)");

  expect(painted, "seam must paint --moss-border-light").toBe(borderLight);
  expect(painted).not.toBe(accent);
  expect(painted, "the green focus-underline regression").not.toBe(accentQuiet);
});

test("a wrapped masthead keeps the toggles at the end edge of the last row", async ({
  browser,
  baseURL,
}) => {
  // The no-script fallback (nav-split.ts replaces it when scripts run), and a
  // trail page's first paint. 1024px is well above the 48rem breakpoint, so the
  // wrap comes from the trail's measured width alone.
  const context = await browser.newContext({ baseURL, javaScriptEnabled: false, viewport: { width: 1024, height: 900 } });
  const page = await newPage(context);
  await page.goto(
    "/programmes-and-long-form-reporting/field-notes-from-a-very-long-section-title/",
    { waitUntil: "load" },
  );

  const box = await page.evaluate(() => {
    const rect = (sel: string) => {
      const r = document.querySelector(sel)!.getBoundingClientRect();
      return { top: r.top, bottom: r.bottom, left: r.left, right: r.right };
    };
    const content = document.querySelector(".nav-content") as HTMLElement;
    const cs = window.getComputedStyle(content);
    const c = content.getBoundingClientRect();
    return {
      start: c.left + parseFloat(cs.paddingLeft),
      end: c.right - parseFloat(cs.paddingRight),
      trail: rect(".nav-left"),
      links: rect(".nav-links"),
      icons: rect(".nav-icons"),
    };
  });
  const where = JSON.stringify(box);

  // Precondition: the trail wrapped the masthead. Without it the assertions
  // below pass on a single row and test nothing.
  expect(box.icons.top, `the fixture must wrap the toggles below the trail: ${where}`).toBeGreaterThanOrEqual(box.trail.bottom);
  expect(box.icons.top, `the toggles are on the last row: ${where}`).toBeGreaterThanOrEqual(Math.max(box.trail.top, box.links.top));
  expect(box.icons.right, `the toggles sit at the end edge: ${where}`).toBeCloseTo(box.end, 0);
  const overlaps = box.links.top < box.trail.bottom && box.links.bottom > box.trail.top
    && box.links.left < box.trail.right && box.links.right > box.trail.left;
  expect(overlaps, `the links must not overlap the trail: ${where}`).toBe(false);
  for (const [name, r] of Object.entries({ trail: box.trail, links: box.links, icons: box.icons })) {
    expect(r.left, `${name} inside the start edge: ${where}`).toBeGreaterThanOrEqual(box.start - 0.5);
    expect(r.right, `${name} inside the end edge: ${where}`).toBeLessThanOrEqual(box.end + 0.5);
  }
  await context.close();
});

test("a theme's own focus ring draws no second box inside the search field", async ({ page }) => {
  // A theme stylesheet sits outside moss's cascade layers, as this does.
  await page.addStyleTag({ content: ":focus-visible { outline: 2px solid rgb(255, 0, 0); outline-offset: 3px; }" });
  await page.locator(".nav-search-btn").click();
  const input = page.locator(".moss-search__input");
  await expect(input).toBeFocused();

  expect(await input.evaluate((el) => window.getComputedStyle(el).outlineStyle)).toBe("none");
  const border = await page
    .locator(".moss-search__panel")
    .evaluate((el) => window.getComputedStyle(el).borderTopColor);
  expect(border, "the panel's own border shows the focus").toBe(
    await resolveColor(page, "var(--moss-color-ui-accent)"),
  );
});

test("opening search does not move the page sideways under a classic scrollbar", async ({ playwright, browserName, baseURL }) => {
  // Headless Chromium hides scrollbars; a reader on Windows, Linux or a Mac set
  // to always show them sees one, and that is the width the lock must keep.
  test.skip(browserName !== "chromium", "only Chromium can be told to draw classic scrollbars");
  const browser = await playwright.chromium.launch({ ignoreDefaultArgs: ["--hide-scrollbars"] });
  try {
    const page = await newPage(browser, { baseURL, viewport: { width: 1024, height: 240 } });
    await page.goto("/", { waitUntil: "load" });
    // A styled scrollbar takes layout width whatever the OS shows; macOS's
    // default overlays one whenever a trackpad is in use.
    await page.addStyleTag({ content: "::-webkit-scrollbar { width: 15px } ::-webkit-scrollbar-thumb { background: gray }" });
    const scrollbar = await page.evaluate(() => window.innerWidth - document.documentElement.clientWidth);
    expect(scrollbar, "the page must scroll and draw a classic scrollbar, or this measures nothing").toBeGreaterThan(0);

    const header = page.locator(".nav-content");
    const before = await header.boundingBox();
    await page.locator(".nav-search-btn").click();
    await expect(page.locator(".moss-search__input")).toBeFocused();
    expect(await header.boundingBox()).toEqual(before);
  } finally {
    await browser.close();
  }
});

test("the focused search field shows a 2px accent edge, and a 2px outline under forced colors", async ({ page }) => {
  await page.locator(".nav-search-btn").click();
  const panel = page.locator(".moss-search__panel");
  const accent = await resolveColor(page, "var(--moss-color-ui-accent)");
  expect(await panel.evaluate((el) => window.getComputedStyle(el).boxShadow)).toBe(`${accent} 0px 0px 0px 1px inset`);

  await page.emulateMedia({ forcedColors: "active" });
  const forced = await panel.evaluate((el) => {
    const cs = window.getComputedStyle(el);
    return [cs.outlineStyle, cs.outlineWidth, cs.outlineOffset];
  });
  expect(forced).toEqual(["solid", "1px", "-2px"]);
});

test("Tab onto a result lights that row alone, through a theme's own focus ring", async ({ page }) => {
  await page.addStyleTag({ content: ":focus-visible { outline: 2px solid rgb(255, 0, 0); outline-offset: 3px; }" });
  await page.locator(".nav-search-btn").click();
  await page.keyboard.type("notes");
  const rows = page.locator(".moss-search__link");
  await expect(rows.nth(1), "the fixture must return at least two results").toBeVisible();

  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(rows.nth(1)).toBeFocused();
  await expect(page.locator(".moss-search__link.is-selected")).toHaveCount(1);
  await expect(rows.nth(1)).toHaveClass(/is-selected/);
  expect(await rows.nth(1).evaluate((el) => window.getComputedStyle(el).outlineStyle)).toBe("none");
});

test("closing search leaves the page where the reader left it", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 240 });
  await page.goto("/", { waitUntil: "load" });
  await page.evaluate(() => window.scrollTo(0, 200));
  const before = await page.evaluate(() => window.scrollY);
  expect(before, "the page must scroll for this to measure anything").toBeGreaterThan(0);
  await page.keyboard.press("/");
  await expect(page.locator(".moss-search__input")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator(".nav-search-btn")).toBeFocused();
  expect(await page.evaluate(() => window.scrollY)).toBe(before);
});
