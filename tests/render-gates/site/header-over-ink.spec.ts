/**
 * A site header laid over a hero picture reads at WCAG AA (4.5:1) on a pale,
 * a dark and a mid-tone picture, on a phone and a desktop, in both engines.
 *
 * Fixture: playwright/header-over-ink.config.ts → tests/e2e/helpers/header-over-ink-site.ts
 * Run via: bash scripts/render-gates.sh header-over-ink
 */
import { test, expect } from '../../support/browser-test';
import { probes } from '../../e2e/helpers/header-ink-probe';
import { PAGES } from '../../e2e/helpers/header-over-ink-site';
import { contrastRatio, luminanceOfCss } from '../../e2e/helpers/wcag-contrast';

for (const { path, name, ink } of PAGES) {
  for (const width of [390, 1280]) {
    test(`${name} picture: header text clears 4.5:1 at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(path);
      const hero = page.locator('.moss-hero[data-header="over"]');
      if (ink === 'none') await expect(hero).not.toHaveAttribute('data-header-ink', /.+/);
      else await expect(hero).toHaveAttribute('data-header-ink', ink);

      for (const p of await probes(page)) {
        const text = luminanceOfCss(p.color);
        const worst = Math.min(contrastRatio(text, p.lightest), contrastRatio(text, p.darkest));
        expect(worst, `"${p.label}" ${p.color} over ground luminance ${p.darkest.toFixed(3)}..${p.lightest.toFixed(3)}`).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
}

test('in the dark theme the pale picture takes its night ink and still clears 4.5:1', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('.moss-hero[data-header="over"]')).toHaveAttribute('data-header-ink-night', 'dark');
  for (const p of await probes(page)) {
    const text = luminanceOfCss(p.color);
    expect(Math.min(contrastRatio(text, p.lightest), contrastRatio(text, p.darkest)), p.label).toBeGreaterThanOrEqual(4.5);
  }
});

test('a dark picture keeps white header ink', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/dusk/');
  const color = await page.locator('header .site-name').evaluate((el) => getComputedStyle(el).color);
  expect(luminanceOfCss(color)).toBeGreaterThan(0.99);
});


// Words at the top of a picture the header is laid over start below the header.
for (const width of [390, 900, 1280]) {
  test(`align=top words start below the header at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/top/');
    const header = (await page.locator('body > header').boundingBox())!;
    const words = (await page.locator('.moss-hero[data-header="over"] .moss-hero-content').boundingBox())!;
    expect(words.y, `words at ${words.y}, header ends at ${header.y + header.height}`).toBeGreaterThanOrEqual(header.y + header.height - 0.5);
  });
}

test('a picture in footer.md wider than a phone fits the page', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto('/');
  const { page: wide, client, picture } = await page.evaluate(() => ({
    page: document.documentElement.scrollWidth,
    client: document.documentElement.clientWidth,
    picture: document.querySelector('footer img[src*="mark"]')!.getBoundingClientRect().right,
  }));
  expect(wide).toBe(client);
  expect(picture).toBeLessThanOrEqual(client);
});
