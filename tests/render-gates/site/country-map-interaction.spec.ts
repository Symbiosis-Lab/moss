/**
 * A country map in a real browser: hover previews, the legend lift, framing
 * beside the panel, the sideways box and bottom sheet on a phone, the first-view
 * tint, and the keyboard path through the legend and the search field.
 *
 * Run via:
 *   npx playwright test -c playwright/country-map-interaction.config.ts
 */
import { test, expect, type Page, type Locator } from "@playwright/test";

// Plain Tab reaches only form fields in macOS WebKit unless the system's "Keyboard navigation" setting is on;
// Option+Tab reaches every control regardless, as a Mac keyboard user would press it.
const TAB = process.platform === "darwin" ? "Alt+Tab" : "Tab";
const WIDE = { width: 1280, height: 800 };
const PHONE = { width: 390, height: 800 };
const path = (page: Page, slug: string) => page.locator(`.moss-country[data-slug="${slug}"]`);
const viewBox = async (page: Page) => (await page.locator(".moss-country-map svg").getAttribute("viewBox"))!.split(" ").map(Number);
const box = async (locator: Locator) => (await locator.boundingBox())!;
const fillOf = (locator: Locator) => locator.evaluate((el) => getComputedStyle(el).fill);

/** A pointermove at a page position, as if the pointer sat over `slug` there. */
const pointAt = (page: Page, slug: string, x: number, y: number) =>
  path(page, slug).evaluate((el, [cx, cy]) => el.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: cx, clientY: cy, pointerType: "mouse" })), [x, y]);

