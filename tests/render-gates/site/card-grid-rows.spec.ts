// The guide's recipe for a theme that boxes grid cells: adding
// `align-self: stretch` to the rule that draws the box makes a row's boxes
// share its height, and leaves the cells' contents where they were, since a
// stretched cell stays in block flow. The theme rules are unlayered, as a
// site's own stylesheet is, so they outrank moss's: a picture paragraph's
// bottom margin collapses into the heading's top margin in block flow and
// would add to it in a flex column.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';
import { TOKEN_CSS } from './tokens-block';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');
const THEME = `.moss-grid-card { border: 1px solid; padding: 12px; }
.stretched > .moss-grid-card { align-self: stretch; }
.moss-grid-card p:has(img) { margin-bottom: 0.75rem; }
.moss-grid-card h3 { margin-top: 1.5rem; }`;

const PIXEL = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';
const LONG = 'A much longer description that runs on over several lines, so that this card is far taller than the cards beside it in its row.';
const card = (text: string) => `<div class="moss-grid-card"><p><picture><img src="${PIXEL}" width="300" height="200" alt=""></picture></p><h3>Title</h3><p>${text}</p></div>`;
const grid = (cls: string) => `<div class="${cls}" data-columns="3">${card('Short.')}${card(LONG)}${card('Middling text here.')}</div>`;

const page = (body: string) => `<!doctype html><html lang="en"><head><meta charset="utf-8">
<style>${TOKEN_CSS}</style><style>${CSS}</style><style>${THEME}</style>
<style>body{margin:0}article{width:960px}</style>
</head><body><main><article>${body}</article></main></body></html>`;

/** Each cell's box and the offsets of its children from the cell's top. */
const cells = (p: import('@playwright/test').Page, selector: string) => p.evaluate((sel) => [...document.querySelector(sel)!.children].map((c) => {
  const box = c.getBoundingClientRect();
  return { top: Math.round(box.top), bottom: Math.round(box.bottom), offsets: [...c.children].map((e) => Math.round(e.getBoundingClientRect().top - box.top)) };
}), selector);

test.use({ viewport: { width: 1000, height: 800 } });

test('a boxed cell with align-self: stretch shares its row height', async ({ page: p }) => {
  await p.setContent(page(grid('moss-grid stretched')));
  const row = await cells(p, '.moss-grid');
  expect(new Set(row.map((c) => c.top)).size, 'the cards are not on one row').toBe(1);
  expect(new Set(row.map((c) => c.bottom)).size, 'the cards in the row differ in height').toBe(1);
});

test('stretching a cell moves nothing inside it', async ({ page: p }) => {
  await p.setContent(page(`${grid('moss-grid stretched')}${grid('moss-grid plain')}`));
  const stretched = await cells(p, '.moss-grid.stretched');
  const plain = await cells(p, '.moss-grid.plain');
  expect(plain[0].offsets[1] - plain[0].offsets[0], 'the heading should sit apart from its picture').toBeGreaterThan(8);
  expect(stretched.map((c) => c.offsets)).toEqual(plain.map((c) => c.offsets));
});

test('a prose cell is as tall as its own content', async ({ page: p }) => {
  await p.setContent(page(grid('moss-grid')));
  const [short, long] = await cells(p, '.moss-grid');
  expect(short.bottom, 'a cell was stretched to its neighbour').toBeLessThan(long.bottom);
});
