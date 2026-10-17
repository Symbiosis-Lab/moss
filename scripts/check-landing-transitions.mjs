#!/usr/bin/env node
// Walks the scenes in each locale on a desktop viewport: an idle page changes nothing, every change
// runs as a leg over a visible canvas, the real Publish control crosses between scenes 3 and 4, and
// scene 4's logos come out of it on arrival.
import { pathToFileURL } from 'node:url';

if (!process.argv[2]) throw new Error('Usage: node scripts/check-landing-transitions.mjs <site-url>');
const base = new URL(process.argv[2]);
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const locales = process.env.LOCALES === 'en' ? [''] : ['', 'zh-hant/', 'zh-hans/'];

// Scroll to a scene and report what was on screen while the change ran.
const walk = (page, scene) => page.evaluate((scene) => new Promise((done) => {
  const seen = { leg: false, canvas: false, bridge: false };
  scrollTo({ top: __restY(scene), behavior: 'instant' });
  (function tick() {
    const s = __state();
    if (s.leg) {
      seen.leg = true;
      const gl = getComputedStyle(document.getElementById('gl'));
      if (gl.display !== 'none' && gl.visibility !== 'hidden' && +gl.opacity > 0) seen.canvas = true;
      if (document.getElementById('publish-bridge')) seen.bridge = true;
    } else if (s.shown === scene && s.target === scene) return done(seen);
    requestAnimationFrame(tick);
  })();
}), scene);

const logosOut = (page, where) => page.waitForFunction(() => { const o = __orbit(); return o.nodes?.length && o.nodes.every((n) => n.popAt && Math.hypot(n.x - o.cx, n.y - o.cy) > o.buttonR); }, null, { timeout: 10000 })
  .catch(async () => { throw new Error(`${where}: the logos did not come out on arrival: ${JSON.stringify(await page.evaluate(() => __orbit()))}`); });

for (const name of (process.env.ENGINE || 'chromium,webkit').split(',')) {
  const browser = await engines[name].launch();
  try {
    for (const locale of locales) {
      const where = `${name}/${locale || 'en'}`;
      const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: 'no-preference' });
      const errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.goto(new URL(locale, base).href);
      await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 60000 });
      const first = await page.evaluate(() => ({ ...__state(), prints: __onHand.slice(0, 4).map(Boolean) }));
      assert(first.sim && first.primed && first.prints.every(Boolean), `${where}: no prints to wash with: ${JSON.stringify(first)}`);
      await page.waitForTimeout(2400);
      const idle = await page.evaluate(() => __state());
      assert(!idle.leg && idle.shown === 0 && idle.dissolves === first.dissolves, `${where}: an idle page changed: ${JSON.stringify(idle)}`);

      let at = 0;
      for (const scene of [1, 2, 3, 2, 0]) {
        const seen = await walk(page, scene);
        assert(seen.leg && seen.canvas, `${where}: scene ${at + 1} to ${scene + 1} ran without a visible canvas: ${JSON.stringify(seen)}`);
        assert(seen.bridge === (at + scene === 5), `${where}: the Publish control crossed wrongly from ${at + 1} to ${scene + 1}: ${JSON.stringify(seen)}`);
        if (scene === 3) await logosOut(page, where);
        at = scene;
      }
      // scene 4 left the moment it arrives, before its logos have run, still brings them out on the way back
      await walk(page, 2); await walk(page, 3);
      await page.evaluate(() => scrollTo({ top: __restY(2), behavior: 'instant' }));
      await page.waitForFunction(() => (__state().leg?.p ?? 0) > .3, null, { timeout: 8000 });
      await walk(page, 3);
      await logosOut(page, `${where}, back in scene 4 after leaving it on arrival`);
      assert(errors.length === 0, `${where}: ${errors.join('; ')}`);
      console.log(`${where}: idle page still; five changes each a visible leg; Publish crosses 3 and 4; logos come out, also after scene 4 was left on arrival`);
      await page.close();
    }
  } finally {
    await browser.close();
  }
}
