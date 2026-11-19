/**
 * A link card's picture slot closes up only when the card is known to have no
 * picture: its fetched page names none. A card not fetched yet, or whose
 * picture did not download, keeps the cover's box with a hairline edge, and in
 * the preview the live grid fills it.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → LINK_CARD_COVER_SLOT_GATE. Grids 0-2
 * hold one card each; grid 3 holds all three, grid 4 the same with
 * `{rows=equal}`. In a shared grid the no-picture card keeps an empty box
 * the size of its siblings', so the words in the row start below one line.
 */
import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { openCrateDir } from '../../support/crate-paths';

const BRIDGE_JS = fs.readFileSync(
  path.join(openCrateDir('moss-build'), 'src/ops/serve/js/iframe-bridge.js'), 'utf8');

type Slot = {
  grid: number;
  title: string;
  missing: boolean;
  noCover: boolean;
  height: number;
  edged: boolean;
  grid_canvas: boolean;
};

async function slots(page: Page): Promise<Slot[]> {
  return page.$$eval('.moss-grid', (grids) =>
    grids.flatMap((grid, i) =>
      Array.from(grid.querySelectorAll<HTMLElement>(':scope > .moss-card')).map((card) => {
        const cover = card.querySelector<HTMLElement>('.moss-card-cover')!;
        return {
          grid: i,
          title: card.querySelector('.moss-card-title')?.textContent?.trim() ?? '',
          missing: cover.getAttribute('data-cover') === 'missing',
          noCover: cover.classList.contains('moss-card-no-cover'),
          height: Math.round(cover.getBoundingClientRect().height),
          edged: getComputedStyle(cover).boxShadow.includes('inset'),
          grid_canvas: cover.querySelector('.moss-blueprint-fallback canvas') !== null,
        };
      }),
    ),
  );
}

function expectOnlyNoPictureClosed(all: Slot[]) {
  const alone = all.filter((s) => s.grid < 3);
  expect(alone.map((s) => s.title)).toEqual(['not-fetched.invalid', 'No Picture', 'Not Downloaded']);
  for (const s of alone) {
    if (s.title === 'No Picture') {
      expect(s, 'a card whose page names no picture closes up').toMatchObject({ noCover: true, missing: false, height: 0 });
    } else {
      expect(s, 'a card whose picture is not in hand keeps an edged box').toMatchObject({ missing: true, noCover: false, edged: true });
      expect(s.height).toBeGreaterThan(100);
    }
  }
  for (const grid of [3, 4]) {
    const row = all.filter((s) => s.grid === grid);
    expect(row.filter((s) => s.missing).length, `grid ${grid}`).toBe(2);
    expect(new Set(row.map((s) => s.height)).size, `grid ${grid}: one box height ${JSON.stringify(row)}`).toBe(1);
    expect(row[0].height).toBeGreaterThan(100);
  }
}

for (const colorScheme of ['light', 'dark'] as const) {
  test.describe(`${colorScheme}`, () => {
    test.use({ colorScheme, viewport: { width: 1280, height: 2400 } });

    test('built page: only the card with no picture closes up', async ({ page }) => {
      await page.goto('/');
      await expect(page.locator('html')).toHaveAttribute('data-theme', colorScheme);
      const all = await slots(page);
      expectOnlyNoPictureClosed(all);
      expect(all.some((s) => s.grid_canvas), 'a published page carries no live grid').toBe(false);
    });

    test('preview: the same, with the live grid in each missing slot', async ({ page }) => {
      await page.goto('/');
      await page.addScriptTag({ content: BRIDGE_JS });
      await expect(page.locator('.moss-card-cover[data-cover="missing"] .moss-blueprint-fallback canvas').first()).toBeAttached();
      const all = await slots(page);
      expectOnlyNoPictureClosed(all);
      for (const s of all) expect(s.grid_canvas, `${s.title} in grid ${s.grid}`).toBe(s.missing);
    });
  });
}
