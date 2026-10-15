#!/usr/bin/env node
// The landing counts a visit and the download click, served from its own
// origin. The page is loaded from the compiled build of site/, so the beacon
// file and the tag the page carries are the ones a reader gets. Every beacon
// POST is intercepted and answered 204, so no real request leaves this machine.
// Which hosts count is the server's decision, so this check does not test it.
import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, resolve, sep } from 'node:path';
import { DEFAULT_BUILD_DIR, loadPlaywright } from './landing-harness.mjs';

const ROOT = resolve(DEFAULT_BUILD_DIR);
const ORIGIN = 'https://mosspub.com';
const BEACON = 'https://api.mosspub.com/api/sites/landing/beacon';
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.webp': 'image/webp' };
const engines = (process.env.LANDING_ENGINES || 'chromium,webkit').split(',');

async function fromDisk(route) {
  const urlPath = decodeURIComponent(new URL(route.request().url()).pathname);
  const target = resolve(join(ROOT, urlPath));
  let file = target.startsWith(ROOT + sep) || target === ROOT ? target : null;
  if (file && (urlPath.endsWith('/') || (await stat(file).catch(() => null))?.isDirectory())) file = join(file, 'index.html');
  const body = file && await readFile(file).catch(() => null);
  if (!body) return route.fulfill({ status: 404, body: 'Not found' });
  return route.fulfill({ status: 200, contentType: MIME[extname(file)] || 'application/octet-stream', body });
}

async function open(browser) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const posts = [];
  await context.route(`${ORIGIN}/**`, fromDisk);
  await context.route('https://github.com/**', route => route.abort());
  await context.route(BEACON, route => {
    posts.push(route.request().postDataJSON());
    return route.fulfill({ status: 204, headers: { 'access-control-allow-origin': '*' } });
  });
  const page = await context.newPage();
  await page.goto(`${ORIGIN}/`);
  await page.waitForFunction(() => document.querySelector('#downloads a[data-platform="macos"]'));
  await page.waitForTimeout(1500);
  return { page, posts, context };
}

const playwright = await loadPlaywright();
let failed = false;
for (const engineName of engines) {
  const browser = await playwright[engineName].launch();
  try {
    const { page, posts, context } = await open(browser);
    const views = () => posts.filter(p => !p.event);
    assert.equal(views().length, 1, `${engineName}: expected one pageview, got ${JSON.stringify(posts)}`);
    assert.equal(new URL(views()[0].url).pathname, '/', `${engineName}: pageview url`);
    assert.equal(views()[0].screen_width, 1440, `${engineName}: screen width`);

    // A page restored from the back-forward cache is counted again.
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    await page.waitForTimeout(800);
    assert.equal(views().length, 2, `${engineName}: restored page must count again, got ${JSON.stringify(posts)}`);

    await page.locator('#downloads a[data-platform="macos"]').dispatchEvent('click');
    await page.waitForTimeout(800);
    const clicks = posts.filter(p => p.event);
    assert.deepEqual(clicks.map(p => [new URL(p.url).pathname, p.event]), [['/', 'download-macos']], `${engineName}: click events ${JSON.stringify(clicks)}`);
    await context.close();
    console.log(`ok ${engineName}: one pageview at /, one more on pageshow, download-macos click`);
  } catch (error) { console.error(`FAIL ${error.message.split('\n')[0]}`); failed = true; }
  finally { await browser.close(); }
}
process.exit(failed ? 1 : 0);
