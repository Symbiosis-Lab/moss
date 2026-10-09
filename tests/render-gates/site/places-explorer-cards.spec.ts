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

test("the row follows the view: panning to East Asia narrows it to works in that view", async ({ page }) => {
  await gotoReady(page);
  const allTitles = await page.locator(".moss-places-cards .moss-card-title").allTextContents();
  expect(allTitles.length).toBeGreaterThanOrEqual(10); // cover shows (close to) every work

  // A camera centred on Japan also includes the boundary story near Taiwan;
  // these five works stay in view (±22px pad) — Lisbon/Lima/etc. are hundreds of
  // world units away on this projection, nowhere near the window. The
  // expected order is also the row's own date-descending rule: Kyoto
  // (04-15) > Osaka (04-01) > Tokyo (03-10) > Nara (02-20), then the
  // boundary story from the preceding year.
  await gotoReady(page, "?p=patterson&z=3&x=742&y=155");
  const japanTitles = await page.locator(".moss-places-cards .moss-card-title").allTextContents();
  expect(japanTitles).toEqual(["Kyoto Garden", "Osaka Market", "Tokyo Crossing", "Nara Deer Park", "Tile boundary story"]);
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

// The row spans the map edge to edge: the first card rests 12px in from the
// left edge, the last 12px short of the right edge once scrolled to the end,
// and in between cards scroll out under the map's own edge rather than being
// cut 12px short of it. Scroll snapping keeps the same 12px.
for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
  test(`cards: the row runs from the map's left edge to its right edge (${viewport.width}px)`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
    await page.waitForTimeout(500);
    const m = await page.evaluate(() => {
      const map = document.querySelector(".moss-places-viewport")!.getBoundingClientRect();
      const row = document.querySelector<HTMLElement>(".moss-places-cards")!;
      const rowBox = row.getBoundingClientRect();
      const cards = [...row.querySelectorAll<HTMLElement>(".moss-card")];
      const first = cards[0].getBoundingClientRect();
      row.scrollLeft = row.scrollWidth;
      const last = cards[cards.length - 1].getBoundingClientRect();
      return { mapLeft: map.left, mapRight: map.right, rowLeft: rowBox.left, rowRight: rowBox.right, firstLeft: first.left, lastRight: last.right, scrollPadding: getComputedStyle(row).scrollPaddingInlineStart, overflows: row.scrollWidth > row.clientWidth };
    });
    expect(m.rowLeft, "row's left edge is the map's").toBeCloseTo(m.mapLeft, 0);
    expect(m.rowRight, "row's right edge is the map's").toBeCloseTo(m.mapRight, 0);
    expect(m.firstLeft - m.mapLeft, "first card rests 12px in").toBeCloseTo(12, 0);
    expect(m.overflows, "the fixture has more cards than fit").toBe(true);
    expect(m.mapRight - m.lastRight, "last card rests 12px in at the end of the scroll").toBeCloseTo(12, 0);
    expect(m.scrollPadding).toBe("12px");
  });
}

// Collapsed covers are one fixed box, cropped to fit: the fixture mixes a
// square-ish, a tall and a wide image, and every <img> must fill the same
// box rather than keep its own natural height.
test("cards: every collapsed cover image fills one shared box", async ({ page }) => {
  await gotoReady(page);
  const sizes = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLImageElement>(".moss-places-cards .moss-card:not([aria-current='true']) .moss-card-cover > img")].map((img) => {
      const box = img.parentElement!.getBoundingClientRect();
      const r = img.getBoundingClientRect();
      return { box: [box.width, box.height], img: [r.width, r.height], src: img.getAttribute("src"), fit: getComputedStyle(img).objectFit };
    }),
  );
  expect(new Set(sizes.map((s) => s.src)).size, "the fixture mixes cover shapes").toBeGreaterThan(2);
  for (const s of sizes) {
    expect(s.fit).toBe("cover");
    expect(s.box[0] / s.box[1], "box is 4:3").toBeCloseTo(4 / 3, 1);
    expect(s.img[0], "image width fills the box").toBeCloseTo(s.box[0], 0);
    expect(Math.abs(s.img[1] - s.box[1]), "image height fills the box (plus the 1px hairline overshoot)").toBeLessThanOrEqual(1.5);
    expect(s.img).toEqual(sizes[0].img);
  }
});

