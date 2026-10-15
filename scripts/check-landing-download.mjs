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

// A hang guard, not a pace bound. Reaching the close plays every crossing it passes, and those are
// paced in frames, so a software-rendered engine under load needs seconds where a real one needs
// tenths. The rule that a button a person can see must work is __visibleButDead below; this only
// bounds how long we wait for the button, and a tight bound here reports a slow machine as a dead
// button and hides that assertion behind its own timeout.
const CLICKABLE_WITHIN_MS = 30000;
const WHEEL_ATTEMPTS = 3;
const VIEWPORTS = [['desktop', { width: 1440, height: 900 }], ['phone', { width: 390, height: 844, isMobile: true, hasTouch: true }]];
const engines = (process.env.LANDING_ENGINES || 'chromium,webkit').split(',');
// Reduced motion takes different code paths in the page's scroll controller, so it gets its own rows.
const ROWS = [['desktop', 'wheel', 'no-preference'], ['desktop', 'wheel', 'reduce'], ['desktop', 'jump', 'no-preference'], ['desktop', 'jump', 'reduce'], ['phone', 'jump', 'no-preference']];

const server = await resolveBaseURL(process.argv[2], { root: fileURLToPath(new URL('../site/', import.meta.url)) });
const playwright = await loadPlaywright();
const seen = new Set();
let failed = false;
try {
  for (const engineName of engines) {
    const browser = await playwright[engineName].launch();
    try {
      for (const [vpName, how, motion] of ROWS) {
        const viewport = VIEWPORTS.find(([name]) => name === vpName)[1];
        const runRow = async (attempt) => {
          const label = `${engineName} ${vpName} ${how}${motion === 'reduce' ? ' reduced-motion' : ''}`;
          const { width, height, ...device } = viewport;
          const context = await browser.newContext({ viewport: { width, height }, ...device, reducedMotion: motion });
          const page = await context.newPage();
          const requested = [];
          context.on('request', request => { if (/\/releases\//.test(request.url())) requested.push(request.url()); });
          await page.goto(server.baseURL);
          await page.waitForFunction(() => document.querySelector('#downloads a[data-platform="macos"]')?.href);
          const href = await page.locator('#downloads a[data-platform="macos"]').getAttribute('href');
          seen.add(href);
          if (how === 'jump') await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
          else {
            // A person's wheel is separate gestures (two notches, then a pause), and the page moves one
            // scene per gesture. One endless stream of events is a single gesture and proves nothing.
            // Over a demo frame the wheel belongs to the frame, by design; a reader scrolling the page has
            // the pointer in the margin, off the frames.
            await page.mouse.move(120, viewport.height / 2);
            for (let g = 0; g < 14 && await page.evaluate(() => !document.querySelector('#five').classList.contains('on')); g++) {
              await page.mouse.wheel(0, 100); await page.waitForTimeout(100); await page.mouse.wheel(0, 100); await page.waitForTimeout(1300);
            }
          }
          const t0 = Date.now();
          try {
            // While polling, note any moment the close is visibly there (opacity >= .5) yet the button
            // is not what a click would reach: a button a person can see must work.
            await page.waitForFunction(() => {
              const a = document.querySelector('#downloads a[data-platform="macos"]');
              const r = a.getBoundingClientRect();
              const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
              const reachable = !!hit && a.contains(hit) && r.bottom <= innerHeight && r.top >= 0;
              if (!reachable && +getComputedStyle(document.querySelector('#five')).opacity >= .5) window.__visibleButDead = (window.__visibleButDead || 0) + 1;
              return reachable;
            }, null, { timeout: CLICKABLE_WITHIN_MS, polling: 100 });
          } catch {
            const covered = await page.evaluate(() => {
              const r = document.querySelector('#downloads a[data-platform="macos"]').getBoundingClientRect();
              const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
              return (hit ? `${hit.tagName}#${hit.id}` : 'nothing') + ` at scrollY ${Math.round(scrollY)} of ${document.documentElement.scrollHeight - innerHeight}, button y ${Math.round(r.y)}`;
            });
            await context.close();
            // The unchanged live page stalls mid-scroll in about 1 of 16 wheel runs in Chromium (a wheel
            // elsewhere frees it), so a wheel row gets fresh attempts; a stall that is deterministic, like
            // reduced motion's was, still fails all of them.
            if (how === 'wheel' && attempt < WHEEL_ATTEMPTS) { console.warn(`WARN ${label}: stalled before the close (${covered}); retrying`); return 'retry'; }
            console.error(`FAIL ${label}: macOS button not clickable ${CLICKABLE_WITHIN_MS}ms after reaching the close; covered by ${covered}`);
            failed = true; return;
          }
          const dead = await page.evaluate(() => window.__visibleButDead || 0);
          if (dead) { console.error(`FAIL ${label}: the close was visible for ${dead * 100}ms while the button could not be clicked`); failed = true; }
          // A disk image answers a click with a download; cancel it rather than pull 40 MB.
          const download = page.waitForEvent('download', { timeout: 10000 }).catch(() => null);
          try { await page.locator('#downloads a[data-platform="macos"]').click({ timeout: 5000 }); }
          catch (error) { console.error(`FAIL ${label}: click failed: ${error.message.split('\n')[0]}`); failed = true; await context.close(); return; }
          const file = await download;
          if (file) await file.cancel();
          if (!file || !/\.dmg$/.test(file.suggestedFilename()) || requested[0] !== href) {
            console.error(`FAIL ${label}: click requested ${JSON.stringify(requested)} and ${file ? 'saved ' + file.suggestedFilename() : 'started no download'}; expected a .dmg from ${href}`);
            failed = true;
          } else console.log(`ok ${label}: clickable after ${Date.now() - t0}ms, downloads ${file.suggestedFilename()}`);
          await context.close();
        };
        for (let attempt = 1; await runRow(attempt) === 'retry'; attempt++);
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
