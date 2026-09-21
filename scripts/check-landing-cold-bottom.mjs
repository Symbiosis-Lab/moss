#!/usr/bin/env node
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const base = new URL(baseURL);
const engines = await loadPlaywright();
const assert = (ok, message) => { if (!ok) throw new Error(message); };

async function state(page) {
  return page.evaluate(() => ({
    ...window.__landing.state(),
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
    if (test.mobile) {
      // Mobile's nativeScroll() stays true through the close (unit 4 left it
      // untouched), so a real wheel/scroll burst still lands on the browser's
      // own document edge synchronously -- the old fixed wait is still enough.
      await page.waitForTimeout(250);
    } else {
      // Desktop now commits the DEPLOY..SHARE boundary the same way as every
      // other scene boundary (unit 4, review-phases-2-4.md Job 2 item 5): a
      // release pulls back to restY(4)/closingRestY(), which by design sits
      // short of document.scrollHeight itself (closingRestY()'s own docblock:
      // "the footer remains a natural scroll away") -- a burst no longer rests
      // on the literal max. The pull also takes measurably longer than the old
      // synchronous native jump (measured 2026-09-21: still 39px short at
      // 250ms, exactly on restY(4) by ~1.5s), so wait for the spring to say
      // it's actually arrived rather than reusing the old fixed delay.
      await page.waitForFunction(() => {
        const s = window.__landing.state();
        return !s.running && Math.abs(scrollY - window.__landing.restY(4)) <= 2;
      }, null, { timeout: 2500 }).catch(async error => { throw new Error(`${test.name}: never settled at its committed rest: ${JSON.stringify(await state(page))}`, { cause: error }); });
    }
    const cold = await state(page);
    if (test.mobile) {
      assert(!cold.ready && cold.y === cold.max, `${test.name}: fixture delay did not hold cold load at bottom: ${JSON.stringify(cold)}`);
    } else {
      const restShare = await page.evaluate(() => window.__landing.restY(4));
      assert(!cold.ready && Math.abs(cold.y - restShare) <= 2, `${test.name}: fixture delay did not hold cold load at its committed rest: ${JSON.stringify({ ...cold, restShare })}`);
    }
    assert(cold.shown === 4 && cold.xf === 1 && cold.closing, `${test.name}: closing missing before capture readiness`);

    await page.mouse.move(10, 100);
    await page.mouse.down();
    // Return to the actual scene 4 rest; footer height differs by viewport and locale.
    await page.evaluate(() => scrollTo(0, window.__landing.restY(3)));
    await page.mouse.up();
    await page.waitForTimeout(200);
    const coldReverse = await state(page);
    assert(coldReverse.xf < cold.xf && !coldReverse.closing, `${test.name}: cold reverse did not uncover scene 4: ${JSON.stringify(coldReverse)}`);
    await whenReady(page, { timeout: 20000 });
    await page.evaluate(() => scrollTo(0, window.__landing.restY(3)));
    await page.waitForFunction(() => { const s = window.__landing.state(); return s.shown === 3 && s.target === 3 && !s.running; }, null, { timeout: 10000 }).catch(async error => { throw new Error(`${test.name}: return failed ${JSON.stringify(await state(page))}`, {cause:error}); });

    await page.evaluate(() => scrollTo(0, window.__landing.restY(1)));
    await page.waitForFunction(() => window.__landing.state().running, null, { timeout: 5000 });
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForFunction(() => { const s = window.__landing.state(); return s.shown === 4 && s.xf === 1 && !s.running; }, null, { timeout: 2000 }).catch(async error => { throw new Error(`${test.name}: active jump failed ${JSON.stringify(await state(page))}`, {cause:error}); });
    const activeJoin = await state(page);
    assert(activeJoin.closing, `${test.name}: closing remained queued behind active join`);
    results.push({ name: test.name, cold, coldReverse, activeJoin });
    await page.close();
  }
  console.log(JSON.stringify(results, null, 2));
} finally {
  await browser.close();
  await close();
}
