/**
 * Render gate: a realistic places root. Four assertions, one per fixture
 * shape PLACES_EXPLORER_GATE's short English fixture cannot exercise — see
 * PLACES_EXPLORER_REAL_GATE's own doc (tests/e2e/helpers/gate-sites.ts).
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-real.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function gotoReady(page: Page, search = "", viewport: { width: number; height: number } = { width: 1280, height: 800 }): Promise<void> {
  await page.setViewportSize(viewport);
  await page.goto(`places/${search}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(300);
}

type Box = { x: number; y: number; width: number; height: number };

function overlaps(a: Box, b: Box): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

/**
 * A real click, routed around a WebKit-only harness limitation: a SECOND
 * `locator.click()` on an element whose attributes an earlier click in the
 * same test just changed hangs `locator.click()`'s own pre-click
 * actionability wait in WebKit specifically — `places-explorer-ring.spec.ts`
 * and `places-explorer-cards.spec.ts` document and work around the same
 * class of repeated-interaction hang this way. Still real input
 * (`page.mouse.click`), never `element.click()`.
 */
async function realClick(page: Page, locator: ReturnType<Page["locator"]>, browserName: string): Promise<void> {
  if (browserName === "webkit") {
    const box = (await locator.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  } else {
    await locator.click();
  }
}

// Defect 1: the explorer root's heading is visually hidden, map leads, even
// though this fixture's root is synthetic (no "places/index.md").
test("the synthetic root's own heading is visually hidden, with the map directly under the site header", async ({ page }) => {
  await gotoReady(page);
  const main = page.locator("#main-content");
  const h1 = main.locator("h1").first();
  await expect(h1).toHaveCount(1);
  await expect(h1).toHaveClass(/visually-hidden/);

  const figureTop = await page.locator("figure[data-moss-places-explorer]").evaluate((el) => el.getBoundingClientRect().top);
  const headerBottom = await page.locator("header").evaluate((el) => el.getBoundingClientRect().bottom);
  expect(Math.abs(figureTop - headerBottom)).toBeLessThan(1);
});

// Defect 5 (same fixture, same page): the header names the section since
// the heading above is hidden.
test("the header breadcrumb names the section on the synthetic root", async ({ page }) => {
  await gotoReady(page);
  const crumb = page.locator('header [aria-current="page"]').filter({ hasText: "地點" });
  await expect(crumb).toHaveCount(1);
});

// Fix: `.breadcrumb-segment:last-child` used to assume the trail's actual
// last CHILD was always the last ANCESTOR segment — true for an ordinary
// nested page, but false here, where the forced-visible current crumb is a
// trailing span after it. Both now target the ancestor explicitly
// (`a.breadcrumb-segment:last-of-type`), so the legibility floor still
// lands on the ancestor on the root AND stays unchanged on an ordinary
// nested page.
test("the breadcrumb legibility floor targets the last ancestor segment, on the explorer root and on an ordinary nested page alike", async ({ page }) => {
  await gotoReady(page);
  const current = page.locator("header .breadcrumb-current");
  await expect(current).toHaveCount(1);
  const currentShrink = await current.evaluate((el) => getComputedStyle(el).flexShrink);
  expect(currentShrink, "the forced-visible current crumb must not get the ancestor's legibility floor").not.toBe("1");

  await page.goto("places/kyoto/", { waitUntil: "domcontentloaded" });
  const ancestors = page.locator("header a.breadcrumb-segment");
  const count = await ancestors.count();
  expect(count, "a nested page keeps at least one ancestor crumb").toBeGreaterThan(0);
  const lastAncestor = ancestors.nth(count - 1);
  expect(await lastAncestor.evaluate((el) => getComputedStyle(el).flexShrink)).toBe("1");
  expect(await lastAncestor.evaluate((el) => getComputedStyle(el).minInlineSize)).not.toBe("0px");
});

// Fix: the figure used to be a full 100svh regardless of the header's own
// rendered height, pushing its bottom (and the card row floating at it)
// below the viewport at rest. The figure now fills the viewport BELOW the
// header exactly, at every width: figure height = viewport height minus
// the figure's own top offset, floored at the existing 480px minimum.
for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
  test(`the card row's bottom is within the viewport at rest, and the figure's bottom matches the viewport's (${viewport.width}x${viewport.height})`, async ({ page }) => {
    await gotoReady(page, "", viewport);
    const figureBox = (await page.locator("figure[data-moss-places-explorer]").boundingBox())!;
    expect(Math.abs(figureBox.y + figureBox.height - viewport.height)).toBeLessThanOrEqual(1);

    const cardsBox = (await page.locator(".moss-places-cards").boundingBox())!;
    expect(cardsBox.y + cardsBox.height).toBeLessThanOrEqual(viewport.height + 1);
  });
}

