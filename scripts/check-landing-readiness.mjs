#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const { chromium, webkit } = await loadPlaywright();
try {
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
    if (process.env.LANDING_HTML_OVERRIDE) {
      const body = await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8');
      await page.route(baseURL, route => route.fulfill({ contentType: 'text/html', body }));
    }
    await page.goto(baseURL);
    await page.waitForFunction(() => window.__landing.state?.().ready);
    await page.evaluate(() => scrollTo(0, window.__landing.restY(2)));
    await page.waitForFunction(() => window.__landing.state().shown === 2 && !window.__landing.state().running, null, { timeout: 30000 });
    // A cold jump can reach the creative scene before its next print exists.
    await page.evaluate(() => { window.__landing.prints[3] = null; scrollTo(0, window.__landing.restY(3)); });
    await page.waitForFunction(() => window.__landing.state().shown === 3 && !window.__landing.state().running, null, { timeout: 15000 });
    console.log(`${engine.name()}: missing next print recovered without another gesture`);
    await page.close();
    for (const javaScriptEnabled of [true, false]) {
      const fallback = await browser.newPage({ javaScriptEnabled, viewport: { width: 390, height: 844 } });
      if (javaScriptEnabled) await fallback.route('**/ui/editor.html*', route => route.abort());
      await fallback.goto(baseURL);
      if (javaScriptEnabled) await fallback.waitForSelector('html[data-static="1"]', { timeout: 15000 });
      const usable = await fallback.evaluate(() => {
        const closing = document.querySelector('#five');
        return getComputedStyle(closing).opacity === '1' && !closing.inert && getComputedStyle(document.querySelector('.page')).display !== 'none';
      });
      if (!usable) throw Error('Static landing content is hidden');
      console.log(`${engine.name()}: ${javaScriptEnabled ? 'failed demo' : 'no JavaScript'} leaves copy and downloads readable`);
      await fallback.close();
    }
    // A desktop failed boot never lets setupTitleDissolve past its own
    // data-ready wait, so the title is never actually left mid-dissolve by
    // it — but the ready().catch() reset exists as the sole safety net for
    // that, so this pins the reset itself: force the h1 invisible (as a
    // dissolve in flight would leave it) right as the boot is failing, and
    // require the catch path to put it back.
    const desktop = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    await desktop.route('**/ui/editor.html*', route => route.abort());
    await desktop.goto(baseURL);
    await desktop.evaluate(() => { const h1 = document.querySelector('#intro h1'); if (h1) h1.style.opacity = '0'; });
    await desktop.waitForSelector('html[data-static="1"]', { timeout: 15000 });
    const h1Opacity = await desktop.evaluate(() => getComputedStyle(document.querySelector('#intro h1')).opacity);
    if (h1Opacity !== '1') throw Error(`Desktop failed boot left the title at opacity ${h1Opacity}`);
    console.log(`${engine.name()}: desktop failed boot restores the title to visible`);
    await desktop.close();
  } finally { await browser.close(); }
}
} finally { await close(); }
