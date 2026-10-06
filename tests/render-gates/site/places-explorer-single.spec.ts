/**
 * Render gate: a places root with a single work. The initial frame must keep
 * the lone marker inside the map and clear of the card row, the chip and the
 * controls, and no card may be open until the reader opens one.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_SINGLE_GATE), built by the playwright config at parse time.
 *
 * Run via:
 *   npx playwright test -c playwright/places-explorer-single.config.ts
 */
import { test, expect } from "@playwright/test";

const FIGURE = "figure[data-moss-places-explorer]";

for (const size of [{ width: 390, height: 844 }, { width: 1280, height: 860 }]) {
  test(`the lone marker is framed clear of the overlays and no card is open, at ${size.width}px`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto("places/", { waitUntil: "domcontentloaded" });
    await expect(page.locator(FIGURE)).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
    await expect(page.locator(".moss-places-marker")).toHaveCount(1);
    await page.waitForTimeout(800);

    const boxes = await page.evaluate(() => {
      const rect = (el: Element | null) => {
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return r.width > 0 && r.height > 0 ? { x: r.x, y: r.y, w: r.width, h: r.height } : null;
      };
      return {
        viewport: rect(document.querySelector(".moss-places-viewport")),
        marker: rect(document.querySelector(".moss-places-marker")),
        overlays: [".moss-places-cards .moss-card", ".moss-places-chip", ".moss-places-controls"].map((s) => ({ s, r: rect(document.querySelector(s)) })),
      };
    });
    const v = boxes.viewport!;
    const m = boxes.marker!;
    expect(m.x, "marker inside on the left").toBeGreaterThanOrEqual(v.x);
    expect(m.x + m.w, "marker inside on the right").toBeLessThanOrEqual(v.x + v.w);
    expect(m.y, "marker inside at the top").toBeGreaterThanOrEqual(v.y);
    expect(m.y + m.h, "marker inside at the bottom").toBeLessThanOrEqual(v.y + v.h);
    for (const { s, r } of boxes.overlays) {
      if (!r) continue;
      const overlap = m.x < r.x + r.w && m.x + m.w > r.x && m.y < r.y + r.h && m.y + m.h > r.y;
      expect(overlap, `marker ${JSON.stringify(m)} must not sit under ${s} ${JSON.stringify(r)}`).toBe(false);
    }

    await expect(page.locator(".moss-places-cards .moss-card[aria-current='true']")).toHaveCount(0);
    expect(new URL(page.url()).searchParams.has("article"), "no article in the address").toBe(false);
  });
}

// The opened card's chapter links keep the card's own line rhythm (a theme's
// loose body line-height used to apply), and each place is a link to its page.
test("an opened card has tight link lines and links its place to the place page", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("places/", { waitUntil: "domcontentloaded" });
  await expect(page.locator(FIGURE)).toHaveAttribute("data-moss-places-explorer-ready", "ready", { timeout: 10000 });
  const card = page.locator(".moss-places-cards .moss-card").first();
  await card.locator(".moss-places-card-select").click();
  await expect(card).toHaveAttribute("aria-current", "true");
  const detail = card.locator(".moss-places-card-detail");

  const chapter = detail.locator("ul a").first();
  await expect(chapter).toHaveText("First Chapter");
  const { lineHeight, fontSize } = await chapter.evaluate((el) => {
    const cs = getComputedStyle(el);
    return { lineHeight: parseFloat(cs.lineHeight), fontSize: parseFloat(cs.fontSize) };
  });
  expect(lineHeight / fontSize, "chapter link line-height ratio").toBeLessThanOrEqual(1.5);

  const place = detail.locator("p a", { hasText: "Cambridge" });
  await expect(place).toHaveAttribute("href", "/places/cambridge/");
});
