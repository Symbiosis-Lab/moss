/**
 * Render gate: the places explorer's label layer — major cities, mountain
 * ranges, peaks and rivers (`labels.ts`), placed greedily by priority,
 * never overlapping another label, a marker, the controls or the card row,
 * within the area budget, respecting the own-place rule for a city label
 * beside a marker, and never exposed to assistive tech.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_LABELS_GATE), built by the playwright config at parse
 * time. Its two works sit at REAL Natural Earth populated-place coordinates
 * (Tokyo, Osaka) under their own gazetteer names — one matching its city
 * (the positive own-place case), one not (the negative case) — so this gate
 * exercises the rule against the real embedded pack rather than a stub.
 * Both run against the DEFAULT world-cover camera, with no custom pan/zoom
 * needed: each work's own coordinate is EXACTLY its real city's own pack
 * coordinate, so the two project to the identical screen point at any
 * camera whatsoever — the own-place rule's own "covering" check (within a
 * small, zoom-independent pixel radius of that shared point) is exercised
 * the same way regardless of what else is in view. Every other assertion
 * here (budget, collision, re-placement, aria-hidden) also runs against
 * that same default camera, which already shows hundreds of the pack's own
 * real labels with no further fixture data needed.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-labels.config.ts
 */
import { test, expect, type Page, type Locator } from "@playwright/test";

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

async function waitReady(page: Page): Promise<void> {
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(300); // camera settle -> markers + labels render
}

async function gotoReady(page: Page, search = ""): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`places/${search}`, { waitUntil: "domcontentloaded" });
  await waitReady(page);
}

async function boxesOf(locator: Locator): Promise<Box[]> {
  const count = await locator.count();
  const boxes: Box[] = [];
  for (let i = 0; i < count; i++) {
    const box = await locator.nth(i).boundingBox();
    if (box) boxes.push(box);
  }
  return boxes;
}

function overlaps(a: Box, b: Box): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

function assertNoneOverlap(boxes: Box[], context: string): void {
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      expect(overlaps(boxes[i], boxes[j]), `${context}: box ${i} overlaps box ${j} — ${JSON.stringify(boxes[i])} / ${JSON.stringify(boxes[j])}`).toBe(false);
    }
  }
}

function assertNoCrossOverlap(as: Box[], bs: Box[], context: string): void {
  for (let i = 0; i < as.length; i++) {
    for (let j = 0; j < bs.length; j++) {
      expect(overlaps(as[i], bs[j]), `${context}: label ${i} overlaps ${j} — ${JSON.stringify(as[i])} / ${JSON.stringify(bs[j])}`).toBe(false);
    }
  }
}

const VISIBLE_LABELS = ".moss-places-label:not([hidden])";

for (const theme of ["light", "dark"] as const) {
  test(`at rest, labels render, never overlap each other, a marker, the controls or the card row, and stay aria-hidden — ${theme}`, async ({ page }) => {
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    await page.evaluate((t) => localStorage.setItem("moss-theme", t), theme);
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.reload({ waitUntil: "domcontentloaded" });
    await waitReady(page);

    await expect(page.locator(".moss-places-labels")).toHaveAttribute("aria-hidden", "true");

    const labels = page.locator(VISIBLE_LABELS);
    const labelCount = await labels.count();
    expect(labelCount, "expected at least one real-pack label at the world cover camera").toBeGreaterThan(0);
    for (let i = 0; i < labelCount; i++) {
      await expect(labels.nth(i)).toHaveAttribute("aria-hidden", "true");
      const color = await labels.nth(i).evaluate((el) => getComputedStyle(el).color);
      expect(color, `label ${i} resolved to a transparent/initial colour in ${theme}`).not.toBe("rgba(0, 0, 0, 0)");
    }

    const labelBoxes = await boxesOf(labels);
    const markerBoxes = await boxesOf(page.locator(".moss-places-marker"));
    const controlsBox = await page.locator(".moss-places-controls").boundingBox();
    // The cards themselves: `.moss-places-cards` is a taller, click-through band an opened card grows up into, so its own box covers most of the map.
    const cardBoxes = await boxesOf(page.locator(".moss-places-cards .moss-card"));
    expect(cardBoxes.length, "expected cards in the row at the world cover camera").toBeGreaterThan(0);
    const reserved = [...markerBoxes, ...(controlsBox ? [controlsBox] : []), ...cardBoxes];

    assertNoneOverlap(labelBoxes, `${theme}: labels vs labels`);
    assertNoCrossOverlap(labelBoxes, reserved, `${theme}: labels vs markers/controls/cards`);
  });
}

test("the visible label count respects the area budget and its minimum", async ({ page }) => {
  await gotoReady(page);
  const viewportBox = (await page.locator(".moss-places-viewport").boundingBox())!;
  const AREA_BUDGET_PX2 = 45000;
  const MIN_COUNT = 5;
  const budget = Math.max(MIN_COUNT, Math.round((viewportBox.width * viewportBox.height) / AREA_BUDGET_PX2));

  const count = await page.locator(VISIBLE_LABELS).count();
  expect(count).toBeGreaterThanOrEqual(MIN_COUNT);
  expect(count).toBeLessThanOrEqual(budget);
});

