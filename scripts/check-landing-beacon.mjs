#!/usr/bin/env node
// The landing counts a visit and the clicks that matter, and only when it is
// served from mosspub.com: the same files are staged on scratch-pad.mosspub.com
// and previewed locally, and neither may count as landing traffic. The page is
// loaded under both origins from the local site/ files; every beacon POST is
// intercepted and answered 204, so no real request leaves this machine.
import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadPlaywright } from './landing-harness.mjs';

const ROOT = resolve(fileURLToPath(new URL('../site/', import.meta.url)));
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

async function visit(browser, origin) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const posts = [];
  await context.route(`${origin}/**`, fromDisk);
  await context.route('https://github.com/**', route => route.abort());
  await context.route(BEACON, route => {
    posts.push(route.request().postDataJSON());
    return route.fulfill({ status: 204, headers: { 'access-control-allow-origin': '*' } });
  });
  const page = await context.newPage();
  await page.goto(`${origin}/`);
  return { page, posts, context };
}

const playwright = await loadPlaywright();
let failed = false;
for (const engineName of engines) {
  const browser = await playwright[engineName].launch();
  try {
    const live = await visit(browser, 'https://mosspub.com');
    await live.page.waitForFunction(() => document.querySelector('#downloads a[data-platform="macos"]'));
    await live.page.waitForTimeout(1500);
    const views = live.posts.filter(p => !p.event);
    assert.equal(views.length, 1, `${engineName}: expected one pageview, got ${JSON.stringify(live.posts)}`);
    assert.equal(views[0].path, '/', `${engineName}: pageview path`);
    assert.equal(views[0].screen_bucket, 'desktop', `${engineName}: screen bucket`);
    await live.page.locator('#downloads a[data-platform="macos"]').dispatchEvent('click');
    await live.page.waitForTimeout(800);
    const clicks = live.posts.filter(p => p.event);
    assert.deepEqual(clicks.map(p => [p.path, p.event]), [['/', 'download-macos']], `${engineName}: click events ${JSON.stringify(clicks)}`);
    await live.context.close();

    const staged = await visit(browser, 'https://scratch-pad.mosspub.com');
    await staged.page.waitForFunction(() => document.querySelector('#downloads a[data-platform="macos"]'));
    await staged.page.locator('#downloads a[data-platform="macos"]').dispatchEvent('click');
    await staged.page.waitForTimeout(1500);
    assert.deepEqual(staged.posts, [], `${engineName}: scratch-pad must send nothing, sent ${JSON.stringify(staged.posts)}`);
    await staged.context.close();
    console.log(`ok ${engineName}: one pageview at /, download-macos click, nothing from scratch-pad`);
  } catch (error) { console.error(`FAIL ${error.message.split('\n')[0]}`); failed = true; }
  finally { await browser.close(); }
}
process.exit(failed ? 1 : 0);
