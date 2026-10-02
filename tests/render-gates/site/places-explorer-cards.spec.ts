/**
 * Render gate: the works row's selection (expand in place) and its own
 * dependence on the current view.
 *
 * Every select click below is real input (`locator.click()`), not the
 * `element.click()` workaround this file used to carry for the same
 * reason `places-explorer-ring.spec.ts` did: `gestures.ts`'s `endPointer`
 * used to settle (re-render the marker/card layers) for an untracked
 * pointer too, which could replace a `.moss-places-card-select` button
 * between press and release. Fixed at the source — see that file's own
 * doc for the mechanism.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-cards.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function gotoReady(page: Page, search = ""): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`places/${search}`, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
  await page.waitForTimeout(300);
}

test("selecting a card expands its detail in place", async ({ page, browserName }) => {
  await gotoReady(page);
  const card = page.locator(".moss-places-cards .moss-card").first();
  const select = card.locator(".moss-places-card-select");
  const detail = card.locator(".moss-places-card-detail");

  await expect(card).toHaveAttribute("aria-current", "false");
  await expect(detail).toHaveAttribute("inert", "");
  const collapsedHeight = await detail.evaluate((el) => el.getBoundingClientRect().height);
  expect(collapsedHeight).toBeLessThan(1);

  await select.click();
  await page.waitForTimeout(400); // the grid-template-rows transition

  await expect(card).toHaveAttribute("aria-current", "true");
  await expect(select).toHaveAttribute("aria-pressed", "true");
  await expect(select).toHaveAttribute("aria-expanded", "true");
  await expect(detail).not.toHaveAttribute("inert", "");
  const expandedHeight = await detail.evaluate((el) => el.getBoundingClientRect().height);
  expect(expandedHeight).toBeGreaterThan(20);

  await expect(detail.locator(".moss-card-description")).toHaveCount(1);
  const readLink = detail.locator(".moss-places-card-read");
  await expect(readLink).toHaveCount(1);
  await expect(readLink).toHaveAttribute("href", /.+/);

  // Clicking the same card again collapses it back. `locator.click()`'s
  // own pre-click actionability wait hangs here in WebKit specifically —
  // the SAME class of harness limitation `places-explorer-ring.spec.ts`
  // diagnosed for a second real click on an element whose attributes an
  // earlier click just changed (there: a ring dot after the ring bloomed;
  // here: this very button, now `aria-pressed="true"`). See that file's
  // own comment for the direct, API-level evidence (isVisible/boundingBox/
  // elementFromPoint all already hold) that ruled out a product
  // re-render race before reaching for this.
  if (browserName === "webkit") {
    const box = (await select.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  } else {
    await select.click();
  }
  await page.waitForTimeout(400);
  await expect(card).toHaveAttribute("aria-current", "false");
  await expect(detail).toHaveAttribute("inert", "");
});

test("a card's cover image actually loads, SVG covers included", async ({ page }) => {
  await gotoReady(page);
  const covers = page.locator(".moss-places-cards .moss-card-cover img");
  const count = await covers.count();
  // Every work in the fixture carries `cover: cover.svg` — a blank box here
  // (a 404 naturalWidth of 0) is exactly what an unresolvable
  // `cover.svg.webp` produced before `resolve_cover` served an SVG
  // source's own raw path.
  expect(count).toBeGreaterThan(0);
  for (let i = 0; i < count; i++) {
    const naturalWidth = await covers.nth(i).evaluate((img) => (img as HTMLImageElement).naturalWidth);
    expect(naturalWidth, `card cover ${i} did not load`).toBeGreaterThan(0);
  }
});

test("the row follows the view: panning to Japan narrows it to Japan's own works", async ({ page }) => {
  await gotoReady(page);
  const allTitles = await page.locator(".moss-places-cards .moss-card-title").allTextContents();
  expect(allTitles.length).toBeGreaterThanOrEqual(10); // cover shows (close to) every work

  // A camera centred on Japan, far enough zoomed in that only its own four
  // works stay in view (±22px pad) — Lisbon/Lima/etc. are hundreds of
  // world units away on this projection, nowhere near the window. The
  // expected order is also the row's own date-descending rule: Kyoto
  // (04-15) > Osaka (04-01) > Tokyo (03-10) > Nara (02-20).
  await gotoReady(page, "?p=patterson&z=3&x=742&y=155");
  const japanTitles = await page.locator(".moss-places-cards .moss-card-title").allTextContents();
  expect(japanTitles).toEqual(["Kyoto Garden", "Osaka Market", "Tokyo Crossing", "Nara Deer Park"]);
});

// The row's layout contract, at a desktop and a phone size: collapsed cards
// share one height with their text inside and a 4:3 cover; opening one grows
// it in place — no other card or the map moves or resizes — and everything it
// holds fits. Two things used to break it: the page's heading margin (48px on
// top of every card title) pushed the text under the card's height cap, and
// the row itself grew with the opened card, lifting every card beside it.
for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
  test(`cards: one collapsed height, text and 4:3 cover inside, and opening one moves nothing else (${viewport.width}x${viewport.height})`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
    await page.waitForTimeout(500);

    const snapshot = () =>
      page.evaluate(() => {
        // Document coordinates: opening a card may scroll the page to show it, which moves nothing in the layout.
        const box = (el: Element) => {
          const r = el.getBoundingClientRect();
          return { x: r.x, y: r.y + window.scrollY, w: r.width, h: r.height, bottom: r.bottom + window.scrollY };
        };
        return {
          map: box(document.querySelector(".moss-places-viewport")!),
          cards: [...document.querySelectorAll<HTMLElement>(".moss-places-cards .moss-card")].map((card) => ({
            id: card.dataset.workId!,
            card: box(card),
            title: card.querySelector(".moss-card-title") ? box(card.querySelector(".moss-card-title")!) : null,
            meta: card.querySelector(".moss-places-card-select .moss-card-meta") ? box(card.querySelector(".moss-places-card-select .moss-card-meta")!) : null,
            cover: card.querySelector(".moss-card-cover") ? box(card.querySelector(".moss-card-cover")!) : null,
          })),
        };
      });

    const before = await snapshot();
    expect(before.cards.length).toBeGreaterThan(2);
    for (const c of before.cards) {
      expect(c.card.h, `card ${c.id} height`).toBeCloseTo(before.cards[0].card.h, 0);
      expect(c.card.bottom, `card ${c.id} sits on the map's bottom edge`).toBeGreaterThan(before.map.bottom - 24);
      expect(c.title!.bottom, `card ${c.id}: the title must sit inside the card`).toBeLessThanOrEqual(c.card.bottom + 0.5);
      if (c.meta) expect(c.meta.bottom, `card ${c.id}: the meta line must sit inside the card`).toBeLessThanOrEqual(c.card.bottom + 0.5);
      if (c.cover) expect(c.cover.w / c.cover.h, `card ${c.id}: cover aspect ratio`).toBeCloseTo(4 / 3, 1);
    }

    const first = page.locator(".moss-places-cards .moss-card").first();
    await first.locator(".moss-places-card-select").click();
    await expect(first).toHaveAttribute("aria-current", "true");
    await page.waitForTimeout(600); // the grid-template-rows transition

    const after = await snapshot();
    expect(after.map).toEqual(before.map);
    const opened = after.cards.find((c) => c.id === before.cards[0].id)!;
    expect(opened.card.w).toBeCloseTo(before.cards[0].card.w, 0);
    expect(opened.card.bottom).toBeCloseTo(before.cards[0].card.bottom, 0);
    expect(opened.card.h).toBeGreaterThan(before.cards[0].card.h + 40);
    expect(opened.card.y).toBeGreaterThanOrEqual(after.map.y);
    // Every other card still in the row is as big as it was and on the same
    // floor, and the ones before the opened card have not slid sideways. (The
    // row follows the view, and an opened card re-fits the map, so a card
    // AFTER it may leave the row and let later ones close the gap.)
    for (const c of after.cards.filter((c) => c.id !== opened.id)) {
      const was = before.cards.find((b) => b.id === c.id);
      if (was) expect({ y: c.card.y, w: c.card.w, h: c.card.h }, `card ${c.id} must not move or resize`).toEqual({ y: was.card.y, w: was.card.w, h: was.card.h });
    }
    for (const c of after.cards.slice(0, after.cards.findIndex((c) => c.id === opened.id))) {
      expect(c.card.x, `card ${c.id} must not slide sideways`).toBe(before.cards.find((b) => b.id === c.id)?.card.x);
    }
    if (viewport.width > 600) expect(after.cards.length).toBeGreaterThan(1);

    // Title, byline, description, place names and the read link all fit.
    const fit = await first.evaluate((card) => {
      const detail = card.querySelector<HTMLElement>(".moss-places-card-detail")!;
      const cardBox = card.getBoundingClientRect();
      const inside = [...card.querySelectorAll<HTMLElement>(".moss-card-title, .moss-places-card-detail > *")].every((el) => {
        const r = el.getBoundingClientRect();
        return r.top >= cardBox.top - 0.5 && r.bottom <= cardBox.bottom + 0.5 && r.right <= cardBox.right + 0.5;
      });
      return { clipped: detail.scrollHeight > detail.clientHeight + 1, inside, read: Boolean(card.querySelector(".moss-places-card-read")) };
    });
    expect(fit).toEqual({ clipped: false, inside: true, read: true });
  });
}
