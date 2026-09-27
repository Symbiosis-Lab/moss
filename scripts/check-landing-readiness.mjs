#!/usr/bin/env node
import { pathToFileURL } from 'node:url';
import { readFile } from 'node:fs/promises';
const name = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(name.startsWith('/') ? pathToFileURL(name).href : name);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-readiness.mjs <url>');
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
    if (process.env.LANDING_HTML_OVERRIDE) {
      const body = await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8');
      await page.route(url, route => route.fulfill({ contentType: 'text/html', body }));
    }
    await page.goto(url);
    await page.waitForFunction(() => window.__state?.().ready);
    await page.evaluate(() => scrollTo(0, __restY(2)));
    await page.waitForFunction(() => __state().shown === 2 && !__state().running, null, { timeout: 30000 });
    // A cold jump can reach the creative scene before its next print exists.
    await page.evaluate(() => { __onHand[3] = null; scrollTo(0, __restY(3)); });
    await page.waitForFunction(() => __state().shown === 3 && !__state().running, null, { timeout: 15000 });
    console.log(`${engine.name()}: missing next print recovered without another gesture`);
    await page.close();
    for (const javaScriptEnabled of [true, false]) {
      const fallback = await browser.newPage({ javaScriptEnabled, viewport: { width: 390, height: 844 } });
      if (javaScriptEnabled) await fallback.route('**/ui/editor.html*', route => route.abort());
      await fallback.goto(url);
      if (javaScriptEnabled) await fallback.waitForSelector('html[data-static="1"]', { timeout: 15000 });
      const usable = await fallback.evaluate(() => {
        const closing = document.querySelector('#five');
        return getComputedStyle(closing).opacity === '1' && !closing.inert && getComputedStyle(document.querySelector('.page')).display !== 'none';
      });
      if (!usable) throw Error('Static landing content is hidden');
      console.log(`${engine.name()}: ${javaScriptEnabled ? 'failed demo' : 'no JavaScript'} leaves copy and downloads readable`);
      await fallback.close();
    }
  } finally { await browser.close(); }
}
