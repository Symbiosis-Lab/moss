#!/usr/bin/env node
import { pathToFileURL } from 'node:url';

const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const base = new URL(process.argv[2] || 'http://localhost:8080/');
const assert = (ok, message) => { if (!ok) throw new Error(message); };

async function state(page) {
  return page.evaluate(() => ({
    ...window.__state(),
    y: scrollY,
    max: document.documentElement.scrollHeight - innerHeight,
    closing: document.querySelector('#five').classList.contains('on'),
  }));
}

const browser = await engines[process.env.ENGINE || 'chromium'].launch({ headless: true });
try {
  const results = [];
  for (const test of [
    { name: 'desktop', viewport: { width: 1920, height: 1200 } },
    { name: 'mobile', viewport: { width: 390, height: 844 }, mobile: true },
  ]) {
    const page = await browser.newPage({ viewport: test.viewport, isMobile: !!test.mobile, hasTouch: !!test.mobile });
    await page.route(/\/ui\/(editor|shell)\.html/, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 3000));
      await route.continue();
    });
    const url = new URL(base);
    await page.goto(url.href, { waitUntil: 'commit' });
    await page.waitForSelector('#five');
    if (test.mobile && process.env.ENGINE === 'webkit') {
      // Playwright cannot synthesize a wheel in mobile WebKit; test native
      // document arrival directly. Chromium covers actual wheel input above.
      await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    } else {
      await page.mouse.wheel(0, 10000);
      await page.mouse.wheel(0, 10000);
    }
    await page.waitForTimeout(250);
    const cold = await state(page);
    assert(!cold.ready && cold.y === cold.max, `${test.name}: fixture delay did not hold cold load at bottom: ${JSON.stringify(cold)}`);
    assert(cold.shown === 4 && cold.xf === 1 && cold.closing, `${test.name}: closing missing before capture readiness`);

    await page.mouse.move(10, 100);
    await page.mouse.down();
    // Return to the actual scene 4 rest; footer height differs by viewport and locale.
    await page.evaluate(() => scrollTo(0, window.__restY(3)));
    await page.mouse.up();
    await page.waitForTimeout(200);
    const coldReverse = await state(page);
    assert(coldReverse.xf < cold.xf && !coldReverse.closing, `${test.name}: cold reverse did not uncover scene 4: ${JSON.stringify(coldReverse)}`);
    await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 20000 });
    await page.evaluate(() => scrollTo(0, window.__restY(3)));
    await page.waitForFunction(() => { const s = window.__state(); return s.shown === 3 && s.target === 3 && !s.running; }, null, { timeout: 10000 }).catch(async error => { throw new Error(`${test.name}: return failed ${JSON.stringify(await state(page))}`, {cause:error}); });

    await page.evaluate(() => scrollTo(0, window.__restY(1)));
    await page.waitForFunction(() => window.__state().running, null, { timeout: 5000 });
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForFunction(() => { const s = window.__state(); return s.shown === 4 && s.xf === 1 && !s.running; }, null, { timeout: 2000 }).catch(async error => { throw new Error(`${test.name}: active jump failed ${JSON.stringify(await state(page))}`, {cause:error}); });
    const activeJoin = await state(page);
    assert(activeJoin.closing, `${test.name}: closing remained queued behind active join`);
    results.push({ name: test.name, cold, coldReverse, activeJoin });
    await page.close();
  }
  console.log(JSON.stringify(results, null, 2));
} finally {
  await browser.close();
}
