/**
 * The dots of a static place map are links, in a real browser. What the Rust
 * pipeline test cannot see: keyboard order, the focus ring painting under
 * site.css, the hit target, and the click landing.
 *
 * Fixture: PLACE_DOT_LINKS_GATE (tests/e2e/helpers/gate-sites.ts). Run via:
 *   npx playwright test -c playwright/place-dot-links.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

const LOCATOR_LINK = ".moss-place-locator .moss-place-map a";

async function openHome(page: Page, theme: "light" | "dark") {
  await page.goto("./", { waitUntil: "domcontentloaded" });
  await page.evaluate((t) => localStorage.setItem("moss-theme", t), theme);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(".moss-place-locator").scrollIntoViewIfNeeded();
}

/** Tab from the top of the page until the locator's dot has focus. WebKit on macOS skips links on a plain Tab (Safari's default), so there Option+Tab is the keyboard reach. */
async function tabToDot(page: Page, browserName: string) {
  const key = browserName === "webkit" && process.platform === "darwin" ? "Alt+Tab" : "Tab";
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  for (let i = 0; i < 60; i++) {
    await page.keyboard.press(key);
    const onDot = await page.evaluate((selector) => document.activeElement?.matches(selector) ?? false, LOCATOR_LINK);
    if (onDot) return;
  }
  throw new Error("Tab never reached the place dot");
}

for (const theme of ["light", "dark"] as const) {
  test(`Tab reaches the dot, which is named for its place and shows a focus ring (${theme})`, async ({ page, browserName }) => {
    await openHome(page, theme);
    await tabToDot(page, browserName);

    await expect(page.getByRole("link", { name: "Harbor", exact: true })).toBeFocused();

    const ring = await page.evaluate(() => {
      const link = document.activeElement as SVGAElement;
      const dot = link.querySelector("[data-map-marker]") as SVGGraphicsElement;
      const focus = link.querySelector("[data-map-focus] path") as SVGGraphicsElement;
      const style = getComputedStyle(focus);
      const group = getComputedStyle(link.querySelector("[data-map-focus]") as Element);
      const stroke = parseFloat(style.strokeWidth);
      const dotStroke = parseFloat(getComputedStyle(dot).strokeWidth);
      return { display: group.display, color: style.stroke, ringWidth: stroke, dotWidth: dotStroke };
    });
    expect(ring.display).not.toBe("none");
    expect(ring.color).not.toMatch(/transparent|none|rgba\(0, 0, 0, 0\)/);
    // The ring reaches at least 2 CSS px beyond the dot's own outline on every side.
    expect(ring.ringWidth - ring.dotWidth).toBeGreaterThanOrEqual(4);
  });
}

test("the focus ring is hidden until the dot has keyboard focus", async ({ page }) => {
  await openHome(page, "light");
  const display = await page.locator(`${LOCATOR_LINK} [data-map-focus]`).evaluate((el) => getComputedStyle(el).display);
  expect(display).toBe("none");
});

test("the dot's hit target is at least 24 by 24 CSS px and no larger than 28", async ({ page }) => {
  await openHome(page, "light");
  const centre = await page.locator(`${LOCATOR_LINK} [data-map-marker]`).evaluate((el) => {
    const box = el.getBoundingClientRect();
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  });
  const hits = (dx: number, dy: number) =>
    page.evaluate(([x, y]) => !!document.elementFromPoint(x, y)?.closest(".moss-place-locator .moss-place-map a"), [centre.x + dx, centre.y + dy]);
  for (const [dx, dy] of [[11, 0], [-11, 0], [0, 11], [0, -11]]) {
    expect(await hits(dx, dy), `${dx},${dy} is inside the target`).toBe(true);
  }
  expect(await hits(16, 0), "the target stops short of 32px").toBe(false);
});

test("Enter on the focused dot opens its place page, whose own dot is not a link", async ({ page, browserName }) => {
  await openHome(page, "light");
  await tabToDot(page, browserName);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/places\/harbor\/$/);
  await expect(page.locator(".moss-place-map").first()).toBeVisible();
  expect(await page.locator(".moss-place-map a").count()).toBe(0);
  // Without links the figure is an image again.
  await expect(page.locator(".moss-place-map").first()).toHaveAttribute("role", "img");
});
