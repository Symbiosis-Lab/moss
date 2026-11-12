/**
 * `chrome: none`: the page has no header, island, footer or skip link, and
 * its controls stand at the end of the title's first line, clear of the
 * title, at desktop and phone widths. One property takes them and their
 * room away. A sibling page keeps the frame.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → PAGE_CHROME_NONE_GATE.
 * Run via: bash scripts/render-gates.sh page-chrome-none
 */
import { test, expect, type Page } from '@playwright/test';

async function open(page: Page, url: string, width: number): Promise<void> {
  await page.setViewportSize({ width, height: 800 });
  await page.goto(url);
  await page.evaluate(() => document.fonts.ready);
}

for (const width of [1280, 390]) {
  test(`the controls stand beside the title's first line at ${width}`, async ({ page }) => {
    await open(page, '/landing/', width);
    for (const gone of ['header', '.moss-nav-island', '.moss-skip-link']) {
      expect(await page.locator(gone).count(), gone).toBe(0);
    }
    await expect(page.locator('body')).not.toContainText('Write to us.');
    await expect(page.locator('.moss-colophon')).toBeVisible();

    const controls = page.locator('body > .nav-icons');
    await expect(controls).toBeVisible();
    const { box, column, lines, firstLine } = await page.evaluate(() => {
      const rect = (el: Element) => { const r = el.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, right: r.right }; };
      const h1 = document.querySelector('main h1')!;
      const range = document.createRange();
      range.selectNodeContents(h1);
      const lineRects = [...range.getClientRects()];
      const first = lineRects.filter((r) => Math.abs(r.top - lineRects[0].top) < 2);
      return {
        box: rect(document.querySelector('body > .nav-icons')!),
        column: rect(document.querySelector('main > article')!),
        lines: new Set(lineRects.map((r) => Math.round(r.top))).size,
        firstLine: { top: first[0].top, bottom: first[0].bottom, right: Math.max(...first.map((r) => r.right)) },
      };
    });
    expect(lines, 'the title is long enough to wrap').toBeGreaterThan(1);
    // In the window, inside the text column's end, on the first line's band.
    expect(box.top).toBeGreaterThanOrEqual(0);
    expect(box.right).toBeLessThanOrEqual(column.right + 1);
    expect(box.left).toBeGreaterThan(column.left);
    expect(box.top).toBeLessThan(firstLine.bottom);
    expect(box.bottom).toBeGreaterThan(firstLine.top);
    // The title's first line stops before the controls.
    expect(firstLine.right).toBeLessThanOrEqual(box.left);
  });
}

test('a theme that takes the controls away gives the title its whole line', async ({ page }) => {
  await open(page, '/landing/', 1280);
  // Narrow words, so the first line fills whatever room it is given.
  await page.locator('main h1').evaluate((h1) => { h1.textContent = 'i '.repeat(300); });
  const columnEnd = await page.locator('main h1').evaluate((h1) => h1.getBoundingClientRect().right);
  const firstLineEnd = () => page.evaluate(() => {
    const range = document.createRange();
    range.selectNodeContents(document.querySelector('main h1')!);
    const rects = [...range.getClientRects()];
    return Math.max(...rects.filter((r) => Math.abs(r.top - rects[0].top) < 2).map((r) => r.right));
  });
  const controlsStart = (await page.locator('body > .nav-icons').boundingBox())!.x;
  expect(await firstLineEnd()).toBeLessThanOrEqual(controlsStart);

  await page.addStyleTag({ content: 'body[data-chrome="none"] { --moss-controls-display: none; }' });
  await expect(page.locator('body > .nav-icons')).toBeHidden();
  expect(await firstLineEnd(), 'the first line reaches the end of the column').toBeGreaterThan(columnEnd - 40);
});

test('the theme toggle works on a page without the frame', async ({ page }) => {
  await open(page, '/landing/', 1280);
  const before = await page.evaluate(() => document.documentElement.dataset.theme);
  await page.locator('body > .nav-icons .nav-theme-btn').click();
  expect(await page.evaluate(() => document.documentElement.dataset.theme)).not.toBe(before);
});

test('a sibling page keeps the site frame', async ({ page }) => {
  await open(page, '/about/', 1280);
  await expect(page.locator('header .main-nav')).toBeVisible();
  await expect(page.locator('.moss-skip-link')).toHaveCount(1);
  expect(await page.locator('body > .nav-icons').count()).toBe(0);
  expect(await page.evaluate(() => document.body.dataset.chrome)).toBeUndefined();
});
