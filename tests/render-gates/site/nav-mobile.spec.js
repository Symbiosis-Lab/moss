// @ts-check
/**
 * Render gate: the narrow-viewport hamburger menu shows every nav link.
 *
 * The scratch site has SIX nav pages (playwright/nav-mobile.config.ts), and
 * that count is the whole gate. `.nav-links` inherits `flex-wrap: wrap` from
 * the desktop nav rule, so when the open menu carried a fixed
 * `max-height: 200px`, the engine did not clip the overflow — it wrapped the
 * links into extra COLUMNS. Six links became two columns of three, ten became
 * four columns, and in webkit twelve pushed three labels past the right edge
 * where `overflow: hidden` deleted them outright. Every one of those states
 * looks fine to a Rust test reading the emitted CSS and to jsdom, which lays
 * nothing out; "how many columns is this menu" has an answer only in an engine.
 *
 * The three breakpoint-visibility tests below are the older contract and stay:
 * hamburger appears only under 20rem, which is the same string
 * `MOBILE_MENU_QUERY` in crates/moss-build/src/js-src/site/theme.ts mirrors
 * to decide when the closed menu is `inert`.
 */
import { test, expect } from '@playwright/test';

/** Rounded left edge of each nav link — one distinct value means one column. */
async function linkColumns(page) {
  return page.evaluate(() => {
    const links = [...document.querySelectorAll('.nav-links a')];
    return [...new Set(links.map((a) => Math.round(a.getBoundingClientRect().left)))];
  });
}

test.describe('Mobile Navigation (375px) - Nav Links Visible', () => {
  test.beforeEach(async ({ page }) => {
    // Set mobile viewport - this is above the 20rem (320px) hamburger threshold
    await page.setViewportSize({ width: 375, height: 812 });
  });

  test('nav links visible directly on mobile (no hamburger)', async ({ page }) => {
    await page.goto('/');

    const hamburger = page.locator('.mobile-menu-button');
    const navLinks = page.locator('.nav-links');

    // Hamburger should be hidden at 375px (above 320px threshold)
    await expect(hamburger).not.toBeVisible();

    // Nav links should be visible directly
    await expect(navLinks).toBeVisible();
  });

});

test.describe('Narrow Viewport (<320px) - Hamburger Menu', () => {
  test.beforeEach(async ({ page }) => {
    // Set very narrow viewport - below 20rem (320px) threshold
    await page.setViewportSize({ width: 300, height: 600 });
  });

  test('hamburger visible on very narrow screens', async ({ page }) => {
    await page.goto('/');

    const hamburger = page.locator('.mobile-menu-button');

    // Hamburger should be visible below 320px
    await expect(hamburger).toBeVisible();
  });

  test('nav links hidden by default on very narrow screens', async ({ page }) => {
    await page.goto('/');

    const navLinks = page.locator('.nav-links');

    // Collapsed and invisible. `visibility: hidden` is what makes the closed
    // panel un-hit-testable in CSS alone, before theme.ts sets `inert`;
    // `max-height: 0` is what keeps it from contributing page scroll.
    await expect(navLinks).toHaveCSS('max-height', '0px');
    await expect(navLinks).toHaveCSS('opacity', '0');
    await expect(navLinks).toHaveCSS('visibility', 'hidden');
  });

  test('the closed menu occupies no space', async ({ page }) => {
    await page.goto('/');

    // The panel is absolutely positioned, but an abspos box still contributes
    // scrollable overflow to the page. Sized by its links and merely faded out,
    // a six-link menu made a 600px viewport scroll to 624 with nothing on
    // screen to explain it; collapsed, it contributes nothing.
    const box = await page.locator('.nav-links').boundingBox();
    expect(box?.height ?? 0).toBeLessThanOrEqual(0.5);
  });

  test('nav links expand when hamburger clicked', async ({ page }) => {
    await page.goto('/');

    const hamburger = page.locator('.mobile-menu-button');
    const navLinks = page.locator('.nav-links');

    await hamburger.click();

    await expect(navLinks).toHaveClass(/mobile-open/);
    await expect(navLinks).toHaveCSS('opacity', '1');
    await expect(navLinks).toHaveCSS('visibility', 'visible');
  });

  test('every nav link is in one column, inside the panel', async ({ page }) => {
    await page.goto('/');
    await page.locator('.mobile-menu-button').click();
    await expect(page.locator('.nav-links')).toHaveCSS('opacity', '1');

    // Six `nav: true` pages. Fewer than five and the shape this gate exists
    // to catch cannot occur, so the count is asserted rather than assumed.
    const count = await page.locator('.nav-links a').count();
    expect(count).toBe(6);

    // One distinct left edge. Two or more means the bounded-height column
    // wrapped into a grid — the shape this gate exists to catch.
    expect(await linkColumns(page)).toHaveLength(1);

    // And nothing sits outside the panel's own box, in either axis.
    const escaped = await page.evaluate(() => {
      const panel = document.querySelector('.nav-links');
      const box = panel.getBoundingClientRect();
      return [...panel.querySelectorAll('a')]
        .filter((a) => {
          const r = a.getBoundingClientRect();
          return r.right > box.right + 0.5 || r.left < box.left - 0.5;
        })
        .map((a) => a.textContent);
    });
    expect(escaped).toEqual([]);

    // The panel grew to hold them rather than stopping at a number: with six
    // links it is far taller than the 200px it used to cap at, and it is not
    // scrolling, so nothing is hidden below the fold either.
    const { height, scrollH, clientH } = await page.evaluate(() => {
      const panel = document.querySelector('.nav-links');
      return {
        height: panel.getBoundingClientRect().height,
        scrollH: panel.scrollHeight,
        clientH: panel.clientHeight,
      };
    });
    expect(height).toBeGreaterThan(200);
    expect(scrollH).toBeLessThanOrEqual(clientH + 1);
  });
});

test.describe('Medium Breakpoint Navigation (600px)', () => {
  test.beforeEach(async ({ page }) => {
    // Set tablet viewport (between 20rem and 48rem)
    await page.setViewportSize({ width: 600, height: 900 });
  });

  test('nav links visible without hamburger', async ({ page }) => {
    await page.goto('/');

    const hamburger = page.locator('.mobile-menu-button');
    const navLinks = page.locator('.nav-links');

    // Hamburger should be hidden
    await expect(hamburger).not.toBeVisible();

    // Nav links should be visible
    await expect(navLinks).toBeVisible();
  });
});
