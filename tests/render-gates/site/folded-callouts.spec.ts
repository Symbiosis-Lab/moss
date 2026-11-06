/**
 * Folded callouts: opening one in an `::: {accordion}` group closes the
 * others, a callout outside the group is independent, and the answer's height
 * animates where the engine supports it (and not under reduced motion).
 *
 * Fixture: playwright/folded-callouts.config.ts → tests/e2e/helpers/folded-callouts-site.ts
 * Run via: bash scripts/render-gates.sh folded-callouts
 */
import { test, expect, type Page } from '@playwright/test';

const grouped = (page: Page) => page.locator('.moss-accordion details.callout');
const supportsMotion = (page: Page) =>
  page.evaluate(() => CSS.supports('interpolate-size: allow-keywords') && CSS.supports('selector(::details-content)'));
const answerHeight = (page: Page, i: number) =>
  grouped(page).nth(i).evaluate((el) => {
    const content = el.querySelector('.callout-content') as HTMLElement;
    return content.getBoundingClientRect().height;
  });

test('opening one callout in a group closes the other', async ({ page }) => {
  await page.goto('/');
  const [first, second] = [grouped(page).nth(0), grouped(page).nth(1)];
  expect(await first.getAttribute('name')).toBeTruthy();
  expect(await first.getAttribute('name')).toBe(await second.getAttribute('name'));
  await first.locator('summary').click();
  await expect(first).toHaveAttribute('open', '');
  await second.locator('summary').click();
  await expect(second).toHaveAttribute('open', '');
  await expect(first).not.toHaveAttribute('open', '');
});

test('a folded callout outside the group is independent of it', async ({ page }) => {
  await page.goto('/');
  const outside = page.locator('details.callout:not(.moss-accordion details)').first();
  expect(await outside.getAttribute('name')).toBeNull();
  await grouped(page).nth(0).locator('summary').click();
  await outside.locator('summary').click();
  await expect(grouped(page).nth(0)).toHaveAttribute('open', '');
  await expect(outside).toHaveAttribute('open', '');
});

test('the answer grows over the token duration where animated details are supported', async ({ page }) => {
  await page.goto('/');
  test.skip(!(await supportsMotion(page)), 'this engine opens a details at once');
  const details = grouped(page).nth(0);
  const full = await (async () => {
    await details.locator('summary').click();
    await page.waitForTimeout(700);
    return answerHeight(page, 0);
  })();
  await details.locator('summary').click();
  await page.waitForTimeout(700);
  await details.locator('summary').click();
  await page.waitForTimeout(120);
  const mid = await answerHeight(page, 0);
  expect(mid).toBeGreaterThan(0);
  expect(mid).toBeLessThan(full);
  await page.waitForTimeout(600);
  expect(await answerHeight(page, 0)).toBeCloseTo(full, 0);
});

test('under reduced motion the answer opens at once', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  const details = grouped(page).nth(0);
  await details.locator('summary').click();
  await page.waitForTimeout(50);
  const h = await answerHeight(page, 0);
  await page.waitForTimeout(600);
  expect(h).toBeCloseTo(await answerHeight(page, 0), 0);
  expect(await details.evaluate((el) => getComputedStyle(el, '::details-content').transitionDuration)).not.toContain('0.4s');
});
