// @ts-check
// `.moss-grid`'s own block-axis margin: bound tight (sm) to a heading that
// owns it (site.css ~5989, 6322371133, 2026-07-30, unrelated to this fix and
// unaffected by it), bound loose otherwise. 3bb545d0 (2026-08-03) took both
// sides from lg to md; this restores only the bottom edge to lg — the grid's
// trailing edge is still a real section break — leaving the heading-bound top
// and every .moss-gallery value exactly as 3bb545d0 left them.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { tokenBlock } from './tokens-block.js';
import { openCrateDir } from '../../support/crate-paths';

const CSS = fs.readFileSync(
  path.join(openCrateDir('moss-build'), 'src/assets/css/site.css'), 'utf8');
const TOKENS = `:root{\n${tokenBlock('light')}\n}`;

const grid = () =>
  '<div class="moss-grid" data-columns="1"><a class="moss-card" href="#">'
  + '<div class="moss-card-content"><span class="moss-card-title">A</span></div></a></div>';

function page(before) {
  return `<!doctype html><html><head><meta charset="utf-8">
<style>${TOKENS}</style><style>${CSS}</style>
</head><body><article class="container">${before}${grid()}</article></body></html>`;
}

async function marginBlock(pg) {
  return pg.locator('.moss-grid').evaluate((el) => {
    const s = getComputedStyle(el);
    return { start: s.marginBlockStart, end: s.marginBlockEnd };
  });
}

test('a grid with no owning heading keeps the section-break margin below it', async ({ page: pg }) => {
  await pg.setContent(page('<p>Prose.</p>'));
  const m = await marginBlock(pg);
  expect(m.start).toBe('24px'); // --moss-space-md, unchanged
  expect(m.end).toBe('32px');   // --moss-space-lg, this fix
});

test('a grid bound to its heading keeps the tight top and the section-break bottom', async ({ page: pg }) => {
  await pg.setContent(page('<h2>Heading</h2>'));
  const m = await marginBlock(pg);
  expect(m.start).toBe('16px'); // --moss-space-sm, the pre-existing heading-bind rule
  expect(m.end).toBe('32px');   // --moss-space-lg, this fix, unaffected by that rule
});
