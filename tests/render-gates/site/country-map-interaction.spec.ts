/**
 * A country map in a real browser: hover previews, the legend lift, framing
 * beside the panel, zoom (controls, ctrl/⌘ + wheel, pinch, drag, keyboard), the
 * bottom sheet on a phone, the first-view tint, and the keyboard path through
 * the legend and the search field.
 *
 * Run via:
 *   npx playwright test -c playwright/country-map-interaction.config.ts
 */
import { test, expect, type Page, type Locator } from "@playwright/test";
import { expectFocusOnOwnEdge } from "./field-focus-edge";

// Plain Tab reaches only form fields in macOS WebKit unless the system's "Keyboard navigation" setting is on;
// Option+Tab reaches every control regardless, as a Mac keyboard user would press it.
const TAB = process.platform === "darwin" ? "Alt+Tab" : "Tab";
const WIDE = { width: 1280, height: 800 };
const PHONE = { width: 390, height: 800 };
const path = (page: Page, slug: string) => page.locator(`.moss-country[data-slug="${slug}"]`);
const viewBox = async (page: Page) => (await page.locator(".moss-country-map svg").getAttribute("viewBox"))!.split(" ").map(Number);
const WORLD_HEIGHT = 435;
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

  test("search: a click into the field shows focus on its own edge, under a theme focus ring", async ({ page }) => {
    await page.goto("/");
    await expectFocusOnOwnEdge(page, page.locator(".moss-country-search input"), "country search");
  });

  test("search: the combobox finds a country or a city, chooses it and frames it beside the panel", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await page.keyboard.press(TAB);
    await page.keyboard.press(TAB);
    await field.focus();
    expect(await field.evaluate((el) => getComputedStyle(el).boxShadow), "the field shows focus").not.toBe("none");
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

  test("the map is one box with no sideways scroll box, opening on the world's full height", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator(".moss-country-scroll")).toHaveCount(0);
    const stage = await box(page.locator(".moss-country-stage"));
    expect(stage.height / stage.width, "about the square it was").toBeGreaterThan(0.8);
    const [x, y, w, h] = await viewBox(page);
    expect(y).toBe(0);
    expect(h, "the world's full height").toBeCloseTo(WORLD_HEIGHT, 0);
    expect(x + w / 2, "centred on the world's middle").toBeCloseTo(500, 0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });

  test("choosing rises a bottom sheet and frames the country in the middle of the map", async ({ page }) => {
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
    const stage = await box(page.locator(".moss-country-stage"));
    const brazil = await box(path(page, "brazil"));
    expect(Math.abs(brazil.x + brazil.width / 2 - (stage.x + stage.width / 2))).toBeLessThanOrEqual(8);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);

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
    await path(page, "brazil").click();
    await expect(page.locator(".moss-country-panel")).toContainText("Brazil");
  });
});

const stageOf = (page: Page) => page.locator(".moss-country-stage");
const control = (page: Page, name: string) => page.locator(`.moss-country-stage [data-control="${name}"]`);
const widthOf = async (page: Page) => (await viewBox(page))[2];

