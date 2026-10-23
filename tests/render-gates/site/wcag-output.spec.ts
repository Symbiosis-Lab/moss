// A built moss page meets WCAG 2.1 AA. The token test proves the palette's
// own pairs; it cannot see a colour mixed in a stylesheet partial, an
// attribute the renderer emits, or a layout. axe-core covers the mechanical
// criteria over the real cascade in both themes and at phone width; the
// checks after it cover what axe does not judge.
import { test, expect, type Locator, type Page } from '@playwright/test';
import { createRequire } from 'node:module';
import type { AxeResults } from 'axe-core';
import { gatePort } from '../../../playwright/gate-ports';
import { contrastRatio, relativeLuminance } from '../../e2e/helpers/wcag-contrast';

const base = `http://localhost:${gatePort('wcag-output')}`;
const PAGES = ['/', '/zh-hans/', '/notes/', '/notes/first/'];
const AXE = createRequire(import.meta.url).resolve('axe-core/axe.min.js');

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [1280, 320]) {
    test(`axe finds no WCAG 2.1 AA violation — ${scheme}, ${width}px`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width, height: 800 });
      for (const path of PAGES) {
        await page.goto(base + path);
        await page.addScriptTag({ path: AXE });
        const violations = await page.evaluate(async () => {
          const { axe } = window as unknown as { axe: { run: (ctx: Document, opts: object) => Promise<AxeResults> } };
          const result = await axe.run(document, {
            runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] },
          });
          return result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html.slice(0, 120)).join(' | ')}`);
        });
        expect(violations, `${path}`).toEqual([]);
      }
    });
  }
}

// Resolves any CSS colour — `color-mix()` reports as `color(srgb …)` — to
// sRGB bytes by painting it. Installed on the page as `window.paint`.
const installPaint = (page: Page) =>
  page.evaluate(() => {
    const ctx = document.createElement('canvas').getContext('2d')!;
    (window as unknown as { paint: (css: string) => number[] }).paint = (css) => {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = css;
      ctx.fillRect(0, 0, 1, 1);
      return Array.from(ctx.getImageData(0, 0, 1, 1).data.slice(0, 3));
    };
  });
type Painted = Window & { paint: (css: string) => number[] };

for (const scheme of ['light', 'dark'] as const) {
  test(`every callout title reads at 4.5:1 on its own band — ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto(base + '/');
    await installPaint(page);
    const callouts = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('.callout')].map((c) => ({
        type: c.dataset.type,
        ink: (window as unknown as Painted).paint(getComputedStyle(c.querySelector('.callout-title')!).color),
        band: (window as unknown as Painted).paint(getComputedStyle(c).backgroundColor),
      })),
    );
    expect(callouts.length).toBeGreaterThanOrEqual(16);
    for (const { type, ink, band } of callouts) {
      const ratio = contrastRatio(relativeLuminance(...(ink as [number, number, number])), relativeLuminance(...(band as [number, number, number])));
      expect(ratio, `${type} title ${ink} on ${band}`).toBeGreaterThanOrEqual(4.5);
    }
  });
}

// The engine's own focus outline would satisfy "has an outline"; moss's ring
// is the 2px ui-accent one.
async function expectAccentRing(page: Page, target: Locator) {
  await installPaint(page);
  const ring = await target.evaluate((el) => {
    const { paint } = window as unknown as Painted;
    const cs = getComputedStyle(el);
    return {
      width: cs.outlineWidth,
      color: paint(cs.outlineColor),
      accent: paint(getComputedStyle(document.documentElement).getPropertyValue('--moss-color-ui-accent')),
    };
  });
  expect(ring.width).toBe('2px');
  expect(ring.color).toEqual(ring.accent);
}

test('a code block too wide for a phone scrolls from the keyboard', async ({ page, browserName }) => {
  await page.setViewportSize({ width: 320, height: 800 });
  await page.goto(base + '/');
  const pre = page.locator('article pre').first();
  await pre.focus();
  await expect(pre).toBeFocused();
  await expectAccentRing(page, pre);
  // WebKit does not arrow-scroll a focused overflow box at all; focus is the
  // part moss owns.
  if (browserName === 'webkit') return;
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => pre.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
});

test('embedded media shows the focus ring', async ({ page }) => {
  await page.goto(base + '/');
  await page.evaluate(() =>
    document.querySelector('article')!.insertAdjacentHTML('afterbegin', '<video class="moss-embed" controls tabindex="0"></video>'),
  );
  const video = page.locator('article video.moss-embed');
  await video.focus();
  await expectAccentRing(page, video);
});

test('the language switch speaks each language in its own voice', async ({ page }) => {
  await page.goto(base + '/');
  const link = page.locator('.nav-lang-toggle .nav-lang-link');
  await expect(link).toHaveAttribute('lang', (await link.getAttribute('hreflang'))!);
  await expect(page.locator('.nav-lang-toggle')).toHaveAttribute('role', 'group');
});

for (const width of [1280, 320]) {
  test(`a sidebar page gives its article the width of the page — ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto(base + '/');
    const m = await page.evaluate(() => ({
      main: document.querySelector('main.has-sidebar')!.getBoundingClientRect().width,
      article: document.querySelector('main.has-sidebar > article')!.getBoundingClientRect().width,
    }));
    expect(m.main).toBeGreaterThan(width * 0.8);
    expect(m.article).toBeGreaterThan(Math.min(width * 0.6, 500));
  });
}

test('no page scrolls sideways at 320px', async ({ page }) => {
  // The 3D model's element comes from a CDN; a local stand-in with its
  // default 300px width keeps the check off the network.
  await page.route('**/model-viewer.min.js', (route) =>
    route.fulfill({
      contentType: 'text/javascript',
      body: `customElements.define('model-viewer', class extends HTMLElement {
        constructor() { super(); this.attachShadow({ mode: 'open' }).innerHTML = '<style>:host{display:inline-block;width:300px;height:150px}</style>'; }
      });`,
    }),
  );
  await page.setViewportSize({ width: 320, height: 800 });
  for (const path of PAGES) {
    await page.goto(base + path);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow, path).toBeLessThanOrEqual(0);
  }
});

test('a link inside a footer sentence is underlined, not told apart by colour alone', async ({ page }) => {
  await page.goto(base + '/');
  const link = page.locator('footer.container p a', { hasText: 'an editor' });
  expect(await link.evaluate((el) => getComputedStyle(el).textDecorationLine)).toContain('underline');
});
