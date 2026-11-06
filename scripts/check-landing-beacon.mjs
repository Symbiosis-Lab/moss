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

// `answer(n)` is the status for the n-th beacon POST (0-based); 204 by default.
// `init` runs in the page before any of its scripts.
async function open(browser, answer = () => 204, init = null) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  if (init) await context.addInitScript(init);
  const posts = [];
  await context.route(`${ORIGIN}/**`, fromDisk);
  await context.route('https://github.com/**', route => route.abort());
  await context.route(BEACON, route => {
    const status = answer(posts.length);
    posts.push({ ...route.request().postDataJSON(), at: Date.now() });
    return route.fulfill({ status, headers: { 'access-control-allow-origin': '*', 'access-control-expose-headers': 'Retry-After', 'retry-after': '1' } });
  });
  const page = await context.newPage();
  // The fragment stands in for a sign-in token; the query must still arrive.
  await page.goto(`${ORIGIN}/?utm_source=check#token=secret`);
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
    assert.equal(views()[0].url, `${ORIGIN}/?utm_source=check`, `${engineName}: pageview url keeps the query, drops the fragment`);
    assert.equal(views()[0].screen_width, 1440, `${engineName}: screen width`);

    // A page restored from the back-forward cache is counted again.
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    await page.waitForTimeout(800);
    assert.equal(views().length, 2, `${engineName}: restored page must count again, got ${JSON.stringify(posts)}`);

    await page.locator('#downloads a[data-platform="macos"]').dispatchEvent('click');
    await page.waitForTimeout(800);
    const clicks = posts.filter(p => p.event);
    assert.deepEqual(clicks.map(p => [p.url, p.event]), [[`${ORIGIN}/?utm_source=check`, 'download-macos']], `${engineName}: click events ${JSON.stringify(clicks)}`);

    // What leaves the browser: only these fields, and a fresh random id per event.
    const PAGEVIEW = ['id', 'referrer', 'screen_width', 'url'];
    for (const p of posts) {
      const { at, ...body } = p;
      assert.deepEqual(Object.keys(body).sort(), p.event ? ['event', 'id', 'url'] : PAGEVIEW, `${engineName}: fields sent ${JSON.stringify(body)}`);
      assert.match(p.id, /^[0-9a-f]{16}$/, `${engineName}: event id ${p.id}`);
    }
    assert.equal(new Set(posts.map(p => p.id)).size, posts.length, `${engineName}: ids repeat across events`);
    await context.close();

    // A 503 is retried once its Retry-After has passed, with the same id.
    const retried = await open(browser, n => (n === 0 ? 503 : 204));
    await retried.page.waitForTimeout(1500);
    const [first, second, ...more] = retried.posts;
    assert.ok(second && more.length === 0, `${engineName}: one retry after a 503, got ${retried.posts.length} posts`);
    assert.equal(second.id, first.id, `${engineName}: the retry keeps the event's id`);
    assert.ok(second.at - first.at >= 900, `${engineName}: retried after ${second.at - first.at} ms, before Retry-After`);
    await retried.context.close();

    // A server that keeps refusing gets the first try and two retries, then nothing.
    const refused = await open(browser, () => 503);
    await refused.page.waitForTimeout(3000);
    assert.equal(refused.posts.length, 3, `${engineName}: tries against a refusing server, got ${refused.posts.length}`);
    await refused.context.close();

    // Without an id the server cannot tell a retry from a new visit, so there is none.
    const noId = await open(browser, () => 503, () => { crypto.getRandomValues = () => { throw new Error('unavailable'); }; });
    await noId.page.waitForTimeout(3000);
    assert.deepEqual(noId.posts.map(p => p.id), [null], `${engineName}: an event without an id is sent once`);
    await noId.context.close();
    console.log(`ok ${engineName}: one pageview at / without the fragment, one more on pageshow, download-macos click, only the agreed fields, one bounded retry, none without an id`);
  } catch (error) { console.error(`FAIL ${error.message.split('\n')[0]}`); failed = true; }
  finally { await browser.close(); }
}
process.exit(failed ? 1 : 0);
