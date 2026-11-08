/**
 * `[site].header_scroll = "hide"`: the header stays at the top, slides out of
 * view while the reader scrolls down past it, returns on any scroll up, and
 * never covers a heading reached by a link.
 *
 * Fixture: playwright/header-scroll.config.ts → tests/e2e/helpers/header-scroll-site.ts
 * Run via: bash scripts/render-gates.sh header-scroll
 */
import { test, expect, type Page } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';
import { COVER_PATH, TARGET_ID } from '../../e2e/helpers/header-scroll-site';

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

async function open(page: Page, path = '/'): Promise<void> {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto(path);
  await page.evaluate(() => document.fonts.ready);
  await frames(page);
}

async function scrollTo(page: Page, y: number): Promise<void> {
  await page.evaluate((target) => window.scrollTo(0, target), y);
  // The transition is 0.2s; wait it out so the box is where it will rest.
  await page.waitForTimeout(350);
}

const header = (page: Page) => page.locator('body > header');
const hidden = (page: Page) => header(page).evaluate((el) => el.hasAttribute('data-hidden'));
/** The header's bottom edge in the window: <= 0 once it has slid away. */
const bottom = (page: Page) => header(page).evaluate((el) => el.getBoundingClientRect().bottom);

test.describe('header_scroll = "hide"', () => {
  test('is marked, sticky, and measured', async ({ page }) => {
    await open(page);
    await expect(page.locator('body')).toHaveAttribute('data-header-scroll', 'hide');
    expect(await header(page).evaluate((el) => getComputedStyle(el).position)).toBe('sticky');
    const height = await header(page).evaluate((el) => (el as HTMLElement).offsetHeight);
    const measured = await page.evaluate(() => document.documentElement.style.getPropertyValue('--moss-header-height'));
    expect(measured).toBe(`${height}px`);
  });

  test('slides away on scrolling down past it and returns on any scroll up', async ({ page }) => {
    await open(page);
    expect(await bottom(page)).toBeGreaterThan(0);
    await scrollTo(page, 900);
    expect(await hidden(page)).toBe(true);
    expect(await bottom(page)).toBeLessThanOrEqual(0);
    await scrollTo(page, 880);
    expect(await hidden(page)).toBe(false);
    expect(await bottom(page)).toBeGreaterThan(0);
  });

  test('slides rather than jumps', async ({ page }) => {
    await open(page);
    await page.mouse.wheel(0, 900);
    await expect.poll(() => hidden(page)).toBe(true);
    const sliding = await header(page).evaluate((el) =>
      el.getAnimations().some((a) => (a as CSSTransition).transitionProperty === 'transform'),
    );
    expect(sliding).toBe(true);
  });

  test('ignores a movement of five pixels or fewer', async ({ page }) => {
    await open(page);
    await scrollTo(page, 900);
    expect(await hidden(page)).toBe(true);
    await scrollTo(page, 896);
    expect(await hidden(page)).toBe(true);
  });

  test('is shown near the top of the page', async ({ page }) => {
    await open(page);
    await scrollTo(page, 900);
    await scrollTo(page, 910);
    await scrollTo(page, 20);
    expect(await hidden(page)).toBe(false);
  });

  test('stays while focus is inside it, and a Tab into it brings it back', async ({ page }) => {
    await open(page);
    await scrollTo(page, 900);
    expect(await hidden(page)).toBe(true);
    await header(page).locator('a').first().focus();
    await frames(page);
    expect(await hidden(page)).toBe(false);
    await scrollTo(page, 1200);
    expect(await hidden(page)).toBe(false);
  });

  test('stays under reduced motion', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await open(page);
    await scrollTo(page, 900);
    expect(await hidden(page)).toBe(false);
    expect(await bottom(page)).toBeGreaterThan(0);
  });

  test('a heading reached by its link clears the header, hidden or not', async ({ page }) => {
    await open(page);
    await page.getByRole('link', { name: 'Jump to salt' }).click();
    await page.waitForTimeout(400);
    const { top, height } = await page.evaluate((id) => ({
      top: document.getElementById(id)!.getBoundingClientRect().top,
      height: (document.querySelector('body > header') as HTMLElement).offsetHeight,
    }), TARGET_ID);
    // The header slid away on the way down, but it returns on the next scroll
    // up, so the heading is parked below where it would be.
    expect(top).toBeGreaterThanOrEqual(height);
  });

  test('the island bar does not repeat the header', async ({ page }) => {
    await open(page);
    const bar = page.locator('.moss-nav-island-bar');
    if ((await bar.count()) > 0) {
      expect(await bar.evaluate((el) => getComputedStyle(el).display)).toBe('none');
    }
  });
});

test.describe('a header laid over the opening hero', () => {
  test('is not kept, and the island keeps its bar', async ({ page }) => {
    await open(page, COVER_PATH);
    await expect(page.locator('body')).not.toHaveAttribute('data-header-scroll', /.*/);
    expect(await header(page).evaluate((el) => getComputedStyle(el).position)).not.toBe('sticky');
    await scrollTo(page, 1400);
    await scrollTo(page, 1300);
    const bar = page.locator('.moss-nav-island-bar');
    expect(await bar.count()).toBeGreaterThan(0);
    expect(await bar.evaluate((el) => getComputedStyle(el).display)).not.toBe('none');
  });
});

test.describe('the open phone menu', () => {
  test('keeps the header in view while the page scrolls down', async ({ page }) => {
    await page.setViewportSize({ width: 300, height: 700 });
    await page.goto('/');
    await page.evaluate(() => document.fonts.ready);
    await frames(page);
    await page.locator('.mobile-menu-button').click();
    await expect(page.locator('.nav-links')).toHaveClass(/mobile-open/);
    await scrollTo(page, 900);
    await scrollTo(page, 1300);
    expect(await hidden(page)).toBe(false);
    expect(await bottom(page)).toBeGreaterThan(0);
  });
});

test.describe('header_scroll = "sticky" (twin site)', () => {
  test.use({ baseURL: `http://localhost:${gatePort('header-scroll:sticky')}/` });

  test('stays at the top and never slides away', async ({ page }) => {
    await open(page);
    await expect(page.locator('body')).toHaveAttribute('data-header-scroll', 'sticky');
    await scrollTo(page, 900);
    await scrollTo(page, 1400);
    expect(await hidden(page)).toBe(false);
    expect(await bottom(page)).toBeGreaterThan(0);
  });
});
