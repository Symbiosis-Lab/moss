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
/**
 * Toggle a grouped callout and record the outer `<details>` height on every
 * frame for 800ms. The motion runs on `::details-content`, so it is the
 * details box that grows and shrinks; `before` is its height at the click.
 */
const toggle = (page: Page, i: number) =>
  grouped(page).nth(i).evaluate(async (el) => {
    const height = () => el.getBoundingClientRect().height;
    const before = height();
    (el.querySelector('summary') as HTMLElement).click();
    const samples: number[] = [];
    const start = performance.now();
    while (performance.now() - start < 800) {
      await new Promise((resolve) => requestAnimationFrame(resolve));
      samples.push(height());
    }
    return { before, samples };
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

test('the callout grows and shrinks over frames where animated details are supported', async ({ page }) => {
  await page.goto('/');
  test.skip(!(await supportsMotion(page)), 'this engine opens a details at once');
  const opening = await toggle(page, 0);
  const full = opening.samples[opening.samples.length - 1];
  expect(full).toBeGreaterThan(opening.before + 20);
  expect(opening.samples[0]).toBeLessThan(full - 1);
  expect(opening.samples.some((h) => h > opening.before + 1 && h < full - 1)).toBe(true);

  const closing = await toggle(page, 0);
  const shut = closing.samples[closing.samples.length - 1];
  expect(shut).toBeCloseTo(opening.before, 0);
  expect(closing.samples[0]).toBeGreaterThan(shut + 1);
  expect(closing.samples.some((h) => h > shut + 1 && h < full - 1)).toBe(true);
});

test('under reduced motion the full height is there on the first frame, opening and closing', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  const opening = await toggle(page, 0);
  const full = opening.samples[opening.samples.length - 1];
  expect(full).toBeGreaterThan(opening.before + 20);
  expect(opening.samples[0]).toBeCloseTo(full, 0);
  const closing = await toggle(page, 0);
  expect(closing.samples[0]).toBeCloseTo(opening.before, 0);
});
