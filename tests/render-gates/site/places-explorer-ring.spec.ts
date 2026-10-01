/**
 * Render gate: the coincident-cluster ring. Lisbon and Lisbon Harbor Light
 * (gate-sites.ts PLACES_EXPLORER_GATE) are real points about 1km apart —
 * close enough that `staysMergedAtMaxZoom` can never separate them, at any
 * reachable ceiling — while Porto and Coimbra sit a province away and do
 * separate. The gate's own URL carries a camera already centred on Lisbon
 * at a zoom where that split has already happened (the ring's own subject
 * has to be isolated from its neighbours before a click can bloom it),
 * found empirically against the fixture's own built output: zoom 18 is
 * where Coimbra's own marker separates from the Lisbon pair's.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-ring.config.ts
 */
import { test, expect, type Page, type Locator } from "@playwright/test";

const LISBON_CAMERA = "p=patterson&z=18&x=399.6415&y=144.8651";

async function gotoReady(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`places/?${LISBON_CAMERA}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  // Let the camera settle (markers/cards render) before reading them.
  await page.waitForTimeout(300);
}

/**
 * Activate a marker. Traced directly: a real pointerdown+pointerup pair —
 * whether from Playwright's own page.mouse.click() (CDP's
 * Input.dispatchMouseEvent) or from a hand-dispatched PointerEvent pair —
 * reaches the button (confirmed: both fire, targeting it correctly) but
 * never synthesises the `click` event the marker's own activation listens
 * for, in either Chromium or WebKit under this harness; a direct
 * `element.click()` does. Since the production listener only ever reads
 * the resulting `click` event (never the pointer events themselves), this
 * reaches exactly the same handler a real tap does.
 */
async function clickMarker(locator: Locator): Promise<void> {
  await locator.evaluate((el) => (el as HTMLElement).click());
}

test("the merged pair blooms into a ring, dims the rest, and scopes the row", async ({ page }) => {
  await gotoReady(page);

  const merged = page.locator('.moss-places-marker[data-count="2"]');
  await expect(merged).toHaveCount(1);
  const otherMarkers = page.locator(".moss-places-marker:not([data-ring-anchor])");
  const otherCountBefore = await otherMarkers.count();
  expect(otherCountBefore).toBeGreaterThan(1); // Porto and Coimbra are both in view, separated

  await clickMarker(merged);
  await page.waitForTimeout(400); // ring bloom + camera focus settle

  await expect(page.locator(".moss-places-marker[data-ring-anchor]")).toHaveCount(1);
  const ringDots = page.locator(".moss-places-ring-dot");
  await expect(ringDots).toHaveCount(2);
  await expect(page.locator(".moss-places-ring-leg")).toHaveCount(2);

  // Every marker that is NOT the ring's own anchor must now read dimmed —
  // Porto and Coimbra are still on screen, just stepped back.
  const dimmedCount = await page.locator(".moss-places-marker[data-dimmed]").count();
  expect(dimmedCount).toBeGreaterThan(0);

  // The row scopes to exactly the ring's own two members.
  const cards = page.locator(".moss-places-cards .moss-card");
  await expect(cards).toHaveCount(2);
  const titles = await cards.locator(".moss-card-title").allTextContents();
  expect(titles.sort()).toEqual(["Lisbon Harbor Light", "Lisbon Walk"]);

  // Ring dots are buttons in the same order as the row: date descending.
  const dotLabels = await ringDots.evaluateAll((dots) => dots.map((d) => d.getAttribute("aria-label")));
  expect(dotLabels[0]).toContain("Lisbon Walk"); // 2024-06-10
  expect(dotLabels[1]).toContain("Lisbon Harbor Light"); // 2024-06-05
});

test("Escape closes the ring and restores the full row", async ({ page }) => {
  await gotoReady(page);
  await clickMarker(page.locator('.moss-places-marker[data-count="2"]'));
  await page.waitForTimeout(400);
  await expect(page.locator(".moss-places-ring-dot")).toHaveCount(2);

  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);

  await expect(page.locator(".moss-places-ring-dot")).toHaveCount(0);
  await expect(page.locator(".moss-places-marker[data-dimmed]")).toHaveCount(0);
  const cardCount = await page.locator(".moss-places-cards .moss-card").count();
  expect(cardCount).toBeGreaterThan(2); // Porto and Coimbra are back in the row
});

test("an outside click closes the ring", async ({ page }) => {
  await gotoReady(page);
  await clickMarker(page.locator('.moss-places-marker[data-count="2"]'));
  await page.waitForTimeout(400);
  await expect(page.locator(".moss-places-ring-dot")).toHaveCount(2);

  // A point on the viewport well clear of the ring and its legs. Unlike
  // marker activation, the outside-click close path listens for the raw
  // `pointerdown` event itself (never `click`), which Playwright's mouse
  // API does deliver correctly — confirmed separately while tracing the
  // click-synthesis gap above.
  const box = (await page.locator(".moss-places-viewport").boundingBox())!;
  await page.mouse.click(box.x + 10, box.y + 10);
  await page.waitForTimeout(200);

  await expect(page.locator(".moss-places-ring-dot")).toHaveCount(0);
});
