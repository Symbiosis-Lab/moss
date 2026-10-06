// `--moss-card-cover-fit` is how a site asks for the whole picture in a card
// instead of a centre crop. The shared `.moss-card-cover > img` rule reads it,
// but the list layout's own cover rule re-declared `object-fit: cover` at a
// higher specificity, so on a list listing the token was silently ignored:
// an artwork site's paintings kept cropping to blank paper (a real
// artwork site, 2026-09-23) while its grid cards obeyed.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');
const IMG = 'data:image/svg+xml,' + encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="400" height="1600"><rect width="100%" height="100%" fill="#357"/></svg>');

const card = `<a href="#" class="moss-card"><div class="moss-card-row"><div class="moss-card-body">`
  + `<h3 class="moss-card-title">t</h3></div><div class="moss-card-cover"><picture>`
  + `<img src="${IMG}" width="400" height="1600" alt=""></picture></div></div></a>`;

for (const layout of ['list', 'grid']) {
  test(`a ${layout} listing honours --moss-card-cover-fit`, async ({ page }) => {
    await page.setContent(`<!doctype html><html><head><style>${CSS}</style>
<style>.moss-cards { --moss-card-cover-fit: contain; }</style></head><body>
<article class="container"><div class="moss-cards-container"><div class="moss-cards" data-layout="${layout}">
${card}</div></div></article></body></html>`);
    const fit = await page.locator('.moss-card-cover img').evaluate((el) => getComputedStyle(el).objectFit);
    expect(fit).toBe('contain');
  });

  test(`a ${layout} listing still crops by default`, async ({ page }) => {
    await page.setContent(`<!doctype html><html><head><style>${CSS}</style></head><body>
<article class="container"><div class="moss-cards-container"><div class="moss-cards" data-layout="${layout}">
${card}</div></div></article></body></html>`);
    const fit = await page.locator('.moss-card-cover img').evaluate((el) => getComputedStyle(el).objectFit);
    expect(fit).toBe('cover');
  });
}
