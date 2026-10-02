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
 * Every interaction below is real input (`locator.click()`), not the
 * `element.click()` workaround this file used to carry: that workaround
 * was masking a product bug rather than dodging a harness limitation.
 * `gestures.ts`'s `pointerdown` ignores a `button`/`a` target so a marker
 * can handle its own click, but its `pointerup` path (`endPointer`) used
 * to settle unconditionally regardless — ending a gesture that never
 * started, which re-rendered the marker layer between `pointerdown` and
 * `mouseup` and left the browser's click synthesis with no element to
 * fire `click` on. Fixed at the source (`gestures.ts`'s `endPointer`,
 * `markers.ts`'s keyed button reuse) rather than here.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-ring.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

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

test("the merged pair blooms into a ring, dims the rest, and scopes the row", async ({ page }) => {
  await gotoReady(page);

  const merged = page.locator('.moss-places-marker[data-count="2"]');
  await expect(merged).toHaveCount(1);
  const otherMarkers = page.locator(".moss-places-marker:not([data-ring-anchor])");
  const otherCountBefore = await otherMarkers.count();
  expect(otherCountBefore).toBeGreaterThan(1); // Porto and Coimbra are both in view, separated

  await merged.click();
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
  await page.locator('.moss-places-marker[data-count="2"]').click();
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
  await page.locator('.moss-places-marker[data-count="2"]').click();
  await page.waitForTimeout(400);
  await expect(page.locator(".moss-places-ring-dot")).toHaveCount(2);

  // A point on the viewport well clear of the ring and its legs.
  const box = (await page.locator(".moss-places-viewport").boundingBox())!;
  await page.mouse.click(box.x + 10, box.y + 10);
  await page.waitForTimeout(200);

  await expect(page.locator(".moss-places-ring-dot")).toHaveCount(0);
});

test("a real click on a ring dot selects that work", async ({ page, browserName }) => {
  await gotoReady(page);
  await page.locator('.moss-places-marker[data-count="2"]').click();
  await page.waitForTimeout(400);
  const ringDots = page.locator(".moss-places-ring-dot");
  await expect(ringDots).toHaveCount(2);

  const firstDot = ringDots.first();
  const firstDotLabel = await firstDot.getAttribute("aria-label");
  // `locator.click()`'s own pre-click actionability wait ("visible, enabled
  // and stable") hangs here in WebKit specifically, past this test's
  // timeout, on the FIRST ring dot after a real click opened the ring —
  // diagnosed directly, not assumed: `isVisible()`/`isEnabled()` both read
  // true, `boundingBox()` is bit-for-bit identical 50ms apart (and across
  // 40 real animation frames, polled separately), and
  // `document.elementFromPoint` at the box's own centre resolves to this
  // SAME button — every condition `locator.click()` waits on already
  // holds, checked with Playwright's own APIs, not a guess. A plain
  // `page.mouse.click()` at that same point (still real OS-level input,
  // only skipping Playwright's own pre-check) lands correctly first try.
  // This is a WebKit/Playwright actionability-polling limitation, not the
  // product re-rendering under the pointer — that mechanism (`gestures.ts`'s
  // `endPointer`, `markers.ts`'s keyed reuse) is what the single-marker and
  // separable-cluster tests above and below already prove fixed, in both
  // engines, with ordinary `locator.click()`.
  if (browserName === "webkit") {
    const box = (await firstDot.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  } else {
    await firstDot.click();
  }
  await page.waitForTimeout(200);

  // Selecting a work expands its own card and carries the selection in the
  // URL — both are the production `selectWork` callback's own visible
  // effects, neither reachable without a real `click` event.
  const selectedCard = page.locator(".moss-card[aria-current='true']");
  await expect(selectedCard).toHaveCount(1);
  const selectedTitle = await selectedCard.locator(".moss-card-title").textContent();
  expect(firstDotLabel).toContain(selectedTitle);
  expect(page.url()).toMatch(/[?&]article=/);
});

test("a real click on a cluster that can separate changes the camera", async ({ page }) => {
  await gotoReady(page);
  // Porto and Coimbra are a province apart — able to separate by zooming,
  // unlike the Lisbon pair — so clicking their shared cluster (if any is
  // merged at this zoom) must zoom to fit them (case 1) rather than bloom a
  // ring (case 2/3). At this camera they are already separate single
  // markers; zoom out first so a cluster forms, then click it.
  await page.keyboard.press("-");
  await page.keyboard.press("-");
  await page.keyboard.press("-");
  await page.waitForTimeout(300);

  const cameraBefore = await page.evaluate(() => (document.querySelector(".moss-places-world") as HTMLElement).style.transform);
  const cluster = page.locator(".moss-places-marker[data-count]").first();
  const countBefore = await page.locator(".moss-places-marker").count();
  await cluster.click();
  await page.waitForTimeout(500);

  const cameraAfter = await page.evaluate(() => (document.querySelector(".moss-places-world") as HTMLElement).style.transform);
  expect(cameraAfter, "a separable cluster's own click must move the camera (case 1: zoom-to-fit)").not.toBe(cameraBefore);
  // Separating should reveal more distinct markers than the merged state had.
  const countAfter = await page.locator(".moss-places-marker").count();
  expect(countAfter).toBeGreaterThanOrEqual(countBefore);
});

test("a real click on a single (unclustered) marker selects its work", async ({ page }) => {
  await gotoReady(page);
  const single = page.locator(".moss-places-marker:not([data-count])").first();
  const label = await single.getAttribute("aria-label");
  await single.click();
  await page.waitForTimeout(200);

  const selectedCard = page.locator(".moss-card[aria-current='true']");
  await expect(selectedCard).toHaveCount(1);
  const selectedTitle = await selectedCard.locator(".moss-card-title").textContent();
  expect(label).toContain(selectedTitle);
  expect(page.url()).toMatch(/[?&]article=/);
});
