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

// Stands in for the comment server the site is configured with: the list is
// empty, and a post succeeds as a reply to comment 7.
const stubComments = (page: Page) =>
  page.route('https://comments.example.test/**', (route) =>
    route.request().method() === 'POST'
      ? route.fulfill({ json: { id: 99, content: '<p>A reply.</p>', nick: 'Cy', date: '2026-01-06T10:00:00Z', rid: 7 } })
      : route.fulfill({ json: { comments: [], count: 0 } }),
  );

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [1280, 320]) {
    test(`axe finds no WCAG 2.1 AA violation — ${scheme}, ${width}px`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width, height: 800 });
      await stubComments(page);
      for (const path of PAGES) {
        await page.goto(base + path);
        // axe skips what is collapsed; the comment form lives in a <details>.
        await page.evaluate(() => document.querySelectorAll('details').forEach((d) => (d.open = true)));
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

test('the actions on a keyboard selection can be reached and left from the keyboard', async ({ page }) => {
  await page.goto(base + '/notes/first/');
  await page.evaluate(() => {
    const text = document.querySelector('article p')!.firstChild!;
    const range = document.createRange();
    range.setStart(text, 2);
    range.setEnd(text, 10);
    getSelection()!.removeAllRanges();
    getSelection()!.addRange(range);
  });
  await page.keyboard.press('Shift+ArrowRight');
  const bar = page.locator('.sel-popover');
  await expect(bar).toBeVisible();
  await expect(bar).toHaveAttribute('role', 'group');

  await page.keyboard.press('Tab');
  await expect(page.locator('.sel-popover .sel-share')).toBeFocused();
  const count = await bar.locator('button').count();
  for (let i = 1; i < count; i++) await page.keyboard.press('Tab');
  await expect(bar.locator('button').last()).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(bar).toBeHidden();
  expect(await page.evaluate(() => getSelection()!.toString().length)).toBeGreaterThan(0);

  await page.keyboard.press('Shift+ArrowRight');
  await expect(bar).toBeVisible();
  await page.keyboard.press('Tab');
  await page.keyboard.press('Escape');
  await expect(bar).toBeHidden();
});

test('the underline of a comment field is its boundary, at 3:1', async ({ page }) => {
  await page.goto(base + '/notes/first/');
  await page.evaluate(() => document.querySelectorAll('details').forEach((d) => (d.open = true)));
  await installPaint(page);
  const pairs = await page.evaluate(() => {
    const { paint } = window as unknown as Painted;
    const bg = paint(getComputedStyle(document.body).backgroundColor);
    return [...document.querySelectorAll('#moss-comment-text, .comment-field')].map((el) => ({
      name: el.getAttribute('name'),
      line: paint(getComputedStyle(el).borderBottomColor),
      bg,
    }));
  });
  expect(pairs.length).toBe(4);
  for (const { name, line, bg } of pairs) {
    const ratio = contrastRatio(relativeLuminance(...(line as [number, number, number])), relativeLuminance(...(bg as [number, number, number])));
    expect(ratio, name!).toBeGreaterThanOrEqual(3);
  }
});

test('posting a reply keeps the keyboard focus and says it was sent', async ({ page }) => {
  await stubComments(page);
  await page.goto(base + '/notes/first/');
  await page.locator('summary.comments-toggle').click();
  await page.locator('.comment-reply-btn[data-reply-id="7"]').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#moss-comment-text')).toBeFocused();
  await page.keyboard.type('A reply.');
  await page.fill('#moss-comment-form [name="name"]', 'Cy');
  await page.fill('#moss-comment-form [name="email"]', 'cy@example.test');
  await page.locator('.comment-form-submit').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#comment-artalk-99')).toHaveCount(1);
  await expect(page.locator('.comment-form-submit')).toBeFocused();
  await expect(page.locator('#moss-comment-status')).toHaveText('Sent');
});
