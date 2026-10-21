/**
 * `[site].header = "none"`: no header band, a pinned island that carries the
 * controls, and the contents ruler's scope — the assertions that need a real
 * engine, on sites the moss CLI built.
 *
 * Fixture: playwright/header-none.config.ts → tests/e2e/helpers/header-none-site.ts
 * Run via: bash scripts/render-gates.sh header-none
 */
import { test, expect, type Page } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';
import { AFTER_END, BAND_PATH, BAND_SECTIONS, COLLECTIONS } from '../../e2e/helpers/header-none-site';

const BAND_PAGE = `http://localhost:${gatePort('header-none:band')}${BAND_PATH}`;
const ALL = [...COLLECTIONS, AFTER_END];

async function frames(page: Page, n = 3): Promise<void> {
  await page.evaluate(
    (count) =>
      new Promise<void>((resolve) => {
        let left = count;
        const step = () => (--left <= 0 ? resolve() : requestAnimationFrame(step));
        requestAnimationFrame(step);
      }),
    n,
  );
}

async function open(page: Page, url = '/', width = 1600, height = 900): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(url);
  await page.waitForSelector('.moss-nav-island', { state: 'attached' });
  await page.evaluate(() => document.fonts.ready);
  await frames(page);
}

async function scrollTo(page: Page, y: number): Promise<void> {
  await page.evaluate((target) => window.scrollTo(0, target), y);
  await frames(page);
}

/** Park a section's heading where a fragment jump would. */
async function jumpTo(page: Page, index: number): Promise<void> {
  await page.evaluate((i) => {
    document.querySelectorAll<HTMLElement>('main h2[id]')[i].scrollIntoView({ block: 'start' });
  }, index);
  await frames(page);
}

/** Put the top of `selector`'s element at `fraction` of the window height. */
async function placeTop(page: Page, selector: string, fraction: number): Promise<void> {
  await page.evaluate(
    ([sel, f]) => {
      const el = document.querySelector(sel as string)!;
      window.scrollTo(0, el.getBoundingClientRect().top + window.scrollY - (f as number) * window.innerHeight);
    },
    [selector, fraction] as const,
  );
  await frames(page);
}

const island = (page: Page) => page.locator('.moss-nav-island');
const shown = (page: Page) => island(page).evaluate((el) => (el as HTMLElement).dataset.shown);
const isAway = (page: Page) => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-away'));
const nameOf = (page: Page) =>
  page.locator('.moss-nav-island-section-name').evaluate((el) => (el.textContent ?? '').trim());

test.describe('a one-page site with header = "none"', () => {
  test.beforeEach(async ({ page }) => {
    await open(page);
  });

  test('has no header element, and keeps the skip link', async ({ page }) => {
    expect(await page.locator('header').count()).toBe(0);
    expect(await page.locator('.main-nav').count()).toBe(0);
    await expect(page.locator('.moss-skip-link')).toHaveAttribute('href', '#main-content');
  });

  test('the island carries the theme toggle, with no trail and no site name', async ({ page }) => {
    expect(await page.locator('.moss-nav-island .nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.moss-nav-island-trail').count()).toBe(0);
    expect(await island(page).locator('.site-name').count()).toBe(0);

    await jumpTo(page, 1);
    const theme = () => page.evaluate(() => document.documentElement.getAttribute('data-theme'));
    const before = await theme();
    await island(page).locator('.nav-theme-btn').click();
    await expect.poll(theme).not.toBe(before);
    await island(page).locator('.nav-theme-btn').click();
    await expect.poll(theme).toBe(before);
  });

  test('is hidden at the top, appears at the first section, and stays to the end', async ({ page }) => {
    expect(await shown(page)).toBe('false');
    expect(await island(page).evaluate((el) => (el as HTMLElement).inert)).toBe(true);
    await scrollTo(page, 60);
    expect(await shown(page), 'still on the title area').toBe('false');

    await jumpTo(page, 0);
    expect(await shown(page)).toBe('true');
    await expect.poll(() => island(page).evaluate((el) => getComputedStyle(el).opacity)).toBe('1');

    // Down, then up a little, then to the very bottom: it never leaves.
    for (const k of [2, 4]) {
      await jumpTo(page, k);
      expect(await shown(page), `at section ${k}`).toBe('true');
    }
    await scrollTo(page, 1e6);
    expect(await shown(page), 'at the bottom').toBe('true');
    await page.evaluate(() => window.scrollBy(0, -200));
    await frames(page);
    expect(await shown(page), 'after scrolling up a little').toBe('true');

    // And back above the first section it is put away again.
    await scrollTo(page, 0);
    expect(await shown(page)).toBe('false');
  });

  test('the button is labelled with the current section and opens the panel, pictures aside', async ({ page }) => {
    for (const k of [0, 2, 4]) {
      await jumpTo(page, k);
      expect(await nameOf(page), `section ${k}`).toBe(COLLECTIONS[k]);
    }
    const button = page.locator('.moss-nav-island-sections');
    await expect(button).toBeVisible();
    await button.click();
    const rows = page.locator('[data-island-menu="sections"] a');
    // Headings after the marker are in the panel as well.
    expect(await rows.allTextContents()).toEqual(ALL);
  });

  test('the ruler lists the main sections, is up inside them and steps aside past the marker', async ({ page }) => {
    expect(await page.locator('.moss-contents-ruler a').count()).toBe(COLLECTIONS.length);

    await jumpTo(page, 2);
    await expect.poll(() => isAway(page)).toBe(false);

    // The marker passes the middle of the window: the ruler goes, the island stays.
    await placeTop(page, '[data-contents-end]', 0.3);
    await expect.poll(() => isAway(page)).toBe(true);
    expect(await shown(page)).toBe('true');
    // The section after the marker still names itself on the island's button.
    await jumpTo(page, 5);
    expect(await nameOf(page)).toBe(AFTER_END);
    expect(await isAway(page)).toBe(true);

    // Scrolling back above the marker brings the ruler back.
    await jumpTo(page, 3);
    await expect.poll(() => isAway(page)).toBe(false);
  });

  test('the sections button stays on the island while the ruler is up', async ({ page }) => {
    await jumpTo(page, 2);
    await expect.poll(() => isAway(page)).toBe(false);
    expect(await page.locator('.moss-nav-island-sections').evaluate((el) => getComputedStyle(el).display)).not.toBe('none');
  });
});

