#!/usr/bin/env node
// The landing counts a visit, the download click, links out, downloads and
// engagement, served from its own origin. The page is loaded from the compiled build of site/, so the beacon
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
  // Nothing else leaves this machine; a later route takes precedence.
  await context.route('**/*', route => route.abort());
  await context.route(`${ORIGIN}/**`, fromDisk);
  // `v2` marks a post to the path for the rows the script adds by itself.
  const beacon = v2 => route => {
    const status = answer(posts.length);
    posts.push({ ...route.request().postDataJSON(), at: Date.now(), v2 });
    return route.fulfill({ status, headers: { 'access-control-allow-origin': '*', 'access-control-expose-headers': 'Retry-After', 'retry-after': '1' } });
  };
  await context.route(BEACON, beacon(false));
  await context.route(`${BEACON}/v2`, beacon(true));
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
    const views = () => posts.filter(p => !p.event && !p.kind);
    assert.equal(views().length, 1, `${engineName}: expected one pageview, got ${JSON.stringify(posts)}`);
    assert.equal(views()[0].url, `${ORIGIN}/?utm_source=check`, `${engineName}: pageview url keeps the query, drops the fragment`);
    assert.equal(views()[0].screen_width, 1440, `${engineName}: screen width`);

    // A page restored from the back-forward cache is counted again.
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    await page.waitForTimeout(800);
    assert.equal(views().length, 2, `${engineName}: restored page must count again, got ${JSON.stringify(posts)}`);

    await page.locator('#downloads a[data-platform="macos"]').dispatchEvent('click');
    await page.waitForTimeout(800);
    const clicks = posts.filter(p => p.event && !p.kind);
    assert.deepEqual(clicks.map(p => [p.url, p.event]), [[`${ORIGIN}/?utm_source=check`, 'download-macos']], `${engineName}: click events ${JSON.stringify(clicks)}`);

    // Links nobody marked: one to another site, one to a file, each clicked; the
    // other site is clicked again with the middle button. Navigation is stopped
    // for both buttons so the page stays (WebKit follows a synthetic auxclick).
    await page.evaluate(() => document.body.insertAdjacentHTML('beforeend',
      '<a id="x-out" href="https://example.org/a/b?token=secret" onclick="event.preventDefault()" onauxclick="event.preventDefault()">out</a>' +
      '<a id="x-file" href="/files/Report%202026.pdf?v=2" onclick="event.preventDefault()">file</a>' +
      '<a id="x-page" href="/docs/" onclick="event.preventDefault()">page</a>'));
    await page.locator('#x-out').dispatchEvent('click');
    await page.locator('#x-file').dispatchEvent('click');
    await page.locator('#x-page').dispatchEvent('click');
    // A synthetic event, so its button is set as a browser sets it.
    const auxclick = button => page.evaluate(b => document.getElementById('x-out').dispatchEvent(new MouseEvent('auxclick', { button: b, bubbles: true, cancelable: true })), button);
    await auxclick(1);
    await auxclick(2);
    await page.waitForTimeout(800);
    const links = posts.filter(p => p.kind === 'out' || p.kind === 'download');
    assert.deepEqual(links.map(p => [p.kind, p.event]), [['out', 'example.org'], ['download', '/files/Report%202026.pdf'], ['out', 'example.org']],
      `${engineName}: links out and downloads ${JSON.stringify(links)}`);
    assert.equal(posts.filter(p => p.event === 'download-macos').length, 1, `${engineName}: a marked link stays one named click`);

    // Leaving the page sends one engagement event: the page scrolled to its end, on screen under 10 s.
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForTimeout(300);
    const hide = () => page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await hide();
    await hide();
    await page.waitForTimeout(800);
    const engaged = posts.filter(p => p.kind === 'engage');
    assert.deepEqual(engaged.map(p => [p.url, p.scroll, p.visible]), [[`${ORIGIN}/?utm_source=check`, 100, '<10s']],
      `${engineName}: one engagement event on leaving ${JSON.stringify(engaged)}`);

    // What leaves the browser: only these fields, and a fresh random id per event.
    const FIELDS = { view: ['id', 'referrer', 'screen_width', 'url'], click: ['event', 'id', 'url'], out: ['event', 'id', 'kind', 'url'], download: ['event', 'id', 'kind', 'url'], engage: ['id', 'kind', 'scroll', 'url', 'visible'] };
    for (const p of posts) {
      const { at, v2, ...body } = p;
      assert.equal(v2, Boolean(p.kind), `${engineName}: ${p.kind || 'a pageview or click'} posted to ${v2 ? '/v2' : 'the old path'}`);
      assert.deepEqual(Object.keys(body).sort(), FIELDS[p.kind || (p.event ? 'click' : 'view')], `${engineName}: fields sent ${JSON.stringify(body)}`);
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
    console.log(`ok ${engineName}: one pageview at / without the fragment, one more on pageshow, download-macos click, a link out and a download by host and path (middle click too), one engagement event on leaving, each on its path, only the agreed fields, one bounded retry, none without an id`);
  } catch (error) { console.error(`FAIL ${error.message.split('\n')[0]}`); failed = true; }
  finally { await browser.close(); }
}
process.exit(failed ? 1 : 0);
