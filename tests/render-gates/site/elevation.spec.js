// @ts-check
// One declared light (docs/reference/design/elevation.md): every floating
// surface in the emitted site reads a `--moss-elevation-N` token and casts the
// way the light says; content casts nothing. Only a browser can see this:
// jsdom does not compute a box-shadow through calc() and color-mix(), and a
// Rust test cannot tell which selector a shadow lands on. The assertions
// compare a real surface against a bare element reading the same token, so a
// surface that drifts back to a literal — even one that happens to look
// similar — goes red without this file naming any number.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { TOKEN_CSS } from './tokens-block.js';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(
  path.join(mossBuildAssets(), 'css/site.css'), 'utf8');

// Real classes, the level map's examples: the nav island and the breadcrumb
// menu are floating chrome (level 2), the active font pill is a resting
// control (level 1), a code block and the article are content (level 0).
function sitePage({ theme = null, lightX = null } = {}) {
  return `<!doctype html><html${theme ? ` data-theme="${theme}"` : ''}><head><meta charset="utf-8">
<style>${TOKEN_CSS}</style><style>${CSS}</style>
${lightX == null ? '' : `<style>:root{--moss-light-x:${lightX}}</style>`}
</head><body>
<nav class="moss-nav-island"><div class="moss-nav-island-bar" id="island">nav</div></nav>
<div class="moss-breadcrumb-menu" id="crumb">menu</div>
<div class="font-pill"><button class="active" id="pill">A</button></div>
<article id="article"><p>flat</p><pre id="pre">code</pre></article>
<div id="bare1" style="box-shadow: var(--moss-elevation-1)"></div>
<div id="bare2" style="box-shadow: var(--moss-elevation-2)"></div>
</body></html>`;
}

/** @param {import('@playwright/test').Page} page */
async function shadows(page, ids) {
  const out = {};
  for (const id of ids) out[id] = await page.locator(`#${id}`).evaluate((el) => getComputedStyle(el).boxShadow);
  return out;
}

const ALL = ['island', 'crumb', 'pill', 'article', 'pre', 'bare1', 'bare2'];

test('floating chrome paints the level its map assigns, content paints nothing', async ({ page }) => {
  await page.setContent(sitePage());
  const s = await shadows(page, ALL);
  expect(s.bare2).not.toBe('none');
  expect(s.bare1).not.toBe(s.bare2);
  expect(s.island).toBe(s.bare2);
  expect(s.crumb).toBe(s.bare2);
  expect(s.pill).toBe(s.bare1);
  expect(s.article).toBe('none');
  expect(s.pre).toBe('none');
});

test('turning the light moves every cast, and the surfaces follow the token', async ({ page }) => {
  await page.setContent(sitePage({ lightX: 0 }));
  const above = await shadows(page, ['island', 'bare2']);
  await page.setContent(sitePage({ lightX: 1 }));
  const side = await shadows(page, ['island', 'bare2']);
  expect(side.bare2).not.toBe(above.bare2);
  expect(side.island).toBe(side.bare2);
});

test('the dark ground changes the inputs, never the surface', async ({ page }) => {
  await page.setContent(sitePage());
  const light = await shadows(page, ['island', 'bare2']);
  await page.setContent(sitePage({ theme: 'dark' }));
  const dark = await shadows(page, ['island', 'bare2']);
  expect(dark.bare2).not.toBe(light.bare2);
  expect(dark.island).toBe(dark.bare2);
});
