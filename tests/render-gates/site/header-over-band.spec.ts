/**
 * `::: {screen header=over}` on the band that opens a page: the header lies
 * over the band's pictures with no band of its own, reads at WCAG AA over each
 * ink the build chose, and lets the pointer through to the band's links.
 *
 * Fixture: playwright/header-over-band.config.ts → tests/e2e/helpers/header-over-band-site.ts
 * Run via: bash scripts/render-gates.sh header-over-band
 */
import { test, expect } from '@playwright/test';
import { probes } from '../../e2e/helpers/header-ink-probe';
import { BAND_PAGES } from '../../e2e/helpers/header-over-band-site';
import { contrastRatio, luminanceOfCss } from '../../e2e/helpers/wcag-contrast';

const BAND = 'article.container > [data-width="screen"][data-header="over"]';

for (const { path, name, ink } of BAND_PAGES) {
  for (const width of [390, 1280]) {
    test(`${name} band: header lies over the pictures and reads at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(path);
      const band = page.locator(BAND);
      if (ink === 'none') await expect(band).not.toHaveAttribute('data-header-ink', /.+/);
      else await expect(band).toHaveAttribute('data-header-ink', ink);

      const header = (await page.locator('body > header').boundingBox())!;
      const box = (await band.boundingBox())!;
      const picture = (await band.locator('img').first().boundingBox())!;
      expect(box.y, 'no gap above the band').toBeLessThanOrEqual(0.5);
      expect(picture.y, 'the pictures start at the page top').toBeLessThanOrEqual(0.5);
      expect(header.y).toBeLessThanOrEqual(0.5);
      expect(header.y + header.height, 'the header ends over the first picture').toBeLessThan(picture.y + picture.height);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);

      for (const p of await probes(page)) {
        const text = luminanceOfCss(p.color);
        const worst = Math.min(contrastRatio(text, p.lightest), contrastRatio(text, p.darkest));
        expect(worst, `"${p.label}" ${p.color} over ground luminance ${p.darkest.toFixed(3)}..${p.lightest.toFixed(3)}`).toBeGreaterThanOrEqual(4.5);
      }
    });

    test(`${name} band: the header passes the pointer to the band's link at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(path);
      const hit = await page.evaluate((bandSel) => {
        const link = document.querySelector(`${bandSel} a`)!.getBoundingClientRect();
        const controls = [...document.querySelectorAll('header a, header button')].map((el) => el.getBoundingClientRect());
        const y = Math.min(48, link.bottom - 2);
        for (let x = link.left + 4; x < link.right - 4; x += 6) {
          if (controls.some((r) => x >= r.left - 4 && x <= r.right + 4 && y >= r.top - 4 && y <= r.bottom + 4)) continue;
          const el = document.elementFromPoint(x, y)!;
          return { inHeader: !!el.closest('header'), link: el.closest('a')?.getAttribute('href') ?? null, y, x };
        }
        return null;
      }, BAND);
      expect(hit, 'a point on the linked picture clear of the header controls').not.toBeNull();
      expect(hit!.inHeader, 'the header does not swallow the pointer').toBe(false);
      expect(hit!.link).toMatch(/about/);
      await page.mouse.click(hit!.x, hit!.y);
      await expect.poll(() => new URL(page.url()).pathname).toBe('/about/');
    });
  }
}

test('the header still takes the pointer on its own links', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  const link = page.locator('header .nav-links a').first();
  const box = (await link.boundingBox())!;
  const top = await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.closest('a')?.getAttribute('href'), [box.x + box.width / 2, box.y + box.height / 2]);
  expect(top).toBe(await link.getAttribute('href'));
});

test('a band that does not open the page keeps the header in its own band', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/later/');
  await expect(page.locator('[data-header="over"]')).toHaveCount(0);
  const header = (await page.locator('body > header').boundingBox())!;
  expect(header.height).toBeGreaterThan(0);
  expect(await page.locator('body > header').evaluate((el) => getComputedStyle(el).position)).not.toBe('absolute');
});

const CONTROLS = 'header a, header button, header input, header select, header summary, header [role="button"], header [tabindex]:not([tabindex="-1"])';

/** Every visible header control is what the pointer finds at its centre. */
async function unreachableControls(page: import('@playwright/test').Page): Promise<string[]> {
  return page.evaluate((sel) => {
    const missed: string[] = [];
    for (const el of document.querySelectorAll(sel)) {
      const r = el.getBoundingClientRect();
      const visible = r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden';
      if (!visible || el.closest('[inert]')) continue;
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      if (!hit || !(el === hit || el.contains(hit) || hit.contains(el))) missed.push(`${el.tagName}.${el.className} -> ${hit?.tagName}.${hit?.className}`);
    }
    return missed;
  }, CONTROLS);
}

// The header takes the pointer on every control it holds, over a hero and over a band, closed and with the phone menu open.
for (const { path, what } of [{ path: '/hero/', what: 'hero' }, { path: '/', what: 'band' }]) {
  for (const width of [390, 1280]) {
    test(`every header control over an opening ${what} takes the pointer at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(path);
      await expect(page.locator('body > header')).toHaveAttribute('data-header', 'over');
      expect(await unreachableControls(page)).toEqual([]);

      const menu = page.locator('header .mobile-menu-button');
      if (await menu.isVisible()) {
        await menu.click();
        await expect(page.locator('header .nav-links')).toHaveClass(/mobile-open/);
        expect(await unreachableControls(page)).toEqual([]);
        await menu.click();
      }

      const theme = page.locator('header .nav-theme-btn');
      const before = await page.locator('html').getAttribute('data-theme');
      await theme.click();
      await expect.poll(() => page.locator('html').getAttribute('data-theme')).not.toBe(before);

      if (await menu.isVisible()) await menu.click();
      await page.locator('header .nav-links a').first().click();
      await expect.poll(() => new URL(page.url()).pathname).not.toBe(path);
    });
  }
}
