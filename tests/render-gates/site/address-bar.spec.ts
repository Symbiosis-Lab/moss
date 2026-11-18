/**
 * The preview's address bar in the editor `moss edit --serve` serves, in a real
 * browser. Typing in it lists the site's pages. The arrow keys move a visible
 * highlight through the list and the field names the highlighted row for
 * assistive technology; Enter opens that row and Escape closes the list. A
 * wheel over the list scrolls the list, and never the previewed page under it.
 *
 * Ports: playwright/gate-ports.ts, keys `address-bar:ui` / `address-bar:preview`.
 * Config: playwright/address-bar.config.ts. With SHOTS set to a directory, the
 * highlighted row is photographed there in the light and dark schemes.
 */
import { test, expect, type Frame, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { openShell as openEditServe, useEditServe } from '../../e2e/helpers/edit-serve';

// Enough pages that the list overflows its height, and a home page long enough to scroll.
const PAGES = Array.from({ length: 24 }, (_, n) => `Room ${String(n + 1).padStart(2, '0')}`);
const HOME = ['---', 'title: Garden', '---', 'Opening words.', '', ...Array.from({ length: 80 }, (_, n) => `Paragraph ${n + 1} of the garden.\n`)].join('\n');

const serve = useEditServe('address-bar', (site) => {
  writeFileSync(join(site, 'index.md'), HOME);
  for (const title of PAGES) writeFileSync(join(site, `${title.toLowerCase().replace(' ', '-')}.md`), `---\ntitle: ${title}\n---\n${title} words.\n`);
});

const openShell = (page: Page) => openEditServe(page, serve, 'Opening words', { width: 1280, height: 800 });

/** Opens the list and waits for every page in it. */
async function openList(shell: Frame): Promise<void> {
  await shell.dblclick('#moss-url-text');
  await expect.poll(() => shell.locator('.moss-url-autocomplete-item').count(), { timeout: 15_000 }).toBeGreaterThan(PAGES.length);
}

/** The highlighted row's index, the row the field names, and each row's painted background. */
const state = (shell: Frame) => shell.evaluate(() => {
  const rows = [...document.querySelectorAll<HTMLElement>('.moss-url-autocomplete-item')];
  const input = document.querySelector<HTMLInputElement>('.moss-url-autocomplete-input')!;
  return {
    selected: rows.flatMap((r, i) => (r.getAttribute('aria-selected') === 'true' ? [i] : [])),
    named: rows.findIndex((r) => r.id === input.getAttribute('aria-activedescendant')),
    paint: rows.map((r) => getComputedStyle(r).backgroundColor),
  };
});

test('the arrow keys move a visible highlight, Enter opens it and Escape closes the list', async ({ page, browserName }) => {
  const { shell, preview } = await openShell(page);
  await openList(shell);
  // The pointer rests away from the list, so no row is under it.
  await page.mouse.move(5, 795);
  const input = shell.locator('.moss-url-autocomplete-input');

  await input.press('ArrowDown');
  await input.press('ArrowDown');
  let now = await state(shell);
  expect(now.selected, 'the highlight after two presses of Down').toEqual([2]);
  expect(now.named, 'the field names the highlighted row').toBe(2);
  expect(now.paint[2], 'the highlighted row is painted unlike the rest').not.toBe(now.paint[0]);
  expect(now.paint[0]).toBe(now.paint[1]);
  const dir = process.env.SHOTS;
  const shoot = async (name: string) => {
    if (!dir) return;
    mkdirSync(dir, { recursive: true });
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(dir, `address-bar-${name}-${browserName}-${scheme}.png`), clip: { x: 0, y: 0, width: 1280, height: 520 } });
    }
    await page.emulateMedia({ colorScheme: 'light' });
  };
  await shoot('highlight');

  // Far down the overflowing list, the highlighted row is scrolled into the list's view.
  for (let n = 0; n < 14; n++) await input.press('ArrowDown');
  const inView = await shell.evaluate(() => {
    const row = document.querySelector('.moss-url-autocomplete-item[aria-selected="true"]')!.getBoundingClientRect();
    const list = document.querySelector('.moss-url-autocomplete-dropdown')!.getBoundingClientRect();
    return row.top >= list.top - 1 && row.bottom <= list.bottom + 1;
  });
  expect(inView, 'the highlighted row is inside the visible list').toBe(true);
  await shoot('scrolled');
  for (let n = 0; n < 14; n++) await input.press('ArrowUp');

  await input.press('ArrowUp');
  now = await state(shell);
  expect(now.selected, 'the highlight after Up').toEqual([1]);
  expect(now.named).toBe(1);

  const path = await shell.locator('.moss-url-autocomplete-item').nth(1).locator('.moss-url-autocomplete-path').innerText();
  await input.press('Enter');
  // The preview serves a page's folder address from its index.html.
  const opened = () => preview.evaluate(() => location.pathname.replace(/index\.html$/, '')).catch(() => '');
  await expect.poll(opened, { timeout: 15_000, message: 'Enter opens the highlighted page' }).toBe(path.endsWith('/') ? path : `${path}/`);

  // The field now holds the opened page's address, which narrows the list to it.
  await shell.dblclick('#moss-url-text');
  await expect(shell.locator('.moss-url-autocomplete-item').first()).toBeVisible();
  await shell.locator('.moss-url-autocomplete-input').press('Escape');
  await expect(shell.locator('.moss-url-autocomplete-container')).toBeHidden();
});

test('a wheel over the list scrolls the list and never the page under it', async ({ page }) => {
  const { shell, preview } = await openShell(page);
  await openList(shell);
  const list = shell.locator('.moss-url-autocomplete-dropdown');
  const box = (await list.boundingBox())!;
  const listTop = () => list.evaluate((l) => l.scrollTop);
  const pageTop = () => preview.evaluate(() => Math.round(scrollY));
  expect(await list.evaluate((l) => l.scrollHeight > l.clientHeight), 'premise: the list overflows').toBe(true);

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, 120);
  await expect.poll(listTop, { timeout: 5_000, message: 'the list scrolled' }).toBeGreaterThan(0);
  // Past the list's end the wheel must stop there, not pass on to the page.
  for (let n = 0; n < 10; n++) await page.mouse.wheel(0, 400);
  await page.waitForTimeout(500);
  expect(await list.evaluate((l) => Math.ceil(l.scrollTop + l.clientHeight) >= l.scrollHeight), 'the list reached its end').toBe(true);
  expect(await pageTop(), 'the wheel over the list scrolled the previewed page').toBe(0);
});