test("after a zoom, the label set is re-placed and stays collision-free", async ({ page }) => {
  await gotoReady(page);
  const before = await boxesOf(page.locator(VISIBLE_LABELS));

  await page.locator('[data-control="zoom-in"]').click();
  await page.waitForTimeout(400);
  await page.locator('[data-control="zoom-in"]').click();
  await page.waitForTimeout(400);

  const after = await boxesOf(page.locator(VISIBLE_LABELS));
  expect(after.length, "expected labels after zooming in too").toBeGreaterThan(0);
  // Proves the set actually got RE-placed for the new camera, not left at
  // its pre-zoom screen positions (which a stale re-render would produce).
  expect(JSON.stringify(after)).not.toBe(JSON.stringify(before));

  const markerBoxes = await boxesOf(page.locator(".moss-places-marker"));
  assertNoneOverlap(after, "post-zoom: labels vs labels");
  assertNoCrossOverlap(after, markerBoxes, "post-zoom: labels vs markers");
});

/** The Patterson (2014) cylindrical projection `projection.ts` implements, reimplemented here from the published polynomial rather than imported — the same cross-check `places-explorer-camera.spec.ts` applies, so a drift between the build's runtime and this gate fails loudly instead of cancelling out. Used only to zoom tightly enough that the own-place rule — not ordinary rank/budget competition among the hundreds of OTHER real cities a world-cover camera also shows — is what decides whether these two specific labels appear. */
function pattersonProject(latitude: number, longitude: number): { x: number; y: number } {
  const K1 = 1.0148;
  const K2 = 0.23185;
  const K3 = -0.14499;
  const K4 = 0.02406;
  const pattersonY = (phi: number): number => {
    const phi2 = phi * phi;
    const phi4 = phi2 * phi2;
    return phi * (K1 + phi4 * (K2 + phi2 * (K3 + K4 * phi2)));
  };
  const HEIGHT = 480;
  const yAtPole = pattersonY(Math.PI / 2);
  const scale = HEIGHT / 2 / yAtPole;
  const width = (HEIGHT * Math.PI) / yAtPole;
  return {
    x: width / 2 + scale * ((longitude * Math.PI) / 180),
    y: HEIGHT / 2 - scale * pattersonY((latitude * Math.PI) / 180),
  };
}

test("a city label beside a marker that IS one of the marker's own places is shown (Tokyo Shibuya)", async ({ page }) => {
  const target = pattersonProject(35.687, 139.749); // the real Natural Earth "Tokyo" point — identical to Tokyo Shibuya's own declared coordinate
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`places/?p=patterson&z=18&x=${target.x}&y=${target.y}`, { waitUntil: "domcontentloaded" });
  await waitReady(page);

  const tokyoLabel = page.locator(`${VISIBLE_LABELS}[data-kind="city"]`, { hasText: "Tokyo" });
  await expect(tokyoLabel).toHaveCount(1);

  // Tokyo Shibuya's own declared coordinate IS the real Natural Earth
  // "Tokyo" point, so its marker sits exactly where the label's own anchor
  // does — whichever marker is closest to the label's box is the one the
  // own-place rule had to accept.
  const labelBox = (await tokyoLabel.boundingBox())!;
  const markerBoxes = await boxesOf(page.locator(".moss-places-marker"));
  const labelCenter = { x: labelBox.x + labelBox.width / 2, y: labelBox.y + labelBox.height / 2 };
  const nearest = markerBoxes.reduce((best, box) => {
    const center = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    const distance = Math.hypot(center.x - labelCenter.x, center.y - labelCenter.y);
    return distance < best.distance ? { box, distance } : best;
  }, { box: markerBoxes[0], distance: Infinity });
  expect(nearest.distance, "no marker is anywhere near the Tokyo label — the own-place rule has nothing to test").toBeLessThan(80);
  expect(overlaps(labelBox, nearest.box), "the label text itself must sit BESIDE the marker, not on top of it").toBe(false);
});

test("a city label merely covered by a marker that is NOT its own place is dropped, never shown beside it (Osaka / Namba District)", async ({ page }) => {
  const target = pattersonProject(34.752, 135.458); // the real Natural Earth "Osaka" point — identical to Namba District's own declared coordinate
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`places/?p=patterson&z=18&x=${target.x}&y=${target.y}`, { waitUntil: "domcontentloaded" });
  await waitReady(page);

  // Namba District's own declared coordinate IS the real Natural Earth
  // "Osaka" point — the case the own-place rule exists for: a marker
  // sitting right on a real city's label point whose own place is NOT that
  // city must never wear its name. At this zoom "Osaka" (a real, quite
  // major pack city) is well clear of the area-budget's own competition
  // from everywhere else on Earth, so its absence here is the own-place
  // rule's own doing, not budget pressure.
  const osakaLabel = page.locator(`${VISIBLE_LABELS}[data-kind="city"]`, { hasText: "Osaka" });
  await expect(osakaLabel).toHaveCount(0);
});
