#!/usr/bin/env node
// The landing's macOS button must work for a person: reached by wheel or by a
// jump to the bottom, on a desktop or a phone, in Chromium and WebKit. Reading
// the href is not enough — the close is a fixed layer that takes the pointer
// only once it settles, and on 2026-10-12 a button with a perfectly good href
// was reported dead. Run before every landing deploy, against the local site
// (default) or the staged one: check-landing-download.mjs [url].
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const CLICKABLE_WITHIN_MS = 12000;
const VIEWPORTS = [['desktop', { width: 1440, height: 900 }], ['phone', { width: 390, height: 844, isMobile: true, hasTouch: true }]];
const engines = (process.env.LANDING_ENGINES || 'chromium,webkit').split(',');

const server = await resolveBaseURL(process.argv[2], { root: fileURLToPath(new URL('../site/', import.meta.url)) });
const playwright = await loadPlaywright();
const seen = new Set();
let failed = false;
try {
  for (const engineName of engines) {
    const browser = await playwright[engineName].launch();
    try {
      for (const [vpName, viewport] of VIEWPORTS) {
        for (const how of vpName === 'phone' ? ['jump'] : ['wheel', 'jump']) {
          const label = `${engineName} ${vpName} ${how}`;
          const context = await browser.newContext(viewport);
          const page = await context.newPage();
          const requested = [];
          context.on('request', request => { if (/\/releases\//.test(request.url())) requested.push(request.url()); });
          await page.goto(server.baseURL);
          await page.waitForFunction(() => document.querySelector('#downloads a[data-platform="macos"]')?.href);
          const href = await page.locator('#downloads a[data-platform="macos"]').getAttribute('href');
          seen.add(href);
          if (how === 'jump') await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
          else {
            await page.mouse.move(viewport.width / 2, viewport.height / 2);
            for (let i = 0; i < 150 && await page.evaluate(() => scrollY < document.documentElement.scrollHeight - innerHeight - 2); i++) {
              await page.mouse.wheel(0, 400); await page.waitForTimeout(120);
            }
          }
          const t0 = Date.now();
          try {
            await page.waitForFunction(() => {
              const a = document.querySelector('#downloads a[data-platform="macos"]');
              const r = a.getBoundingClientRect();
              const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
              return hit && a.contains(hit) && r.bottom <= innerHeight && r.top >= 0;
            }, null, { timeout: CLICKABLE_WITHIN_MS, polling: 100 });
          } catch {
            const covered = await page.evaluate(() => {
              const r = document.querySelector('#downloads a[data-platform="macos"]').getBoundingClientRect();
              const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
              return (hit ? `${hit.tagName}#${hit.id}` : 'nothing') + ` at scrollY ${Math.round(scrollY)} of ${document.documentElement.scrollHeight - innerHeight}, button y ${Math.round(r.y)}`;
            });
            console.error(`FAIL ${label}: macOS button not clickable ${CLICKABLE_WITHIN_MS}ms after reaching the close; covered by ${covered}`);
            failed = true; await context.close(); continue;
          }
          // A disk image answers a click with a download; cancel it rather than pull 40 MB.
          const download = page.waitForEvent('download', { timeout: 10000 }).catch(() => null);
          await page.locator('#downloads a[data-platform="macos"]').click({ timeout: 5000 });
          const file = await download;
          if (file) await file.cancel();
          if (!file || !/\.dmg$/.test(file.suggestedFilename()) || requested[0] !== href) {
            console.error(`FAIL ${label}: click requested ${JSON.stringify(requested)} and ${file ? 'saved ' + file.suggestedFilename() : 'started no download'}; expected a .dmg from ${href}`);
            failed = true;
          } else console.log(`ok ${label}: clickable after ${Date.now() - t0}ms, downloads ${file.suggestedFilename()}`);
          await context.close();
        }
      }
    } finally { await browser.close(); }
  }
} finally { await server.close(); }

// The href itself must serve a disk image, not a 404 or an HTML page. This is
// the check that fails if the "latest" release is not the app's.
for (const href of seen) {
  const res = await fetch(href, { method: 'HEAD', redirect: 'follow' });
  const type = res.headers.get('content-type') || '';
  const size = Number(res.headers.get('content-length') || 0);
  try {
    assert.equal(res.status, 200, `${href} answered ${res.status} at ${res.url}`);
    assert.ok(!type.includes('html') && size > 1e6, `${href} is not a disk image (${type}, ${size} bytes)`);
    console.log(`ok ${href} -> ${res.status} ${type} ${size} bytes`);
  } catch (error) { console.error(`FAIL ${error.message}`); failed = true; }
}
process.exit(failed ? 1 : 0);