/** Luminance of any CSS colour, through the browser's own canvas. */
async function luminance(page: Page, colour: string): Promise<number> {
  const [r, g, b] = await page.evaluate((c) => {
    const ctx = document.createElement("canvas").getContext("2d")!;
    ctx.fillStyle = c;
    ctx.fillRect(0, 0, 1, 1);
    return Array.from(ctx.getImageData(0, 0, 1, 1).data.slice(0, 3));
  }, colour);
  const [lr, lg, lb] = [r, g, b].map((v) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
}
const contrast = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

test.describe("wide screen", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(WIDE);
  });

  for (const scheme of ["light", "dark"] as const) {
    test(`${scheme}: hovering previews a rated and an unrated country without committing`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto("/");
      const tip = page.locator(".moss-country-tip");
      const rest = await fillOf(path(page, "brazil"));
      await path(page, "brazil").hover();
      await expect(tip).toBeVisible();
      await expect(tip).toHaveText("Brazil · Elevated");
      const lifted = await fillOf(path(page, "brazil"));
      expect(lifted).not.toBe(rest);
      const land = await luminance(page, await fillOf(path(page, "niger")));
      expect(contrast(await luminance(page, lifted), land), "a lifted level still reads against land").toBeGreaterThanOrEqual(3);
      const tipBox = await box(tip);
      const stage = await box(page.locator(".moss-country-stage"));
      expect(tipBox.x).toBeGreaterThanOrEqual(stage.x);
      expect(tipBox.x + tipBox.width).toBeLessThanOrEqual(stage.x + stage.width);
      const textColour = await tip.evaluate((el) => getComputedStyle(el).color);
      const ground = await tip.evaluate((el) => getComputedStyle(el).backgroundColor);
      expect(contrast(await luminance(page, textColour), await luminance(page, ground))).toBeGreaterThanOrEqual(4.5);

      const landRest = await fillOf(path(page, "chad"));
      await path(page, "chad").hover();
      await expect(tip).toHaveText("Chad · Not rated");
      expect(await fillOf(path(page, "chad"))).not.toBe(landRest);

      await page.locator(".moss-country-disputed").first().hover({ position: { x: 1, y: 1 }, force: true });
      await expect(page.locator(".moss-country-panel")).toBeHidden();
      await expect(page.locator(".is-chosen")).toHaveCount(0);
      expect(await viewBox(page)).toEqual([0, 0, 1000, (await viewBox(page))[3]]);
    });
  }

  test("the label flips off the map's right edge and away from the panel", async ({ page }) => {
    await page.goto("/");
    const stage = await box(page.locator(".moss-country-stage"));
    const tip = page.locator(".moss-country-tip");
    const y = stage.y + 120;
    await pointAt(page, "brazil", stage.x + stage.width - 4, y);
    const edge = await box(tip);
    expect(edge.x + edge.width).toBeLessThanOrEqual(stage.x + stage.width - 7);
    expect(edge.x + edge.width, "flipped to the pointer's left").toBeLessThanOrEqual(stage.x + stage.width - 4);

    await path(page, "brazil").click();
    const panel = await box(page.locator(".moss-country-panel"));
    for (const x of [panel.x - 30, panel.x + 40]) {
      await pointAt(page, "chad", x, panel.y + 60);
      const hidden = await tip.evaluate((el) => (el as HTMLElement).hidden);
      if (hidden) continue;
      const at = await box(tip);
      const overlaps = at.x < panel.x + panel.width && at.x + at.width > panel.x && at.y < panel.y + panel.height && at.y + at.height > panel.y;
      expect(overlaps, `label at ${x} covers the panel`).toBe(false);
    }
  });

  test("hovering a legend level lifts it and dims the others; the legend is no tab stop", async ({ page }) => {
    await page.goto("/");
    const items = page.locator(".moss-country-legend li");
    const rest = await fillOf(path(page, "brazil"));
    await items.nth(2).hover();
    await expect(page.locator(".moss-country-map")).toHaveClass(/is-lifting/);
    expect(await fillOf(path(page, "brazil"))).not.toBe(rest);
    await expect(path(page, "mexico")).toHaveCSS("opacity", "0.3");
    await page.mouse.move(5, 5);
    await expect(page.locator(".moss-country-map")).not.toHaveClass(/is-lifting/);

    await page.locator(".moss-country-search input").focus();
    await page.keyboard.press(TAB);
    expect(await page.evaluate(() => !!document.activeElement?.closest(".moss-country-legend")), "Tab skips the legend").toBe(false);
  });

  test("a keyboard pick focuses the panel heading and Esc returns to the field; a tap leaves focus alone", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await field.fill("brazil");
    await field.press("Enter");
    await expect(page.locator(".moss-country-panel-name")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.locator(".moss-country-panel")).toBeHidden();
    await expect(field).toBeFocused();
    await field.blur();
    await path(page, "japan").click();
    await expect(page.locator(".moss-country-panel")).toBeVisible();
    // The click itself focuses the nearest focusable ancestor (`main` is the skip link's target); the panel must not take it.
    expect(await page.evaluate(() => !document.activeElement?.closest(".moss-country-panel, .moss-country-search")), "a tap leaves focus where the click put it").toBe(true);
  });

  test("search: the combobox finds a country or a city, chooses it and frames it beside the panel", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await page.keyboard.press(TAB);
    await page.keyboard.press(TAB);
    await field.focus();
    expect(await field.evaluate((el) => parseFloat(getComputedStyle(el).outlineWidth)), "the field shows focus").toBeGreaterThanOrEqual(2);
    await expect(field).toHaveAttribute("role", "combobox");
    await field.fill("sao pa");
    const rows = page.locator(".moss-country-search [role=option]");
    await expect(rows.first()).toContainText("São Paulo, Brazil");
    await expect(rows.first()).toContainText("Elevated");
    await expect(rows.first()).toHaveAttribute("aria-selected", "true");
    await expect(field).toHaveAttribute("aria-expanded", "true");
    await expect(page.locator(".moss-country-search-status")).toHaveText("1 result");
    await field.press("Enter");
    const panel = page.locator(".moss-country-panel");
    await expect(panel).toContainText("Brazil");
    expect(new URL(page.url()).hash).toBe("#brazil");
    await expect(rows).toHaveCount(0);

    await expect.poll(async () => (await viewBox(page))[2], { timeout: 3000 }).toBeLessThan(1000);
    await page.waitForTimeout(500);
    const brazil = await box(path(page, "brazil"));
    const p = await box(panel);
    const stage = await box(page.locator(".moss-country-stage"));
    expect(brazil.x).toBeGreaterThanOrEqual(stage.x - 1);
    expect(brazil.x + brazil.width, "the country sits beside the panel").toBeLessThanOrEqual(p.x + 1);

    await field.fill("zzzz");
    await expect(page.locator(".moss-country-search-none")).toContainText("zzzz");
    await expect(page.locator(".moss-country-search-none a")).toHaveAttribute("href", "#moss-country-rows");
    await field.press("Escape");
    await expect(panel).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden();
    await expect.poll(async () => (await viewBox(page))[2]).toBe(1000);
  });

  test("framing is capped at 2.5x for a tiny country and never below the world view for a vast one", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await field.fill("singapore");
    await field.press("Enter");
    await expect.poll(async () => (await viewBox(page))[2], { timeout: 3000 }).toBeCloseTo(400, 0);
    await field.fill("russia");
    await field.press("Enter");
    await expect.poll(async () => (await viewBox(page))[2], { timeout: 3000 }).toBeCloseTo(1000, 0);
  });

  test("the address frames on load without moving through the world view", async ({ page }) => {
    await page.goto("/#japan");
    await expect(page.locator(".moss-country-panel")).toContainText("Japan");
    expect((await viewBox(page))[2]).toBeLessThan(1000);
  });

  test("reduced motion jumps to the frame", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await path(page, "japan").click();
    expect((await viewBox(page))[2]).toBeLessThan(1000);
    await expect(page.locator(".moss-country-map")).not.toHaveClass(/is-arming/);
  });

  test("the levels fade in lowest first, once, and settle", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".moss-country-map")).toHaveClass(/is-tinted/);
    await expect(path(page, "brazil")).toHaveCSS("opacity", "1", { timeout: 3000 });
    await expect(path(page, "mexico")).toHaveCSS("opacity", "1");
  });
});

