#!/usr/bin/env node
// A page loaded at the bottom stands on the close before anything has loaded, uncovers scene 4 when the
// reader scrolls back up (all before boot), and a later jump from scene 2 to the bottom ends on the close.
import { pathToFileURL } from 'node:url';

const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const base = new URL(process.argv[2] || 'http://localhost:8080/');
const assert = (ok, message) => { if (!ok) throw new Error(message); };

const state = (page) => page.evaluate(() => ({ ...__state(), y: scrollY, rest: __restY(4), closing: document.querySelector('#five').classList.contains('on') }));
// Wait for a smooth scroll to finish: End reports xf = 1 before the page has arrived.
const still = (page) => page.evaluate(() => new Promise((done) => {
  let last = -1, since = performance.now();
  (function tick() { if (scrollY !== last) { last = scrollY; since = performance.now(); } else if (performance.now() - since > 500) return done(); requestAnimationFrame(tick); })();
}));

// Desktop runs in WebKit unless ENGINE says otherwise: headless Chromium draws this page at about a
// frame a second, too slowly for the picture to arrive.
const engineFor = (test) => process.env.ENGINE || (test.mobile ? 'chromium' : 'webkit');
let browser;
try {
  for (const test of [
    { name: 'desktop', viewport: { width: 1920, height: 1200 } },
    { name: 'mobile', viewport: { width: 390, height: 844 }, mobile: true },
  ]) {
    browser = await engines[engineFor(test)].launch({ headless: true });
    const page = await browser.newPage({ viewport: test.viewport, isMobile: !!test.mobile, hasTouch: !!test.mobile });
    // holds the demos back; a demo that takes ten seconds ends the boot, so no longer than this
    await page.route(/\/ui\/(editor|shell)\.html/, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, test.mobile ? 3000 : 8000));
      await route.continue();
    });
    await page.goto(base.href, { waitUntil: 'commit' });
    await page.waitForSelector('#five');
    if (test.mobile) await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
    else await page.keyboard.press('End');
    await page.waitForFunction(() => window.__state?.().xf === 1, null, { timeout: 20000 });
    await still(page);
    const cold = await state(page);
    assert(!cold.ready && cold.y >= cold.rest - 1, `${test.name}: not cold at the bottom: ${JSON.stringify(cold)}`);
    assert(cold.shown === 4 && cold.xf === 1 && cold.closing, `${test.name}: the close does not stand before boot: ${JSON.stringify(cold)}`);

    await page.evaluate(() => scrollTo({ top: __restY(3), behavior: 'instant' }));
    await page.waitForFunction(() => window.__state().xf < 1, null, { timeout: 5000 }).catch(() => {});
    const back = await state(page);
    assert(!back.ready && back.xf < 1 && !back.closing, `${test.name}: scrolling up before boot did not uncover scene 4: ${JSON.stringify(back)}`);

    await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 60000 });
    await page.waitForFunction(() => { const s = __state(); return s.shown === 3 && s.target === 3 && !s.leg; }, null, { timeout: 40000 })
      .catch(async () => { throw new Error(`${test.name}: did not return to scene 4: ${JSON.stringify(await state(page))}`); });

    // from scene 2 to the bottom in one jump
    await page.evaluate(() => scrollTo({ top: __restY(1), behavior: 'instant' }));
    await page.waitForFunction(() => { const s = __state(); return s.shown === 1 && s.target === 1 && !s.leg; }, null, { timeout: 40000 });
    await page.evaluate(() => scrollTo({ top: document.documentElement.scrollHeight, behavior: 'instant' }));
    await page.waitForFunction(() => { const s = __state(); return s.shown === 4 && s.xf === 1 && !s.leg; }, null, { timeout: 40000 })
      .catch(async () => { throw new Error(`${test.name}: the jump to the bottom did not end on the close: ${JSON.stringify(await state(page))}`); });
    assert((await state(page)).closing, `${test.name}: the close is not standing after the jump`);
    console.log(`${engineFor(test)}: ${test.name}: close stands cold, uncovers on the way up, and follows a jump from scene 2`);
    await browser.close();
  }
} finally {
  await browser?.close().catch(() => {});
}