test.describe('a headerless page with fewer than two sections', () => {
  test('shows the controls from the start, without a sections button', async ({ page }) => {
    await open(page, '/short/');
    expect(await shown(page)).toBe('true');
    await expect(island(page).locator('.nav-theme-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island-sections')).toBeHidden();
    expect(await page.locator('.moss-contents-ruler').count()).toBe(0);
  });
});

test.describe('a headerless page without JavaScript', () => {
  test.use({ javaScriptEnabled: false });

  test('still shows the island with its controls, at the top of the page', async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 800 });
    await page.goto('/');
    await expect(page.locator('.moss-nav-island .nav-theme-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island-sections')).toBeHidden();
    const box = await page.locator('.moss-nav-island').boundingBox();
    expect(box!.y).toBeLessThan(40);
  });
});

test.describe('a site that keeps its header', () => {
  test('has the header, the controls in it, and an island that is not pinned', async ({ page }) => {
    await open(page, BAND_PAGE);
    expect(await page.locator('header .main-nav').count()).toBe(1);
    expect(await page.locator('header .nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.moss-nav-island .nav-theme-btn').count()).toBe(0);
    expect(await page.locator('.moss-nav-island-trail').count()).toBe(1);
    expect(await island(page).getAttribute('data-header')).toBeNull();

    // Scrolling down hides it, as before.
    await jumpTo(page, 2);
    await page.evaluate(() => window.scrollBy(0, 400));
    await frames(page);
    expect(await shown(page)).toBe('false');
  });
});

test.describe('the ruler steps aside from a full-width block', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, BAND_PAGE);
  });

  test('while the block overlaps it, and returns once the block has passed', async ({ page }) => {
    expect(await page.locator('main [data-width="screen"]').count()).toBe(1);
    await jumpTo(page, 1);
    await expect.poll(() => isAway(page)).toBe(false);

    // The band crosses the middle of the window, where the ruler sits.
    await placeTop(page, '[data-width="screen"]', 0.4);
    await expect.poll(() => isAway(page)).toBe(true);

    // Below the band again (the band is above the window's middle): back.
    await page.evaluate(() => {
      const band = document.querySelector('[data-width="screen"]')!;
      const ol = document.querySelector('.moss-contents-ruler ol')!.getBoundingClientRect();
      window.scrollTo(0, band.getBoundingClientRect().bottom + window.scrollY - ol.top + 40);
    });
    await frames(page);
    await expect.poll(() => isAway(page)).toBe(false);

    // A heading can carry the end marker too: the ruler lists the sections
    // before it, and steps aside once it passes the middle of the window.
    const marker = page.locator('main h2[data-contents-end]');
    expect(await marker.evaluate((el) => ({ id: el.id, text: (el.textContent ?? '').replace('#', '').trim() }))).toEqual({
      id: 'afterword',
      text: 'Afterword',
    });
    expect(await page.locator('.moss-contents-ruler a').count()).toBe(BAND_SECTIONS.length - 1);
    await placeTop(page, 'main h2[data-contents-end]', 0.3);
    await expect.poll(() => isAway(page)).toBe(true);

    // And before it, above the ruler, too.
    await jumpTo(page, 0);
    await expect.poll(() => isAway(page)).toBe(false);
    expect(BAND_SECTIONS.length).toBeGreaterThan(3);
  });
});
