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

test("every card's cover sits at the same offset from its own card top, even when siblings differ in content height", async ({ page }) => {
  await gotoReady(page);
  // Selecting a card stretches every sibling to match its own (now taller)
  // height — `.moss-places-cards`' own `align-items: stretch` — which is
  // exactly the condition that exposed the stagger: a `<button>`
  // (`.moss-places-card-select`) centres its content vertically by default
  // once its own box is taller than that content, so cards whose title
  // happened to wrap to one line (a shorter `.moss-card-row`) got pushed
  // down by half the slack while two-line titles got none.
  const select = page.locator(".moss-places-card-select").first();
  await select.click();
  await page.waitForTimeout(400); // the grid-template-rows/inline-size transition

  const offsets = await page.$$eval(".moss-places-cards .moss-card", (cards) =>
    cards.map((card) => {
      const cover = card.querySelector(".moss-card-cover");
      if (!cover) return null;
      return cover.getBoundingClientRect().top - card.getBoundingClientRect().top;
    }),
  );
  const present = offsets.filter((offset): offset is number => offset != null);
  expect(present.length).toBeGreaterThan(1);
  for (const offset of present) {
    expect(offset).toBeCloseTo(present[0], 0);
  }
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
