#!/usr/bin/env node
// Run against plain compiled output or the deployed site, not only moss's preview wrapper.
import { pathToFileURL } from 'node:url';

if (!process.argv[2]) throw new Error('Usage: node scripts/check-landing-transitions.mjs <site-url>');
const base = new URL(process.argv[2]);
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const locales = process.env.LOCALES === 'en' ? [''] : ['', 'zh-hant/', 'zh-hans/'];

for (const name of (process.env.ENGINE || 'chromium,webkit').split(',')) {
  const browser = await engines[name].launch();
  try {
    for (const locale of locales) {
      const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: 'no-preference' });
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.goto(new URL(locale, base).href);
      await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 30000 });
      const initial = await page.evaluate(() => ({ state: __state(), sheets: __onHand.slice(0, 4).map(Boolean) }));
      assert(initial.state.sim && initial.state.primed && initial.sheets.every(Boolean), `${name}/${locale}: wash captures unavailable: ${JSON.stringify(initial)}`);
      await page.evaluate(() => {
        window.__stageChanges = [];
        new MutationObserver(records => {
          for (const record of records) if (record.attributeName === 'data-scene') __stageChanges.push(record.target.dataset.scene);
        }).observe(document.getElementById('stage'), { attributes: true });
      });
      await page.waitForTimeout(2400);
      assert(await page.evaluate(() => __stageChanges.length === 0), `${name}/${locale}: idle capture switched the visible scene`);

      // Arm the carry without supplying a direction. A wheel delta is scene
      // intent now, even at one pixel, so using one here would move the exact
      // rest below toward the following scene instead of merely enabling it.
      await page.mouse.move(720, 20);
      await page.mouse.down();
      await page.mouse.up();
      for (const scene of [1, 2, 3, 2, 0]) {
        const previous = await page.evaluate(() => __state());
        const prior = previous.washes;
        await page.evaluate(scene => scrollTo(0, __restY(scene)), scene);
        // Returning from the orbit intentionally shrinks the surviving Publish
        // control into the preview instead of dissolving it.
        if (!(previous.shown === 3 && scene === 2)) {
          await page.waitForFunction(prior => __state().washes > prior && document.getElementById('stage').classList.contains('morphing'), prior, { timeout: 15000 });
          const visible = await page.evaluate(() => {
            const style = getComputedStyle(document.getElementById('gl'));
            return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) > 0;
          });
          assert(visible, `${name}/${locale}: wash ran without a visible canvas`);
        }
        await page.waitForFunction(scene => {
          const state = __state();
          return state.shown === scene && state.target === scene && !state.running;
        }, scene, { timeout: 30000 });
      }
      assert(errors.length === 0, `${name}/${locale}: ${errors.join('; ')}`);
      console.log(`${name}/${locale || 'en'}: stable idle scene and four visible watercolor transitions and Publish return`);
      await page.close();
    }
  } finally {
    await browser.close();
  }
}