// Defect 2: compact collapsed cards, no raw markdown, markers clear of the row.
test("collapsed cards stay compact despite long multi-link bylines, with no raw markdown and no marker under the row", async ({ page }) => {
  await gotoReady(page);

  const cardsBox = (await page.locator(".moss-places-cards").boundingBox())!;
  const mapBox = (await page.locator(".moss-places-viewport").boundingBox())!;
  expect(cardsBox.height).toBeLessThanOrEqual(mapBox.height * 0.26);

  const cardTexts = await page.locator(".moss-places-cards .moss-card").allTextContents();
  expect(cardTexts.length).toBeGreaterThan(0);
  for (const text of cardTexts) {
    expect(text, `a card must never show raw markdown link syntax: ${text}`).not.toContain("](");
  }

  const markerBoxes = await page.locator(".moss-places-marker").evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    }),
  );
  expect(markerBoxes.length).toBeGreaterThan(0);
  for (const center of markerBoxes) {
    const inRow = center.x >= cardsBox.x && center.x <= cardsBox.x + cardsBox.width
      && center.y >= cardsBox.y && center.y <= cardsBox.y + cardsBox.height;
    expect(inRow, `a marker centre sits under the card row at rest: ${JSON.stringify(center)} vs ${JSON.stringify(cardsBox)}`).toBe(false);
  }
});

// Fix: a raster cover resolves to a URL the build actually serves, never a
// guessed `.webp` sibling — on the real site this landed on 0 of 25 raster
// covers (the build never serves a bare `<img src>` at that guessed path;
// it only exists inside a `<picture>` moss-core's own synthesizer builds,
// which this JSON-driven card can't reproduce). Every other covered fixture
// in this suite uses an SVG cover, which never took the broken branch.
test("every cover in the places JSON resolves to a file the build served, and a raster card's cover image loads", async ({ page }) => {
  await gotoReady(page);
  const placesUrl = await page.locator("figure[data-moss-places-explorer]").getAttribute("data-places");
  const data = await (await page.request.get(placesUrl!)).json();
  const covers: string[] = data.works
    .map((w: { cover?: string }) => w.cover)
    .filter((c: string | undefined): c is string => Boolean(c));
  expect(covers.length).toBeGreaterThan(0);
  for (const cover of covers) {
    const res = await page.request.get(cover);
    expect(res.ok(), `${cover} must resolve to a file the build served`).toBe(true);
  }

  const coverImg = page.locator(".moss-places-cards .moss-card-cover img").first();
  await expect(coverImg).toHaveCount(1);
  const naturalWidth = await coverImg.evaluate((img) => (img as HTMLImageElement).naturalWidth);
  expect(naturalWidth, "the raster cover must actually decode, not 404").toBeGreaterThan(0);
});

