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
    pageInert: document.querySelector('.page').inert,
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
    if (process.env.ENGINE === 'webkit') {
      // WebKit's synthetic wheel can coast short of the edge while a snap is
      // active. Test native document arrival directly; Chromium covers wheel.
      await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    } else {
      await page.mouse.wheel(0, 10000);
      await page.mouse.wheel(0, 10000);
    }
    // Desktop briefly (unit 4, 3efdf3e) pulled a burst like this back down to
    // restY(4)/closingRestY() on release, the same two-sided well every other
    // scene boundary uses -- correct for approaching the rest, wrong for a
    // reader who scrolled past it toward the footer, who got pulled straight
    // back every time (owner review, 2026-09-21: I-footer-reachable). The
    // well is one-sided for the closing scene now: nothing pulls back once
    // past restY(4), so a hard burst like this one rests exactly on the
    // document's own edge again, synchronously, on both layouts alike
    // (measured: still exactly at max from the first 16ms sample).
    await page.waitForTimeout(250);
    const cold = await state(page);
    assert(!cold.ready && cold.y === cold.max, `${test.name}: fixture delay did not hold cold load at bottom: ${JSON.stringify(cold)}`);
    assert(cold.shown === 4 && cold.xf === 1 && cold.closing, `${test.name}: closing missing before capture readiness`);

    await page.mouse.move(10, 100);
    await page.mouse.down();
    // Return to the actual scene 4 rest; footer height differs by viewport and locale.
    await page.evaluate(() => scrollTo(0, window.__landing.restY(3)));
    await page.mouse.up();
    await page.waitForTimeout(200);
    const coldReverse = await state(page);
    assert(coldReverse.xf < cold.xf && !coldReverse.closing && !coldReverse.pageInert, `${test.name}: cold reverse did not uncover scene 4: ${JSON.stringify(coldReverse)}`);
    await whenReady(page, { timeout: 20000 });
    await page.evaluate(() => scrollTo(0, window.__landing.restY(3)));
    await page.waitForFunction(() => { const s = window.__landing.state(); return s.shown === 3 && s.target === 3 && !s.running; }, null, { timeout: 10000 }).catch(async error => { throw new Error(`${test.name}: return failed ${JSON.stringify(await state(page))}`, {cause:error}); });

    await page.addStyleTag({ content: 'html { scroll-snap-type: none !important; }' });
    await page.evaluate(async () => {
      let lo = window.__landing.restY(3), hi = document.documentElement.scrollHeight - innerHeight;
      for (let i = 0; i < 12; i++) {
        const y = (lo + hi) / 2;
        scrollTo(0, y);
        await new Promise(requestAnimationFrame);
        if (window.__landing.state().progress < 3.5) lo = y; else hi = y;
      }
      scrollTo(0, (lo + hi) / 2);
    });
    await page.waitForTimeout(300);
    const activeWash = await page.evaluate(() => ({
      progress: window.__landing.state().progress,
      cover: window.__landing.state().xf,
      canvas: getComputedStyle(document.querySelector('#closing-wash')).display,
    }));
    assert(activeWash.progress > 3 && activeWash.progress < 4 && activeWash.cover > 0 && activeWash.cover < 1 && activeWash.canvas !== 'none', `${test.name}: midpoint did not present an active wash: ${JSON.stringify(activeWash)}`);
    await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForFunction(() => { const s = window.__landing.state(); return s.shown === 4 && s.xf === 1 && !s.running; }, null, { timeout: 10000 }).catch(async error => { throw new Error(`${test.name}: active jump failed ${JSON.stringify(await state(page))}`, {cause:error}); });
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
