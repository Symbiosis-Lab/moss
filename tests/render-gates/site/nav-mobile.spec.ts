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

test.describe('Open menu at the 320px edge - full sheet', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
  });

  async function openMenu(page) {
    await page.goto('/about/');
    await page.locator('.mobile-menu-button').click();
    await expect(page.locator('.nav-links')).toHaveCSS('opacity', '1');
  }

  test('the sheet spans the viewport from the header bottom to the viewport bottom', async ({ page }) => {
    await openMenu(page);
    const m = await page.evaluate(() => {
      const r = document.querySelector('.nav-links').getBoundingClientRect();
      return {
        left: r.left, right: r.right, top: r.top, bottom: r.bottom,
        headerBottom: document.querySelector('header').getBoundingClientRect().bottom,
        w: window.innerWidth, h: window.innerHeight,
      };
    });
    expect(m.left).toBeCloseTo(0, 0);
    expect(m.right).toBeCloseTo(m.w, 0);
    expect(m.top).toBeCloseTo(m.headerBottom, 0);
    expect(m.bottom).toBeCloseTo(m.h, 0);
    // Opaque: the page behind never shows through.
    const bg = await page.locator('.nav-links').evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(bg).not.toMatch(/rgba\(.*, 0\)|transparent/);
  });

  test('links start on the page gutter, level with the logo, and none overflows the sheet', async ({ page }) => {
    await openMenu(page);
    const m = await page.evaluate(() => {
      const logo = document.querySelector('.site-name, .breadcrumb-segment').getBoundingClientRect();
      const sheet = document.querySelector('.nav-links').getBoundingClientRect();
      const links = [...document.querySelectorAll('.nav-links a')].map((a) => a.getBoundingClientRect());
      return {
        logoLeft: logo.left, firstLeft: links[0].left,
        outside: links.filter((r) => r.right > sheet.right + 0.5).length,
        shortest: Math.min(...links.map((r) => r.height)),
      };
    });
    expect(m.firstLeft).toBeCloseTo(m.logoLeft, 0);
    expect(m.outside).toBe(0);
    expect(m.shortest).toBeGreaterThanOrEqual(44);
  });

  test('opening moves focus to the first link and keeps the button name', async ({ page }) => {
    await openMenu(page);
    await expect(page.locator('.nav-links a').first()).toBeFocused();
    await expect(page.locator('.mobile-menu-button')).toHaveAttribute('aria-label', 'Menu');
    await expect(page.locator('.mobile-menu-button')).toHaveAttribute('aria-expanded', 'true');
  });

  test('Escape closes the menu and returns focus to the button', async ({ page }) => {
    await openMenu(page);
    await page.keyboard.press('Escape');
    const button = page.locator('.mobile-menu-button');
    await expect(button).toHaveAttribute('aria-expanded', 'false');
    await expect(button).toHaveAttribute('aria-label', 'Menu');
    await expect(button).toBeFocused();
    await expect(page.locator('.nav-links')).toHaveCSS('visibility', 'hidden');
  });

  test('the page behind cannot scroll while the menu is open', async ({ page }) => {
    await page.goto('/about/');
    // Without a scrollable page the lock proves nothing.
    await page.evaluate(() => {
      const filler = document.createElement('div');
      filler.style.height = '3000px';
      document.body.appendChild(filler);
    });
    expect(await page.evaluate(() => document.documentElement.scrollHeight > innerHeight)).toBe(true);
    await page.locator('.mobile-menu-button').click();
    await expect(page.locator('.nav-links')).toHaveCSS('opacity', '1');
    // A wheel gesture, as a reader would scroll: `scrollTo` ignores the lock.
    await page.mouse.move(160, 500);
    await page.mouse.wheel(0, 500);
    await page.waitForTimeout(150);
    expect(await page.evaluate(() => window.scrollY)).toBe(0);
    const overflow = () => page.evaluate(() => getComputedStyle(document.documentElement).overflow);
    expect(await overflow()).toBe('hidden');
    await page.keyboard.press('Escape');
    expect(await overflow()).not.toBe('hidden');
  });

  test('under a header=over hero the links keep the sheet ink through the fade-out', async ({ page }) => {
    await page.goto('/cover/');
    const link = page.locator('.nav-links a').first();
    const color = () => link.evaluate((a) => getComputedStyle(a).color);
    await page.locator('.mobile-menu-button').click();
    await expect(page.locator('.nav-links')).toHaveCSS('opacity', '1');
    const sheetInk = await color();
    // Start every transition halfway so a colour that is about to change shows
    // at once instead of after the 160ms fade has run out.
    await page.addStyleTag({ content: '* { transition-duration: 10s !important; transition-delay: -5s !important; }' });
    await page.keyboard.press('Escape');
    expect(await color()).toBe(sheetInk);
  });

  test('the current page link carries aria-current="page"', async ({ page }) => {
    await openMenu(page);
    const current = page.locator('.nav-links a.active');
    await expect(current).toHaveCount(1);
    await expect(current).toHaveAttribute('aria-current', 'page');
  });

  test('the hamburger turns into a cross by transforming its strokes', async ({ page }) => {
    await openMenu(page);
    const t = await page.evaluate(() =>
      [...document.querySelectorAll('.mobile-menu-button line')].map((l) => getComputedStyle(l).transform),
    );
    expect(t).toHaveLength(3);
    expect(t[1]).not.toBe('none');
    expect(t[2]).not.toBe('none');
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
