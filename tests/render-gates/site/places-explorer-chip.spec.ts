/**
 * Render gate: the breadcrumb scope chip — the map's one hierarchy control,
 * top left, in the controls' own glass material.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 * Portugal/Porto/Coimbra are its one parent/child pair (Porto and Coimbra
 * both carry `parent = "Portugal"` in the fixture's `.moss/places.toml`),
 * giving the chip a nested place to dig into; every other place in the
 * fixture is a top-level sibling with exactly one work.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-chip.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function gotoReady(page: Page, search = ""): Promise<void> {
  await page.goto(`places/${search}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(300); // camera/markers/chip settle
}

async function setTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.evaluate((t) => localStorage.setItem("moss-theme", t), theme);
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(300);
}

function trigger(page: Page) {
  return page.locator(".moss-places-chip-crumb[data-terminal]").first();
}

function menuItems(page: Page) {
  return page.locator(".moss-places-chip-menu-item");
}

type Box = { x: number; y: number; width: number; height: number };

function rectsOverlap(a: Box, b: Box): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

test.describe("desktop, light", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("the root chip shows 'All articles' with a chevron, and opening it lists every top-level place by count then name", async ({ page }) => {
    await gotoReady(page);
    const root = trigger(page);
    await expect(root).toContainText("All articles");
    await expect(root).toHaveAttribute("aria-haspopup", "menu");
    await expect(root.locator(".moss-places-chip-chevron svg")).toHaveCount(1);
    await expect(page.locator(".moss-places-chip-menu")).toHaveCount(0);

    await root.click();
    const menu = page.locator('.moss-places-chip-menu[role="menu"]');
    await expect(menu).toBeVisible();
    const labels = await menuItems(page).allTextContents();
    // Portugal rolls up its own work plus Porto + Coimbra (count 3) and
    // sorts first; every other place has exactly one work, so the rest is a
    // plain name sort.
    expect(labels).toEqual([
      "Portugal(3)",
      "Arequipa(1)",
      "Cusco(1)",
      "Faro(1)",
      "Iquitos(1)",
      "Kyoto(1)",
      "Lima(1)",
      "Lisbon(1)",
      "Lisbon Harbor(1)",
      "Nara(1)",
      "Osaka(1)",
      "Tokyo(1)",
    ]);
  });

  test("ArrowDown moves focus; Escape closes the menu and refocuses the crumb", async ({ page }) => {
    await gotoReady(page);
    const root = trigger(page);
    await root.click();
    const items = menuItems(page);
    await expect(items.first()).toBeFocused();

    await page.keyboard.press("ArrowDown");
    await expect(items.nth(1)).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(page.locator(".moss-places-chip-menu")).toHaveCount(0);
    await expect(root).toBeFocused();
  });

  test("choosing a menu item narrows the scope and updates the URL without a navigation", async ({ page }) => {
    await gotoReady(page);
    await page.evaluate(() => {
      (window as unknown as { __gateSentinel: number }).__gateSentinel = 1;
    });

    await trigger(page).click();
    await page.getByRole("menuitem", { name: "Portugal (3)" }).click();

    await expect(page).toHaveURL(/place=places%2Fportugal/);
    const trail = page.locator(".moss-places-chip-trail");
    await expect(trail).toContainText("All articles");
    await expect(trail).toContainText("Portugal");

    // `history.replaceState`, never a real navigation — the sentinel a fresh
    // document load would have wiped survives only if the page never reloaded.
    expect(await page.evaluate(() => (window as unknown as { __gateSentinel: number }).__gateSentinel)).toBe(1);
  });

  test("the nested place's own crumb trail is three deep, and the terminal crumb's own menu lists its children", async ({ page }) => {
    await gotoReady(page, "?place=places/porto");
    const trail = page.locator(".moss-places-chip-trail");
    await expect(trail).toContainText("All articles");
    await expect(trail).toContainText("Portugal");
    await expect(trail).toContainText("Porto");
    // Porto is a leaf — no children, no chevron: its terminal crumb is a
    // plain span, never a menu-opening button.
    await expect(page.locator(".moss-places-chip-crumb[aria-haspopup]")).toHaveCount(0);

    await gotoReady(page, "?place=places/portugal");
    await trigger(page).click();
    const labels = await menuItems(page).allTextContents();
    expect(labels).toEqual(["Coimbra(1)", "Porto(1)"]);
  });

  test("hovering a menu item highlights its own markers and dims the rest; closing clears it", async ({ page }) => {
    await gotoReady(page);
    await trigger(page).click();
    // The menu auto-focuses its first item for keyboard use, and focusing
    // an item highlights it the same way hovering does — so dimming is
    // already active against the FIRST item (Portugal) before any hover.
    const lisbonItem = page.getByRole("menuitem", { name: "Lisbon (1)" });
    await lisbonItem.hover();
    const total = await page.locator(".moss-places-marker").count();
    const dimmed = await page.locator(".moss-places-marker[data-dimmed]").count();
    expect(dimmed).toBeGreaterThan(0);
    expect(dimmed).toBeLessThan(total);

    await page.keyboard.press("Escape");
    await expect(page.locator(".moss-places-marker[data-dimmed]")).toHaveCount(0);
  });

  test("every crumb and separator shares one vertical centre within 0.5px", async ({ page }) => {
    await gotoReady(page, "?place=places/porto");
    // `:visible` excludes the collapsed-at-phone-width ellipsis's own
    // separator, which sits in the DOM at this desktop width but is
    // `display: none` (a bounding rect of all zeros would otherwise read as
    // a wildly different "centre").
    const centres = await page.$$eval(".moss-places-chip-crumb:visible, .moss-places-chip-sep:visible", (nodes) =>
      nodes.map((node) => {
        const box = node.getBoundingClientRect();
        return box.top + box.height / 2;
      }),
    );
    expect(centres.length).toBeGreaterThan(2);
    for (const centre of centres) {
      expect(Math.abs(centre - centres[0])).toBeLessThanOrEqual(0.5);
    }
  });

  test("the chip never overlaps the zoom controls", async ({ page }) => {
    await gotoReady(page, "?place=places/porto");
    const chipBox = (await page.locator(".moss-places-chip").boundingBox())!;
    const controlsBox = (await page.locator(".moss-places-controls").boundingBox())!;
    expect(rectsOverlap(chipBox, controlsBox)).toBe(false);
  });

  test("no map label intersects the chip", async ({ page }) => {
    await gotoReady(page);
    const chipBox = (await page.locator(".moss-places-chip").boundingBox())!;
    const labelBoxes = await page.locator(".moss-places-label").evaluateAll((els) =>
      els.map((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height };
      }),
    );
    expect(labelBoxes.length).toBeGreaterThan(0);
    for (const labelBox of labelBoxes) {
      expect(rectsOverlap(chipBox, labelBox)).toBe(false);
    }
  });

  // The menu's own glass material is translucent (places-explorer.css), so
  // a label hidden behind the chip's own trail but not behind the wider
  // dig-down menu would still read through it — this is the real-browser
  // counterpart to map.test.ts's own mounted check, covering the actual
  // CSS and real label layout neither of those exercises.
  test("no map label intersects the open dig-down menu", async ({ page }) => {
    await gotoReady(page);
    await trigger(page).click();
    const menu = page.locator('.moss-places-chip-menu[role="menu"]');
    await expect(menu).toBeVisible();
    const menuBox = (await menu.boundingBox())!;
    const labelBoxes = await page.locator(".moss-places-label").evaluateAll((els) =>
      els.map((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height };
      }),
    );
    expect(labelBoxes.length).toBeGreaterThan(0);
    for (const labelBox of labelBoxes) {
      expect(rectsOverlap(menuBox, labelBox)).toBe(false);
    }
  });
});

test.describe("phone, 390px", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("the middle crumb collapses to '…', and the chip still clears the zoom controls", async ({ page }) => {
    await gotoReady(page, "?place=places/porto");
    const trail = page.locator(".moss-places-chip-trail");
    await expect(trail).toContainText("All articles");
    await expect(trail).toContainText("Porto");

    await expect(page.locator(".moss-places-chip-collapsible")).not.toBeVisible();
    await expect(page.locator(".moss-places-chip-ellipsis")).toBeVisible();
    // The collapsed middle crumb ("Portugal") is hidden, not merely
    // scrolled off — this is a CSS display swap, not a clip.
    await expect(page.locator(".moss-places-chip-collapsible")).toContainText("Portugal");

    const chipBox = (await page.locator(".moss-places-chip").boundingBox())!;
    const controlsBox = (await page.locator(".moss-places-controls").boundingBox())!;
    expect(chipBox.x + chipBox.width).toBeLessThanOrEqual(controlsBox.x);
  });

  test("no map label intersects the chip", async ({ page }) => {
    await gotoReady(page);
    const chipBox = (await page.locator(".moss-places-chip").boundingBox())!;
    const labelBoxes = await page.locator(".moss-places-label").evaluateAll((els) =>
      els.map((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height };
      }),
    );
    expect(labelBoxes.length).toBeGreaterThan(0);
    for (const labelBox of labelBoxes) {
      expect(rectsOverlap(chipBox, labelBox)).toBe(false);
    }
  });
});

test.describe("dark theme", () => {
  test("the chip and its menu render in the dark glass material", async ({ page }) => {
    await gotoReady(page);
    await setTheme(page, "dark");
    await expect(page.locator(".moss-places-chip")).toBeVisible();
    await trigger(page).click();
    await expect(page.locator('.moss-places-chip-menu[role="menu"]')).toBeVisible();
    await expect(menuItems(page)).toHaveCount(12);
  });
});
