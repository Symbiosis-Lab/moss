/**
 * A hero drawn from an event is the only hero on the page, shows the event the
 * way the site shows events elsewhere, and hands over to the hero the page
 * falls back to once the event has ended.
 *
 * These are questions about what is painted, so they are asked of an engine:
 * a fallback kept in the markup but not drawn, a label on the same line as the
 * date, and exactly one caption left after the swap.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → HERO_FROM_GATE. The event ends in
 * 2099, so the build always draws it; the ended case moves the browser's clock.
 */
import { test, expect } from "@playwright/test";

test.describe("hero drawn from an event", () => {
  test("while the event is upcoming, one hero is drawn and the fallback is not", async ({ page }) => {
    await page.goto("/");

    const heroes = page.locator(".moss-hero");
    await expect(heroes).toHaveCount(1);
    await expect(heroes).toBeVisible();
    await expect(heroes).toContainText("Example Show");
    await expect(page.getByText("Between seasons")).toHaveCount(0);
    await expect(page.locator(".moss-hero-caption")).toHaveCount(1);
    await expect(page.locator(".moss-hero-caption")).toBeVisible();
  });

  test("the Upcoming label shares a line with the date", async ({ page }) => {
    await page.goto("/");

    const label = page.locator(".moss-hero .moss-event-meta .moss-event-upcoming");
    const time = page.locator(".moss-hero .moss-event-meta .moss-event-time");
    await expect(label).toBeVisible();
    const [l, t] = [(await label.boundingBox())!, (await time.boundingBox())!];
    expect(Math.abs(l.y - t.y)).toBeLessThan(t.height);
    expect(l.x).toBeGreaterThan(t.x);
  });

  test("the link to the event is moss's call-to-action button", async ({ page }) => {
    await page.goto("/");

    const link = page.locator(".moss-hero .moss-buttons a.moss-btn");
    await expect(link).toBeVisible();
    await expect(link).toHaveText("More about Example Show →");
    await expect(link).toHaveAttribute("href", "/shows/example-show/");
    await expect(link).toHaveAttribute("data-role", "primary");
  });

  test("after the event ends only the fallback hero and one caption remain", async ({ page }) => {
    await page.clock.setFixedTime(new Date("2099-03-02T09:00:00"));
    await page.goto("/");

    const heroes = page.locator(".moss-hero");
    await expect(heroes).toHaveCount(1);
    await expect(heroes).toBeVisible();
    await expect(heroes).toContainText("Between seasons");
    await expect(heroes).not.toContainText("Example Show");
    await expect(page.locator(".moss-hero-caption")).toHaveCount(1);
    await expect(page.locator(".moss-hero-caption")).toContainText("Example Hall");
    await expect(page.locator(".moss-hero-caption")).toBeVisible();
    await expect(page.locator("template")).toHaveCount(0);
    const hero = (await heroes.boundingBox())!;
    const caption = (await page.locator(".moss-hero-caption").boundingBox())!;
    expect(caption.y).toBeGreaterThanOrEqual(hero.y + hero.height - 1);
  });

  for (const [state, now] of [["upcoming", "2099-02-01T09:00:00"], ["ended", "2099-03-02T09:00:00"]] as const) {
    test(`header=over: the header stays painted over the hero (${state})`, async ({ page }) => {
      await page.clock.setFixedTime(new Date(now));
      await page.goto("/over/");

      const hero = (await page.locator(".moss-hero").boundingBox())!;
      const header = (await page.locator("body > header").boundingBox())!;
      expect(header.y).toBeGreaterThanOrEqual(hero.y - 1);
      expect(header.y + header.height).toBeLessThanOrEqual(hero.y + hero.height);
      expect(await page.locator("body > header").evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
      expect(await page.locator("body > header .site-name").evaluate((el) => getComputedStyle(el).color)).toBe("rgb(255, 255, 255)");
    });
  }

  test("the hero is still drawn in the minute before the event ends", async ({ page }) => {
    await page.clock.setFixedTime(new Date("2099-03-01T21:29:00"));
    await page.goto("/");

    await expect(page.locator(".moss-hero")).toContainText("Example Show");
  });
});
