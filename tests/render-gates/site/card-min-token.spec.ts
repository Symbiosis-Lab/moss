/**
 * A theme's `--moss-card-min` sets a grid listing's card width at every
 * container width, a phone's 320px included: moss's narrow-container fallback
 * collapses a listing to one column only for its own defaults. A minimum wider
 * than the container still fits it rather than overflowing, at any width.
 */
import { test, expect, type Page } from "@playwright/test";

const LISTINGS = ["shelf", "roster"] as const;

async function columns(page: Page) {
  return page.evaluate((names) =>
    names.map((name, i) => {
      const grid = document.querySelectorAll('.moss-cards[data-layout="grid"]')[i] as HTMLElement;
      return {
        name,
        tracks: getComputedStyle(grid).gridTemplateColumns.split(" ").filter(Boolean).length,
        overflow: grid.scrollWidth - grid.clientWidth,
        container: grid.parentElement!.getBoundingClientRect().width,
      };
    }), [...LISTINGS]);
}

async function setToken(page: Page, value: string | null) {
  await page.evaluate((v) => {
    if (v === null) document.documentElement.style.setProperty("--moss-card-min", "initial");
    else document.documentElement.style.setProperty("--moss-card-min", v);
  }, value);
}

test.describe("theme card minimum at a phone's width", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 800 });
    await page.goto("/");
  });

  test("the container is under the 320px fallback's threshold", async ({ page }) => {
    for (const listing of await columns(page)) expect(listing.container, listing.name).toBeLessThanOrEqual(320);
  });

  test("a small theme minimum keeps two cards a row", async ({ page }) => {
    for (const listing of await columns(page)) {
      expect(listing.tracks, listing.name).toBeGreaterThanOrEqual(2);
      expect(listing.overflow, listing.name).toBeLessThanOrEqual(0);
    }
  });

  test("a theme minimum wider than the container is one card that fits", async ({ page }) => {
    await setToken(page, "40rem");
    for (const listing of await columns(page)) {
      expect(listing.tracks, listing.name).toBe(1);
      expect(listing.overflow, listing.name).toBeLessThanOrEqual(0);
    }
  });

  test("above 320px, a theme minimum wider than the container still fits it", async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 });
    await setToken(page, "40rem");
    for (const listing of await columns(page)) {
      expect(listing.container, listing.name).toBeGreaterThan(320);
      expect(listing.tracks, listing.name).toBe(1);
      expect(listing.overflow, listing.name).toBeLessThanOrEqual(0);
    }
  });

  test("with no theme minimum, moss's own listings stay one card a row", async ({ page }) => {
    await setToken(page, null);
    for (const listing of await columns(page)) {
      expect(listing.tracks, listing.name).toBe(1);
      expect(listing.overflow, listing.name).toBeLessThanOrEqual(0);
    }
  });
});