// Fix: the card title is clamped to two lines in BOTH the collapsed and
// expanded states (same reused element), so a long title can't be read in
// full either way. Collapsed keeps the clamp — with the full title moved
// to the select button's own accessible name — and expanding now unclamps
// it so the title reads in full in the detail state.
test("a long CJK title stays clamped while collapsed, with the full title in the select's accessible name, and reads in full once expanded", async ({ page, browserName }) => {
  await gotoReady(page);
  const card = page.locator(".moss-places-cards .moss-card").first();
  const select = card.locator(".moss-places-card-select");
  const title = card.locator(".moss-card-title");
  const fullTitle = (await title.textContent())?.trim() ?? "";
  expect(fullTitle.length).toBeGreaterThan(10);

  const collapsedOverflow = await title.evaluate((el) => el.scrollHeight > el.clientHeight + 1);
  expect(collapsedOverflow, "the fixture's long title must actually overflow a two-line clamp for this gate to mean anything").toBe(true);
  await expect(select).toHaveAttribute("aria-label", fullTitle);

  await realClick(page, select, browserName);
  await page.waitForTimeout(300);
  const expandedOverflow = await title.evaluate((el) => el.scrollHeight > el.clientHeight + 1);
  expect(expandedOverflow, "the expanded card must unclamp the title instead of still cutting it off").toBe(false);
});

// Defect 3 / fix 2: Japan has no gazetteer row at all (a grouping node —
// `fold_ancestors` emits it from Kansai's own `parent =` reference alone)
// and Kansai has one but no work of its own. Both are reachable from the
// root menu with a rolled-up count, both dig down, and choosing either
// still fits the camera to its descendants' real points even though
// neither node has coordinates of its own.
test("the root menu reaches a coordinate-less grouping country and a work-less middle region, both with a rolled-up count, and digs down to the leaf city", async ({ page, browserName }) => {
  await gotoReady(page);
  const trigger = page.locator(".moss-places-chip-crumb[data-terminal]").first();
  await realClick(page, trigger, browserName);
  const menu = page.locator('.moss-places-chip-menu[role="menu"]');
  await expect(menu).toBeVisible();

  const japan = page.locator(".moss-places-chip-menu-item", { hasText: "Japan" });
  await expect(japan).toHaveCount(1);
  await expect(japan).toContainText("(1)"); // Kyoto's own work, rolled all the way up
  await realClick(page, japan, browserName);
  await page.waitForTimeout(300);

  // Choosing Japan — a grouping node with no coordinates of its own —
  // still fits the camera to Kyoto's real point underneath it: a marker is
  // on screen, not a blank map centred on nothing.
  await expect(page.locator(".moss-places-marker")).toHaveCount(1);

  // Digging into Japan reveals Kansai (the work-less middle place) in the
  // now-current menu, with the SAME rolled-up count — not Kyoto directly.
  const trigger2 = page.locator(".moss-places-chip-crumb[data-terminal]").first();
  await realClick(page, trigger2, browserName);
  const kansai = page.locator(".moss-places-chip-menu-item", { hasText: "Kansai" });
  await expect(kansai).toHaveCount(1);
  await expect(kansai).toContainText("(1)");
  await realClick(page, kansai, browserName);
  await page.waitForTimeout(300);
  await expect(page.locator(".moss-places-marker")).toHaveCount(1);

  // Digging into Kansai reveals the leaf city, Kyoto.
  const trigger3 = page.locator(".moss-places-chip-crumb[data-terminal]").first();
  await realClick(page, trigger3, browserName);
  const kyoto = page.locator(".moss-places-chip-menu-item", { hasText: "Kyoto" });
  await expect(kyoto).toHaveCount(1);
});

// Defect 4: the label layer draws real labels on this zh-Hant page, in
// whichever engine is running — the gate that would have failed in WebKit
// before the fix this fixture exists to pin.
test("place-name labels render at rest on this zh-Hant page", async ({ page, browserName }) => {
  await gotoReady(page);
  const count = await page.locator(".moss-places-label:not([hidden])").count();
  expect(count, `${browserName}: expected at least one visible zh-Hant label at rest`).toBeGreaterThan(0);
});