test.describe("phone", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(PHONE);
  });

  test("the world scrolls sideways inside its box and the page does not", async ({ page }) => {
    await page.goto("/");
    const scroll = page.locator(".moss-country-scroll");
    const m = await scroll.evaluate((el) => ({ scrollWidth: el.scrollWidth, clientWidth: el.clientWidth, overflowY: getComputedStyle(el).overflowY, touch: getComputedStyle(el).touchAction }));
    expect(m.scrollWidth).toBeGreaterThan(m.clientWidth * 1.5);
    expect(m.overflowY).toBe("hidden");
    expect(m.touch).toBe("pan-x pan-y");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });

  test("choosing rises a bottom sheet and scrolls the country to the middle", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await field.fill("brazil");
    await field.press("Enter");
    const panel = page.locator(".moss-country-panel");
    await expect(panel).toContainText("Brazil");
    await page.waitForTimeout(500);
    const p = await box(panel);
    expect(Math.abs(p.y + p.height - PHONE.height), "the sheet sits on the bottom edge").toBeLessThanOrEqual(1);
    expect(p.width).toBe(PHONE.width);
    expect(await panel.evaluate((el) => getComputedStyle(el, "::before").content)).not.toBe("none");
    await expect(panel.locator(".moss-country-panel-close")).toBeVisible();
    await expect(panel.locator("a", { hasText: "Plan a trip" })).toBeVisible();
    const scroll = await box(page.locator(".moss-country-scroll"));
    const brazil = await box(path(page, "brazil"));
    expect(Math.abs(brazil.x + brazil.width / 2 - (scroll.x + scroll.width / 2))).toBeLessThanOrEqual(8);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    expect((await viewBox(page))[2], "the view is not zoomed on a phone").toBe(1000);

    await expect(panel.locator(".moss-country-panel-name"), "opened from the keyboard").toBeFocused();
    await page.keyboard.press(TAB);
    await expect(panel.locator(".moss-country-panel-close"), "the sheet is reachable: close, then the action").toBeFocused();
    await page.keyboard.press(TAB);
    await expect(panel.locator("a", { hasText: "Plan a trip" })).toBeFocused();
    await panel.locator(".moss-country-panel-close").click();
    await expect(panel).toBeHidden();
  });

  test("a tap on a country chooses it at once", async ({ page }) => {
    await page.goto("/");
    await path(page, "brazil").evaluate((el) => el.scrollIntoView({ inline: "center" }));
    await path(page, "brazil").click();
    await expect(page.locator(".moss-country-panel")).toContainText("Brazil");
  });
});

test.describe("without script", () => {
  test.use({ javaScriptEnabled: false });
  test("every level shows at once and the field is a link to the list", async ({ page }) => {
    await page.goto("/");
    await expect(path(page, "brazil")).toHaveCSS("opacity", "1");
    const link = page.locator("a.moss-country-search");
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute("href", "#moss-country-rows");
    await expect(page.locator(".moss-country-list")).toHaveAttribute("id", "moss-country-rows");
  });
});

test.describe("a list placed apart with style:levels", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(WIDE);
  });

  test("the list leaves the map, sits where it is written, and keeps its colours", async ({ page }) => {
    await page.goto("/apart/");
    await expect(page.locator(".moss-country-list")).toHaveCount(1);
    await expect(page.locator(".moss-country-map .moss-country-list")).toHaveCount(0);
    const after = await page.evaluate(() => {
      const para = [...document.querySelectorAll("p")].find((p) => p.textContent?.includes("Between the map and its list."))!;
      return !!(para.compareDocumentPosition(document.querySelector(".moss-country-list")!) & Node.DOCUMENT_POSITION_FOLLOWING);
    });
    expect(after, "the list follows the paragraph written above it").toBe(true);
    const swatch = page.locator('.moss-country-list .moss-country-swatch[data-level="4"]').first();
    expect(await swatch.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(await fillOf(path(page, "chile")));
  });

  test("a row opened from the keyboard focuses the panel, and Esc returns to the row", async ({ page }) => {
    await page.goto("/apart/");
    const row = page.locator('.moss-country-list a[href="#japan"]');
    await row.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(".moss-country-panel")).toContainText("Japan");
    await expect(page.locator(".moss-country-panel-name")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(row).toBeFocused();
  });
});

test.describe("a list placed apart, without script", () => {
  test.use({ javaScriptEnabled: false });
  test("the search field links to the list where it is written", async ({ page }) => {
    await page.goto("/apart/");
    await expect(page.locator("a.moss-country-search")).toHaveAttribute("href", "#moss-country-rows");
    await expect(page.locator(".moss-country-list")).toHaveAttribute("id", "moss-country-rows");
  });
});