test.describe("zoom", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 500 });
  });

  test("the shared controls sit bottom right, named, each with a 44px hit area", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator('.moss-country-stage [role="group"][aria-label="Map controls"]')).toHaveCount(1);
    await expect(control(page, "zoom-in")).toHaveAccessibleName("Zoom in");
    await expect(control(page, "zoom-out")).toHaveAccessibleName("Zoom out");
    await expect(control(page, "reset"), "out of sight and out of the tab order at the whole view").toBeHidden();
    // Zoomed in once, every control is live: a disabled one takes no pointer, so it has no hit area to measure.
    await control(page, "zoom-in").click();
    await expect(control(page, "reset")).toHaveAccessibleName("Show the whole map");
    for (const name of ["zoom-in", "zoom-out", "reset"]) {
      await control(page, name).evaluate((el) => ((el as HTMLElement).hidden = false));
      // elementFromPoint answers null outside the viewport, and the short viewport leaves the controls below the fold.
      await control(page, name).evaluate((el) => el.scrollIntoView({ block: "center" }));
      const hit = await control(page, name).evaluate((el) => {
        const r = el.getBoundingClientRect();
        // The painted circle is 36px; a point 3px outside it still lands on the button through its ::after.
        const hits = (x: number, y: number) => document.elementFromPoint(x, y) === el;
        return { painted: [r.width, r.height], outside: [hits(r.left - 3, r.top + r.height / 2), hits(r.right + 3, r.top + r.height / 2), hits(r.left + r.width / 2, r.top - 3), hits(r.left + r.width / 2, r.bottom + 3)] };
      });
      expect(hit.painted, name).toEqual([36, 36]);
      // The capsule's two cells abut, so the side facing the other cell is the other cell's.
      const shared = name === "zoom-in" ? 3 : name === "zoom-out" ? 2 : -1;
      expect(hit.outside.filter((_, side) => side !== shared).every(Boolean), `${name}: hit area reaches 44px`).toBe(true);
      await control(page, name).evaluate((el) => ((el as HTMLElement).hidden = name === "reset"));
    }
    await page.reload();
    const stage = await box(stageOf(page));
    const controls = await box(page.locator(".moss-country-stage .moss-places-controls"));
    expect(Math.abs(stage.x + stage.width - 12 - (controls.x + controls.width))).toBeLessThanOrEqual(1);
    expect(Math.abs(stage.y + stage.height - 12 - (controls.y + controls.height))).toBeLessThanOrEqual(1);
    await path(page, "brazil").click();
    const panel = await box(page.locator(".moss-country-panel"));
    expect(panel.y + panel.height <= controls.y || controls.y + controls.height <= panel.y || panel.x + panel.width <= controls.x, "the controls stay clear of the panel").toBe(true);
  });

  test("at the whole view only + works and reset is out of the tab order; + then − changes and restores the view", async ({ page }) => {
    await page.goto("/");
    const start = await viewBox(page);
    await expect(control(page, "zoom-out")).toBeDisabled();
    await expect(control(page, "zoom-in")).toBeEnabled();
    await expect(control(page, "reset")).toBeHidden();
    await control(page, "zoom-in").click();
    await expect.poll(() => widthOf(page), { timeout: 3000 }).toBeCloseTo(1000 / 1.5, 0);
    await expect(control(page, "zoom-out")).toBeEnabled();
    await expect(control(page, "reset")).toBeVisible();
    await control(page, "zoom-out").click();
    await expect.poll(async () => (await viewBox(page)).join(" "), { timeout: 3000 }).toBe(start.join(" "));
    await expect(control(page, "reset")).toBeHidden();
  });

  test("zooming in stops at 8 times the whole view and + disables there; reset returns", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    for (let i = 0; i < 12; i++) if (await control(page, "zoom-in").isEnabled()) await control(page, "zoom-in").click();
    expect(await widthOf(page)).toBeCloseTo(125, 0);
    await expect(control(page, "zoom-in")).toBeDisabled();
    await control(page, "reset").click();
    expect(await widthOf(page)).toBe(1000);
  });

  test("reduced motion jumps on the buttons", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await control(page, "zoom-in").click();
    expect(await widthOf(page)).toBeCloseTo(1000 / 1.5, 0);
  });

  test("ctrl + wheel zooms at the pointer; a plain wheel scrolls the page and leaves the map", async ({ page }) => {
    await page.goto("/");
    const stage = await box(stageOf(page));
    await page.mouse.move(stage.x + stage.width / 2, stage.y + stage.height / 2);
    await page.mouse.wheel(0, 300);
    await expect.poll(() => page.evaluate(() => window.scrollY), "a plain wheel scrolls the page").toBeGreaterThan(0);
    expect(await widthOf(page)).toBe(1000);
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.mouse.move(stage.x + 100, stage.y + stage.height / 2);
    await page.keyboard.down("Control");
    await page.mouse.wheel(0, -200);
    await page.keyboard.up("Control");
    await expect.poll(() => widthOf(page)).toBeLessThan(1000);
    expect(await page.evaluate(() => window.scrollY), "ctrl + wheel does not scroll").toBe(0);
  });

  test("the stage is a named, described region; + − 0 and the arrows work while it has focus", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    const stage = stageOf(page);
    await expect(stage).toHaveAttribute("role", "region");
    await expect(stage).toHaveAccessibleName(/.+/);
    await expect(stage).toHaveAccessibleDescription("Use + and − to zoom, and the arrow keys to move.");
    await stage.focus();
    expect(await stage.evaluate((el) => parseFloat(getComputedStyle(el).outlineWidth))).toBeGreaterThanOrEqual(0);
    await page.keyboard.press("+");
    expect(await widthOf(page)).toBeCloseTo(1000 / 1.5, 0);
    const x0 = (await viewBox(page))[0];
    await page.keyboard.press("ArrowRight");
    const x1 = (await viewBox(page))[0];
    expect(x1).toBeGreaterThan(x0);
    await page.keyboard.press("Shift+ArrowRight");
    expect((await viewBox(page))[0] - x1, "Shift moves further").toBeGreaterThan(x1 - x0);
    await page.keyboard.press("0");
    expect(await widthOf(page)).toBe(1000);
    await page.keyboard.press("+");
    await page.keyboard.press("-");
    expect(await widthOf(page)).toBe(1000);
  });

  test("a mouse drag moves the map when zoomed in and never chooses the country it started on", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await control(page, "zoom-in").click();
    await control(page, "zoom-in").click();
    const before = await viewBox(page);
    // Press on land that is on screen: a country's own middle can sit below the short viewport or off the zoomed view.
    const landOnScreen = () => page.evaluate(() => {
      const r = document.querySelector(".moss-country-stage")!.getBoundingClientRect();
      for (let x = r.left + 60; x < r.right - 60; x += 20)
        for (let y = r.top + 20; y < Math.min(r.bottom, innerHeight) - 60; y += 20) {
          const slug = document.elementFromPoint(x, y)?.closest<SVGElement>(".moss-country")?.dataset.slug;
          if (slug) return { x, y, slug };
        }
      return null;
    });
    const start = await landOnScreen();
    expect(start, "land to press on").not.toBeNull();
    const { x, y, slug } = start!;
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x - 120, y, { steps: 6 });
    await page.mouse.up();
    expect((await viewBox(page))[0]).toBeGreaterThan(before[0]);
    await expect(page.locator(".moss-country-panel")).toBeHidden();
    await page.waitForTimeout(100); // the click that ends a drag is swallowed for 50 ms; a later press is a new gesture
    const after = await landOnScreen();
    await page.mouse.click(after!.x, after!.y);
    await expect(page.locator(".moss-country-panel"), "a click on land still chooses it").toBeVisible();
  });

  test("closing the panel returns to the view from before choosing, unless the reader moved meanwhile", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await control(page, "zoom-in").click();
    const before = (await viewBox(page)).join(" ");
    await path(page, "brazil").click();
    expect((await viewBox(page)).join(" ")).not.toBe(before);
    await page.keyboard.press("Escape");
    expect((await viewBox(page)).join(" ")).toBe(before);

    await path(page, "brazil").click();
    await control(page, "zoom-in").click();
    const moved = (await viewBox(page)).join(" ");
    await page.locator(".moss-country-panel-close").click();
    expect((await viewBox(page)).join(" "), "a view the reader chose stays").toBe(moved);
  });

  test("reset shows the whole view and keeps the chosen country; choosing works at every zoom", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await path(page, "japan").click();
    await control(page, "reset").click();
    expect(await widthOf(page)).toBe(1000);
    await expect(page.locator(".moss-country-panel")).toContainText("Japan");
    await expect(path(page, "japan")).toHaveClass(/is-chosen/);
    for (let i = 0; i < 4; i++) await control(page, "zoom-in").click();
    const field = page.locator(".moss-country-search input");
    await field.fill("brazil");
    await field.press("Enter");
    await expect(page.locator(".moss-country-panel")).toContainText("Brazil");
    expect(await widthOf(page)).toBeLessThanOrEqual(1000);
    expect(await widthOf(page), "framed on the country, closer than one + step").toBeLessThan(1000 / 1.5);
  });
});

