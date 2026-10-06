#!/usr/bin/env node

import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const preview = process.argv.find((arg) => arg.startsWith('--preview='))?.slice('--preview='.length) || 'http://localhost:8080/';
const editor = new URL('ui/editor.html?measure=400&root=William%20Blake&file=index.md&date=1790-06-01', preview.endsWith('/') ? preview : `${preview}/`).href;
const treeEditor = new URL('ui/editor.html?measure=400&root=William%20Blake&file=Songs%20of%20Experience%2FThe%20Tyger.md&date=1794-07-01', preview.endsWith('/') ? preview : `${preview}/`).href;
const guideDir = join(root, 'site/assets/guides');
const requestedModule = process.env.PLAYWRIGHT_MODULE || 'playwright';
const moduleSpecifier = requestedModule.startsWith('/') ? pathToFileURL(requestedModule).href : requestedModule;

let playwright;
try {
  playwright = await import(moduleSpecifier);
} catch (error) {
  throw new Error(`Could not load Playwright from ${requestedModule}. Set PLAYWRIGHT_MODULE to playwright/index.mjs in an existing install.\n${error}`);
}

const browser = await playwright.chromium.launch({ headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 760, height: 420 }, deviceScaleFactor: 1 });
  await page.goto(editor, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('html[data-ready="1"]');
  await page.waitForFunction(() => Boolean(window.__editor));
  await page.evaluate(() => {
    window.__editor.setDoc("I must Create a System, or be enslav'd by another Man's.\nI will not Reason & Compare: my business is to Create.");
    window.__editor.focus();
  });
  await page.waitForSelector('#chip-bar:not([style*="display: none"])');
  await page.waitForSelector('.cm-editor.cm-focused');
  await page.mouse.move(380, 400);
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(guideDir, 'editor-ui-source.png') });

  await page.setViewportSize({ width: 520, height: 520 });
  await page.goto(treeEditor, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('html[data-ready="1"]');
  await page.locator('summary.nav-bc-strip').click();
  await page.waitForSelector('details.lab-tree-root[open] .lab-tree-file.active');
  await page.screenshot({ path: join(guideDir, 'choose-page-source.png') });
} finally {
  await browser.close();
}

for (const file of ['editor-ui-source.png', 'choose-page-source.png']) {
  const path = join(guideDir, file);
  if (!existsSync(path)) throw new Error(`Capture was not written: ${path}`);
}

console.log('Captured editor-ui-source.png and choose-page-source.png');
