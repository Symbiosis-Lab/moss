#!/usr/bin/env node
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const { chromium, webkit } = await loadPlaywright();
try {
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
    await page.goto(baseURL);
    await whenReady(page);
    for (const scene of [1, 2, 3]) {
      for (const fraction of [-.01, .5, 1.01, .5, -.01]) {
        await page.evaluate(({ scene, fraction }) => {
          const text = scenesEl[scene].firstElementChild.getBoundingClientRect(), band = mobileVisualBand();
          scrollTo(0, scrollY + text.top - band.bottom + fraction * (band.height + text.height));
        }, { scene, fraction });
        const expected = scene + Math.max(0, Math.min(1, fraction));
        await page.waitForFunction(expected => Math.abs(window.__landing.state().progress - expected) < .005, expected);
        await page.waitForTimeout(300);
        if (fraction === .5) {
          await page.waitForFunction(scene => {
            const state = window.__landing.state();
            return scene === 3 ? Math.abs(finalDissolve - .5) < .01 : state.running && Math.abs(state.washT - 1.05) < .06;
          }, scene, { timeout: 15000 });
        }
        const visible = await page.evaluate(scene => {
          const text = scenesEl[scene].firstElementChild;
          return getComputedStyle(page).opacity === '1' && getComputedStyle(text).opacity === '1';
        }, scene);
        if (!visible) throw Error(`Scene ${scene + 1} text faded with its background`);
      }
      console.log(`${engine.name()}: scene ${scene + 1} copy drives its outgoing morph, forward and reverse`);
    }
  } finally { await browser.close(); }
}
} finally { await close(); }