// The collapsed meta line is the author names then the date, always both, on one line. The
// author is ellipsised when the pair does not fit ("Alexandria Papadopoulos
// Konstantinou" is wider than the line on its own); the date, which carries
// its own " · " separator as text, is never cut or wrapped.
test("cards: meta line always shows the date whole and ellipsises only the author", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  await page.waitForTimeout(500);
  const metas = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>(".moss-places-cards .moss-card")].map((card) => {
      const meta = card.querySelector<HTMLElement>(".moss-places-card-select .moss-card-meta");
      if (!meta) return null;
      const box = meta.getBoundingClientRect();
      const author = meta.querySelector<HTMLElement>(".moss-card-meta-author");
      const date = meta.querySelector<HTMLElement>(".moss-card-meta-date");
      const dr = date?.getBoundingClientRect();
      const lineHeight = parseFloat(getComputedStyle(meta).lineHeight);
      return {
        title: card.querySelector(".moss-card-title")!.textContent,
        text: meta.textContent,
        authorText: author?.textContent ?? null,
        authorLeft: author?.getBoundingClientRect().left ?? null,
        dateLeft: dr?.left ?? null,
        authorTextRight: author ? (() => { const r = document.createRange(); r.selectNodeContents(author); return r.getBoundingClientRect().right; })() : null,
        dateFullyShown: dr ? dr.top >= box.top - 0.5 && dr.bottom <= box.bottom + 0.5 && dr.right <= box.right + 0.5 : null,
        dateOneLine: dr ? dr.height <= lineHeight + 0.5 : null,
        authorClipped: author ? author.scrollWidth > author.clientWidth : null,
      };
    }),
  );
  const wide = metas.find((m) => m?.title === "Lisbon Walk")!;
  // The card shows the `author:` name, not the byline's credit text.
  expect(wide.text).toBe("Ana\u00a0\u00b7\u00a02024-06-10");
  // A page with only a byline falls back to its first entry.
  expect(metas.find((m) => m?.title === "Coimbra Library")!.text).toBe("Field notes\u00a0\u00b7\u00a02024-05-01");
  expect(wide.authorLeft!, "author before date").toBeLessThan(wide.dateLeft!);
  expect(wide.dateLeft! - wide.authorTextRight!, "the dot sits against the author's text, in no wide gap").toBeLessThanOrEqual(1);
  expect(wide.dateFullyShown, "both fit: the date is shown").toBe(true);
  expect(wide.authorClipped, "both fit: no ellipsis").toBe(false);

  const narrow = metas.find((m) => m?.title === "Portugal Overview")!;
  expect(narrow.dateFullyShown, "they do not fit: the date is still fully visible").toBe(true);
  expect(narrow.dateOneLine, "the date does not wrap").toBe(true);
  expect(narrow.authorClipped, "the author's box is narrower than its text: ellipsised").toBe(true);

  for (const m of metas) {
    if (m && m.dateFullyShown !== null) expect(m.dateFullyShown, `${m.title}: the date is always whole`).toBe(true);
  }
});

// Closing an open card from the map itself: a click on bare map (not a marker,
// cluster, card, chip or control, and not the end of a drag) and Escape with
// focus in the map both close it, with the camera left where it is. All real
// input.
test.describe("an open card closes from the map", () => {
  async function openFirstCard(page: Page) {
    await gotoReady(page);
    const first = page.locator(".moss-places-cards .moss-card").first();
    await first.locator(".moss-places-card-select").click();
    await expect(first).toHaveAttribute("aria-current", "true");
    await page.waitForTimeout(500);
    return first;
  }
  /** A point on the map that no marker, card, chip or control covers. */
  const bareMapPoint = (page: Page) =>
    page.evaluate(() => {
      const r = document.querySelector(".moss-places-viewport")!.getBoundingClientRect();
      for (let fy = 0.25; fy < 0.6; fy += 0.04) {
        for (let fx = 0.2; fx < 0.9; fx += 0.04) {
          const x = r.left + r.width * fx;
          const y = r.top + r.height * fy;
          const hit = document.elementFromPoint(x, y);
          if (hit && !hit.closest("button, a, .moss-card, .moss-places-chip, .moss-places-controls") && hit.closest(".moss-places-viewport")) return { x, y };
        }
      }
      throw new Error("no bare map point");
    });
  const cameraOf = (page: Page) => page.evaluate(() => ["z", "x", "y"].map((k) => new URL(location.href).searchParams.get(k)).join(","));

  test("a click on bare map closes it and leaves the camera", async ({ page }) => {
    const first = await openFirstCard(page);
    expect(page.url()).toContain("article=");
    const camera = await cameraOf(page);
    const p = await bareMapPoint(page);
    await page.mouse.click(p.x, p.y);
    await page.waitForTimeout(400);
    await expect(page.locator('.moss-places-cards .moss-card[aria-current="true"]')).toHaveCount(0);
    await expect(first).toHaveAttribute("aria-current", "false");
    expect(page.url()).not.toContain("article=");
    expect(await cameraOf(page)).toBe(camera);
  });

  test("dragging the map with a card open leaves it open", async ({ page }) => {
    const first = await openFirstCard(page);
    const p = await bareMapPoint(page);
    await page.mouse.move(p.x, p.y);
    await page.mouse.down();
    await page.mouse.move(p.x + 60, p.y + 20, { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(400);
    await expect(first).toHaveAttribute("aria-current", "true");
    expect(page.url()).toContain("article=");
  });

  test("Escape with focus in the map closes it", async ({ page }) => {
    const first = await openFirstCard(page);
    await page.locator(".moss-places-viewport").focus();
    await page.keyboard.press("Escape");
    await page.waitForTimeout(400);
    await expect(first).toHaveAttribute("aria-current", "false");
    expect(page.url()).not.toContain("article=");
  });
});
