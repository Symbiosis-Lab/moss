#!/usr/bin/env node
// The try-it block, as a visitor uses it: on a phone (375 px, touch) and a
// desktop, in Chromium and WebKit, in English and both Chinese editions.
//   - The page's first load requests nothing of the block. Opening it loads
//     the block and its sample page, and still no WebAssembly.
//   - Every press that waits shows moss's loading state within 150 ms; the
//     first one loads the renderer. A sample picture, a chosen picture, a
//     dropped note and a typed page each become what moss draws, and a link
//     between pages opens the page. Keys typed in the block stay in it.
//   - The downloaded folder, built by the moss CLI, gives the same pages the
//     visitor saw: one per note, each with the same title, words, pictures in
//     order and language. Download moss leads to the macOS button.
// Needs `node scripts/build-site-try-it.mjs` and `moss-cli build site` first.
// MOSS_CLI names the moss binary for the folder build (default: target/debug/moss-cli).
// Usage: check-landing-try-it.mjs [url]
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tryItWords } from './landing-copy.mjs';
import { loadPlaywright, resolveBaseURL, trackErrors } from './landing-harness.mjs';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const MOSS = process.env.MOSS_CLI || join(process.env.CARGO_TARGET_DIR || join(ROOT, 'target'), 'debug', 'moss-cli');
const engines = (process.env.LANDING_ENGINES || 'chromium,webkit').split(',');
const LOCALES = [['en', ''], ['zh-hans', 'zh-hans/'], ['zh-hant', 'zh-hant/']];
const DEVICES = {
  phone: { viewport: { width: 375, height: 740 }, isMobile: true, hasTouch: true },
  desktop: { viewport: { width: 1280, height: 800 } },
};
const PICTURE = join(ROOT, 'site/home/try-it/samples/fish.jpg');
const PRESS_MS = 150;

assert.ok(existsSync(MOSS), `no moss binary at ${MOSS}; build it or set MOSS_CLI`);
const playwright = await loadPlaywright();
const server = await resolveBaseURL(process.argv[2]);
const work = mkdtempSync(join(tmpdir(), 'try-it-check-'));
let failures = 0;

const sitemap = await (await fetch(new URL('sitemap.xml', server.baseURL))).text();
assert.ok(!sitemap.includes('/try-it/'), 'the sitemap lists none of the block\'s files');

// The folder built by the moss CLI: each page's <article> and its <html lang>.
function buildFolder(folder) {
  const run = spawnSync(MOSS, ['build', folder, '--no-plugins'], { encoding: 'utf8' });
  assert.equal(run.status, 0, `moss build failed: ${run.stderr.slice(-400)}`);
  const log = run.stdout + run.stderr;
  const out = log.match(/Site generated at (.+)$/m)?.[1]?.trim();
  assert.ok(out && existsSync(out), `the build said where it wrote: ${log.slice(-400)}`);
  const pages = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory() && !entry.name.startsWith('_')) walk(full);
      else if (entry.name === 'index.html') {
        const html = readFileSync(full, 'utf8');
        const article = html.match(/<article[\s\S]*?<\/article>/)?.[0];
        if (article) pages.push({ article, lang: html.match(/<html[^>]*\blang="([^"]+)"/)?.[1] });
      }
    }
  };
  walk(out);
  return pages;
}

// What a page says, read the same way from the preview and the build: its
// title, its words and its pictures in order. The build adds what only a whole
// site has (the navigation, the home's list of pages and its heading for screen
// readers, which stands in for the site header, responsive picture sources and
// placeholders), so the reading leaves those out.
function readArticle(html) {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  doc.querySelectorAll('.moss-cards-container, .moss-folder-title, source').forEach((n) => n.remove());
  const words = (n) => n.textContent.replace(/\s+/g, ' ').trim();
  const name = (img) => decodeURIComponent((img.dataset.mossSrc ?? img.getAttribute('src')).split('/').pop());
  return { title: doc.querySelector('h1') ? words(doc.querySelector('h1')) : '', text: words(doc.body), pictures: [...doc.querySelectorAll('img')].map(name) };
}

// Presses `button` and requires the loading state on `holder` within PRESS_MS
// of the click, timed inside the page.
async function pressAnswers(page, holder, button) {
  await holder.evaluate((el) => {
    el.__click = el.__loading = null;
    el.addEventListener('click', () => { el.__click ??= performance.now(); }, { capture: true, once: true });
    new MutationObserver((_, seen) => {
      if (el.dataset.state === 'loading') { el.__loading ??= performance.now(); seen.disconnect(); }
    }).observe(el, { attributes: true });
  });
  await button.click();
  await page.waitForTimeout(PRESS_MS + 50);
  const { click, loading } = await holder.evaluate((el) => ({ click: el.__click, loading: el.__loading }));
  assert.ok(click != null && loading != null && loading - click < PRESS_MS, `the press shows the loading state at once (${click}, ${loading})`);
}

