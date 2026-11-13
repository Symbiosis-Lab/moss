/**
 * A site header laid over a hero picture reads at WCAG AA (4.5:1) on a pale,
 * a dark and a mid-tone picture, on a phone and a desktop, in both engines.
 *
 * The ground under each piece of header text is the page as painted with the
 * text made transparent, so it includes the picture and the wash; the text's
 * computed colour is compared to the worst pixel under its box.
 *
 * Fixture: playwright/header-over-ink.config.ts → tests/e2e/helpers/header-over-ink-site.ts
 * Run via: bash scripts/render-gates.sh header-over-ink
 */
import { test, expect, type Page } from '@playwright/test';
import { PAGES } from '../../e2e/helpers/header-over-ink-site';
import { contrastRatio, luminanceOfCss } from '../../e2e/helpers/wcag-contrast';

const TEXT = 'header .site-name, header .nav-links a';

interface Probe {
  label: string;
  color: string;
  lightest: number;
  darkest: number;
}

async function probes(page: Page): Promise<Probe[]> {
  await page.evaluate(() => document.fonts.ready);
  // The opening hero's picture loads eagerly; the footer's lazy ones may never load in view.
  await page.waitForFunction(() => [...document.images].filter((i) => i.loading !== 'lazy').every((i) => i.complete && i.naturalWidth > 0));
  const items = await page.$$eval(TEXT, (els) =>
    els
      .map((el) => {
        const r = el.getBoundingClientRect();
        return { label: (el.textContent ?? '').trim(), color: getComputedStyle(el).color, x: r.x, y: r.y, w: r.width, h: r.height };
      })
      .filter((i) => i.w > 0 && i.h > 0),
  );
  expect(items.length, 'the header has a site name and nav links').toBeGreaterThan(1);
  await page.addStyleTag({ content: 'header * { color: transparent !important; text-shadow: none !important; }' });
  const out: Probe[] = [];
  for (const item of items) {
    const shot = await page.screenshot({ clip: { x: Math.floor(item.x), y: Math.floor(item.y), width: Math.ceil(item.w), height: Math.ceil(item.h) } });
    const [lightest, darkest] = await page.evaluate(async (b64) => {
      const bitmap = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
      const canvas = document.createElement('canvas');
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d')!;
      ctx.drawImage(bitmap, 0, 0);
      const px = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      const lum = (c: number) => ((c /= 255) <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
      let hi = 0;
      let lo = 1;
      for (let i = 0; i < px.length; i += 4) {
        const l = 0.2126 * lum(px[i]) + 0.7152 * lum(px[i + 1]) + 0.0722 * lum(px[i + 2]);
        hi = Math.max(hi, l);
        lo = Math.min(lo, l);
      }
      return [hi, lo];
    }, shot.toString('base64'));
    out.push({ label: item.label, color: item.color, lightest, darkest });
  }
  return out;
}

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
