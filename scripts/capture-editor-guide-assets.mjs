#!/usr/bin/env node
// Retakes the two still pictures of the editor that Meet the editor shows when JavaScript is off
// (site/assets/guides/). They are taken from the same harvested document the live demo frames,
// site/ui/app-editor.html, so a refreshed harvest and its fallbacks cannot disagree. Needs no
// build and no server of its own:
//   PLAYWRIGHT_MODULE=/path/to/node_modules/playwright/index.mjs node scripts/capture-editor-guide-assets.mjs

import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const guideDir = join(root, 'site/assets/guides');

const playwright = await loadPlaywright();
const { baseURL, close } = await resolveBaseURL(undefined, { root: join(root, 'site/ui') });
const browser = await playwright.chromium.launch({ headless: true });
try {
  // Opens the English fixture and hands back the demo's own target lookup, so a selector the
  // harvest renames is fixed once, in the surface adapter, for the demo and for these pictures.
  const open = async (viewport) => {
    const page = await browser.newPage({ viewport, deviceScaleFactor: 1 });
    await page.goto(new URL('app-editor.html?lang=en', baseURL).href, { waitUntil: 'load' });
    await page.waitForSelector('html[data-ready="1"]');
    await page.addScriptTag({
      type: 'module',
      content: "import { findTarget } from './demo/surfaces/editor.js'; window.__findTarget = (name) => findTarget(document, name);",
    });
    await page.waitForFunction(() => typeof window.__findTarget === 'function');
    return page;
  };

  const editor = await open({ width: 760, height: 420 });
  await editor.evaluate(() => window.__editor.focus());
  await editor.waitForSelector('.cm-editor.cm-focused');
  await editor.waitForTimeout(500);
  await editor.screenshot({ path: join(guideDir, 'editor-ui-source.png') });

  const tree = await open({ width: 520, height: 520 });
  await tree.evaluate(() => window.__findTarget('tree.breadcrumb').click());
  await tree.waitForFunction(() => window.__findTarget('tree.pageRow') !== null);
  await tree.waitForTimeout(500);
  await tree.screenshot({ path: join(guideDir, 'choose-page-source.png') });
} finally {
  await browser.close();
  await close();
}

console.log('Captured editor-ui-source.png and choose-page-source.png from site/ui/app-editor.html');