test.describe("zoom on a phone", () => {
  test.use({ hasTouch: true });
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 600 });
  });

  /** One finger, as a real touch stream, through the protocol (chromium only). */
  async function swipe(page: Page, from: [number, number], to: [number, number]) {
    const cdp = await page.context().newCDPSession(page);
    const at = (type: string, p: [number, number] | null) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: p ? [{ x: p[0], y: p[1] }] : [] });
    await at("touchStart", from);
    for (let i = 1; i <= 8; i++) await at("touchMove", [from[0] + ((to[0] - from[0]) * i) / 8, from[1] + ((to[1] - from[1]) * i) / 8]);
    await at("touchEnd", null);
  }

  test("− zooms out to the whole world width, letterboxed in the box, and + zooms back in", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    const opening = await viewBox(page);
    for (let i = 0; i < 6 && (await control(page, "zoom-out").isEnabled()); i++) await control(page, "zoom-out").click();
    const whole = await viewBox(page);
    expect(whole[2]).toBe(1000);
    expect(whole[3], "the box is squarer than the world, so the whole view has room above and below").toBeGreaterThan(WORLD_HEIGHT);
    await expect(control(page, "zoom-out")).toBeDisabled();
    await control(page, "zoom-in").click();
    expect((await viewBox(page))[2]).toBeCloseTo(1000 / 1.5, 0);
    expect(opening[2]).toBeLessThan(1000);
  });

  test("the stage hands vertical drags to the page", async ({ page }) => {
    await page.goto("/");
    expect(await stageOf(page).evaluate((el) => getComputedStyle(el).touchAction)).toBe("pan-y");
  });

  test("a sideways one-finger drag moves the map and a vertical one scrolls the page", async ({ page, browserName }) => {
    test.skip(browserName !== "chromium", "touch input through the protocol is chromium's");
    await page.goto("/");
    const stage = await box(stageOf(page));
    const y = stage.y + stage.height / 2;
    const before = await viewBox(page);
    await swipe(page, [300, y], [140, y]);
    const after = await viewBox(page);
    expect(after[0], "the map moved with the finger").toBeGreaterThan(before[0]);
    await expect(page.locator(".moss-country-panel")).toBeHidden();
    const scrolled = await page.evaluate(() => window.scrollY);
    await swipe(page, [200, stage.y + 250], [200, stage.y + 60]);
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(scrolled);
    expect((await viewBox(page))[0], "the vertical drag left the map where it was").toBeCloseTo(after[0], 0);
  });

  test("the controls leave while the bottom sheet is open and return when it closes", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await field.fill("brazil");
    await field.press("Enter");
    await expect(page.locator(".moss-country-panel")).toContainText("Brazil");
    await expect(control(page, "zoom-in"), "hidden, so also out of the tab order").toBeHidden();
    await page.locator(".moss-country-panel-close").click();
    await expect(control(page, "zoom-in")).toBeVisible();
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

const EMBEDDED_LIST = ".moss-country-map > .moss-country-list";
const OWN_TABLE = ".moss-country-own ~ .moss-table-scroll";
const sized = async (locator: Locator) => (await box(locator)).width > 100 && (await box(locator)).height > 20;

test.describe("a map shows its rows as colour; their words stay for assistive technology", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(WIDE);
  });

  test("an embedded map's list is clipped out of sight yet present, and shows when focus enters it", async ({ page }) => {
    await page.goto("/");
    const list = page.locator(EMBEDDED_LIST);
    const row = list.getByRole("link", { name: "Japan" });
    await expect(row, "still in the accessibility tree").toHaveCount(1);
    expect((await box(list)).width, "clipped to a point").toBeLessThanOrEqual(1);
    await page.locator(".moss-country-search input").focus();
    // The focusable map and its controls come first in the tab order, then the list.
    for (let i = 0; i < 5 && !(await list.locator("a:focus").count()); i++) await page.keyboard.press(TAB);
    await expect(list.locator("a").first()).toBeFocused();
    expect(await sized(list), "visible while focus is inside").toBe(true);
    await page.locator(".moss-country-search input").focus();
    expect((await box(list)).width, "hidden again when focus leaves").toBeLessThanOrEqual(1);
  });

  test("a row opened from the keyboard chooses the country", async ({ page }) => {
    await page.goto("/");
    const row = page.locator(EMBEDDED_LIST).getByRole("link", { name: "Japan" });
    await row.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(".moss-country-panel")).toContainText("Japan");
    await page.keyboard.press("Escape");
    await expect(row).toBeFocused();
  });

  test("the search's see-the-list link brings the list into view", async ({ page }) => {
    await page.goto("/");
    const field = page.locator(".moss-country-search input");
    await field.fill("zzzz");
    await page.locator(".moss-country-search-none a").click();
    expect(await sized(page.locator(EMBEDDED_LIST))).toBe(true);
  });

  test("on the map's own page the table is clipped yet present, and shows when focus enters it", async ({ page }) => {
    await page.goto("/risk-levels/");
    const table = page.locator(OWN_TABLE).first();
    await expect(page.getByRole("cell", { name: "Japan" })).toHaveCount(1);
    expect((await box(table)).width).toBeLessThanOrEqual(1);
    await table.focus();
    expect(await sized(table)).toBe(true);
  });

  test("on the map's own page the search's see-the-list link brings the table into view", async ({ page }) => {
    await page.goto("/risk-levels/");
    await page.locator(".moss-country-search input").fill("zzzz");
    await page.locator(".moss-country-search-none a").click();
    expect(await sized(page.locator(OWN_TABLE).first())).toBe(true);
  });

  test("a list placed apart with style:levels stays visible", async ({ page }) => {
    await page.goto("/apart/");
    expect(await sized(page.locator(".moss-country-list"))).toBe(true);
  });
});

test.describe("a map's words without script", () => {
  test.use({ javaScriptEnabled: false });
  test("the embedded list and the own page's table are visible", async ({ page }) => {
    await page.goto("/");
    expect(await sized(page.locator(EMBEDDED_LIST))).toBe(true);
    await page.goto("/risk-levels/");
    expect(await sized(page.locator(OWN_TABLE).first())).toBe(true);
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