async function row(engine, device, locale, path) {
  // The words the served page gives the block, so a row checks that page's own.
  const t = tryItWords(await (await fetch(server.baseURL + path)).text());
  assert.ok(t?.folder && t.download, `${locale}: the page carries the try-it words`);
  if (locale !== 'en') assert.deepEqual(Object.keys(t).filter((k) => !/\p{Script=Han}/u.test(t[k])), [], `${locale}: every word of the block is in ${locale}`);
  const browser = await playwright[engine].launch();
  const context = await browser.newContext({ ...DEVICES[device], acceptDownloads: true, locale: locale === 'en' ? 'en-US' : locale });
  const page = await context.newPage();
  const errors = trackErrors(page);
  const consoleErrors = [];
  page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });
  const requests = [];
  page.on('request', (r) => requests.push(new URL(r.url()).pathname));
  // Held back so each press's loading state is observable, as a slow network would show it.
  const hold = (ms) => async (route) => { await new Promise((r) => setTimeout(r, ms)); await route.continue(); };
  await page.route('**/home/try-it/try-it.js', hold(400));
  await page.route('**/moss_bindings_bg.wasm', hold(800));
  try {
    await page.goto(server.baseURL + path, { waitUntil: 'load' });
    await page.waitForTimeout(1500);
    assert.deepEqual(requests.filter((p) => p.includes('/try-it/')), [], 'the first load requests nothing of the block');

    const trigger = page.locator('#try-it-open');
    await pressAnswers(page, trigger, trigger.locator('button'));
    const dialog = page.locator('#try-it[open]');
    await dialog.waitFor({ timeout: 15000 });
    await page.locator('[data-try-it][data-state="sample"]').waitFor();
    const frame = page.frameLocator('#try-it iframe');
    await frame.locator('code', { hasText: 'index.md' }).waitFor({ timeout: 15000 });
    assert.ok(requests.some((p) => p.endsWith(`/sample-${locale}.html`)), 'the sample page loaded');
    assert.ok(!requests.some((p) => p.endsWith('.wasm')), 'no WebAssembly before a press inside the block');
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'no sideways scroll');
    const coarse = await page.evaluate(() => matchMedia('(pointer: coarse)').matches);
    assert.equal(coarse, device === 'phone', 'the device emulation sets the pointer');
    assert.equal(await page.locator('#try-it input[type=file]').getAttribute('accept'), coarse ? 'image/*' : 'image/*,.md,.markdown,text/markdown');
    assert.equal(await page.locator('.try-it-drop').count(), coarse ? 0 : 1, 'the drop label only where there is a pointer to drag with');

    const drawn = () => frame.locator('article img').last().evaluate(async (img) => {
      if (!img.complete) await new Promise((r) => img.addEventListener('load', r, { once: true }));
      return { src: img.getAttribute('src'), width: img.naturalWidth };
    });
    const stageBusy = () => page.locator('.try-it-stage').evaluate((el, ms) => new Promise((done) => {
      const seen = new MutationObserver(() => { if (el.ariaBusy === 'true') { seen.disconnect(); done(true); } });
      seen.observe(el, { attributes: true });
      setTimeout(() => { seen.disconnect(); done(false); }, ms);
    }), PRESS_MS + 50);

    if (coarse) {
      // A phone's first act: a picture from the library or camera. The renderer is still on its way.
      const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.locator('.try-it-add > .moss-btn').click()]);
      const busy = stageBusy();
      await chooser.setFiles(PICTURE);
      assert.ok(await busy, 'the preview shows it is working at once');
      await page.locator('.try-it-file', { hasText: 'fish.jpg' }).waitFor();
      const img = await drawn();
      assert.match(img.src, /^blob:/, 'the picture is drawn from the visitor\'s own file');
      assert.ok(img.width > 0, 'the picture decoded');
    }
    const sample = page.locator('.try-it-add .try-it-slot', { hasText: t.sample });
    await pressAnswers(page, sample, sample.locator('button'));
    await page.locator('[data-try-it][data-state="live"]').waitFor();
    await page.locator('.try-it-file', { hasText: t.sampleName }).waitFor();
    assert.ok(requests.some((p) => p.endsWith('/moss_bindings_bg.wasm')), 'a press loaded the renderer');
    const img = await drawn();
    assert.match(img.src, /^blob:/);
    assert.ok(img.width > 0);

    if (!coarse) {
      const note = join(work, 'A walk.md');
      writeFileSync(note, '# A walk\n\nBy the pond, *slowly*.\n');
      const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.locator('.try-it-add > .moss-btn').click()]);
      await chooser.setFiles(note);
      await page.locator('.try-it-file[aria-current="page"]', { hasText: 'A walk.md' }).waitFor();
      await frame.locator('h1', { hasText: 'A walk' }).waitFor();

      // A dropped file that is not a note or a picture is named, not swallowed.
      await page.evaluate(() => {
        const data = new DataTransfer();
        data.items.add(new File(['plain'], 'notes.txt', { type: 'text/plain' }));
        const target = document.querySelector('[data-try-it]');
        for (const type of ['dragenter', 'dragover', 'drop']) target.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: data }));
      });
      await page.locator('.try-it-status', { hasText: t.refused }).waitFor();
      assert.equal(await page.locator('.try-it-file', { hasText: 'notes.txt' }).count(), 0);
    }

    // A new page, typed with real keys, linking home. The page behind never moves.
    const scrolled = await page.evaluate(() => scrollY);
    const fresh = page.locator('.try-it-add .try-it-slot', { hasText: t.newPage });
    await fresh.locator('button').click();
    const text = page.locator('.try-it-text');
    await text.waitFor();
    assert.ok(await text.evaluate((el) => el === document.activeElement), 'the new page is ready to type in');
    await page.keyboard.type('# Hello\n\nA b. Back to [home](index.md).');
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('PageDown');
    await page.locator('.try-it-toggle').focus();
    await page.keyboard.press('ArrowDown');
    assert.equal(await text.inputValue(), '# Hello\n\nA b. Back to [home](index.md).');
    await page.waitForTimeout(800); // a scene step scrolls smoothly
    assert.equal(await page.evaluate(() => scrollY), scrolled, 'keys in the block do not move the page behind');
    await page.locator('.try-it-toggle').click();
    await frame.locator('h1', { hasText: 'Hello' }).waitFor();
    await frame.locator('a', { hasText: 'home' }).click();
    await page.locator('.try-it-file[aria-current="page"]', { hasText: 'index.md' }).waitFor();

    // What the visitor saw, page by page, then the folder they take away.
    const seen = {};
    for (const button of await page.locator('button.try-it-file').all()) {
      const name = await button.textContent();
      await button.click();
      await page.locator('.try-it-file[aria-current="page"]', { hasText: name }).waitFor();
      await page.waitForTimeout(100);
      seen[name] = await frame.locator('article').evaluate((a) => ({ article: a.outerHTML, lang: document.documentElement.lang }));
    }
    const zip = page.locator('.try-it-take .try-it-slot', { hasText: t.zip });
    const [file] = await Promise.all([page.waitForEvent('download'), pressAnswers(page, zip, zip.locator('button'))]);
    assert.equal(file.suggestedFilename(), `${t.folder}.zip`);
    const out = mkdtempSync(join(work, `${engine}-${device}-${locale}-`));
    const archive = join(out, 'folder.zip');
    await file.saveAs(archive);
    execFileSync('python3', ['-I', '-c', 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])', archive, out]);
    const folder = join(out, t.folder);
    assert.deepEqual(readdirSync(folder).filter((n) => n.endsWith('.md')).sort(), Object.keys(seen).sort());
    const reader = `(${readArticle})`;
    // Language tags are case-insensitive: the build writes zh-Hans where the preview writes zh-hans.
    const read = async ({ article, lang }) => ({ ...(await page.evaluate(([fn, h]) => eval(fn)(h), [reader, article])), lang: lang?.toLowerCase() });
    const built = await Promise.all(buildFolder(folder).map(read));
    assert.equal(built.length, Object.keys(seen).length, 'the build made one page per note');
    for (const [name, page_] of Object.entries(seen)) {
      const saw = await read(page_);
      const match = built.findIndex((b) => JSON.stringify(b) === JSON.stringify(saw));
      assert.ok(match >= 0, `${name} builds to the page the visitor saw: ${JSON.stringify(saw)} not in ${JSON.stringify(built)}`);
      built.splice(match, 1);
    }

    // Download moss leaves the block for the download buttons.
    await page.locator('.try-it-take > a.moss-btn').click();
    await page.locator('#try-it').waitFor({ state: 'hidden' });
    await page.locator('[data-platform="macos"]').click({ trial: true, timeout: 30000 });
    // As from scene 1's own Download button, the page hands focus to the macOS button once the close settles.
    await page.waitForFunction(() => document.activeElement?.matches('[data-platform="macos"]'), null, { timeout: 15000 });

    assert.deepEqual(errors, [], 'no page errors');
    console.log(`ok ${engine} ${device} ${locale}`);
  } catch (error) {
    failures++;
    console.error(`FAIL ${engine} ${device} ${locale}: ${error.message.slice(0, 1500)}`);
    for (const line of [...errors, ...consoleErrors]) console.error(`  page: ${line.slice(0, 300)}`);
    await page.screenshot({ path: join(work, `${engine}-${device}-${locale}.png`) }).catch(() => {});
  } finally {
    await browser.close();
  }
}

// TRY_IT_DEVICES and TRY_IT_LOCALES narrow a run, like LANDING_ENGINES.
const only = (name, all) => (process.env[name] ? process.env[name].split(',') : all);
for (const engine of engines) for (const device of only('TRY_IT_DEVICES', Object.keys(DEVICES))) {
  for (const [locale, path] of LOCALES.filter(([l]) => only('TRY_IT_LOCALES', [l]).includes(l))) await row(engine, device, locale, path);
}
await server.close();
if (failures) { console.error(`${failures} row(s) failed; screenshots in ${work}`); process.exit(1); }
