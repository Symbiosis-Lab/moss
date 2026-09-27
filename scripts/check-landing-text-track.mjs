#!/usr/bin/env node
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-text-track.mjs <url>');
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
    await page.goto(url);
    await page.waitForFunction(() => window.__state?.().ready);
    for (const scene of [0, 1, 2, 3]) {
      let held = null;
      for (const fraction of [-.01, .5, 1.01, .5, -.01]) {
        await page.evaluate(({ scene, fraction }) => {
          const text = scenesEl[scene].firstElementChild.getBoundingClientRect(), band = mobileVisualBand();
          scrollTo(0, scrollY + text.top - band.bottom + fraction * (band.height + text.height));
        }, { scene, fraction });
        const expected = scene + Math.max(0, Math.min(1, fraction));
        await page.waitForFunction(expected => Math.abs(__state().progress - expected) < .005, expected);
        await page.waitForTimeout(300);
        if (fraction === .5) {
          await page.waitForFunction(scene => {
            const state = __state();
            return scene === 3 ? Math.abs(finalDissolve - .5) < .01 : state.running && Math.abs(state.washT - 1.05) < .06;
          }, scene, { timeout: 15000 });
        }
        const visible = await page.evaluate(scene => {
          const text = scenesEl[scene].firstElementChild;
          return getComputedStyle(page).opacity === '1' && getComputedStyle(text).opacity === '1';
        }, scene);
        if (!visible) throw Error(`Scene ${scene + 1} text faded with its background`);
        if (scene === 3 && fraction === .5) {
          // The close dissolves a frozen composition: through scene 5 and back the
          // targets are where its print took them, so the print gathers into them.
          const now = await page.evaluate(() => { window.__closePrint = finalPrints.source; return JSON.stringify(__orbit().nodes.map(n => [Math.round(n.x * 10), Math.round(n.y * 10), !!n.popAt])); });
          if (held && now !== held) throw Error('scene 4 targets moved or reset while the close held their print');
          held = now;
        }
      }
      if (scene === 3) {
        // Back in scene 4 the targets move again, the rest of them still arrive,
        // and the next close takes a fresh print.
        const out = await page.evaluate(() => __orbit().nodes.filter(n => n.popAt).length);
        await page.waitForFunction(() => __orbit().nodes.every(n => n.popAt), null, { timeout: 30000 })
          .catch(() => { throw Error(`scene 4 stopped releasing targets after the close: ${out} were out when it let go`); });
        await page.waitForTimeout(1500);
        await page.evaluate(() => { const text = scenesEl[3].firstElementChild.getBoundingClientRect(), band = mobileVisualBand(); scrollTo(0, scrollY + text.top - band.bottom + .5 * (band.height + text.height)); });
        await page.waitForFunction(() => finalPrints?.source, null, { timeout: 15000 });
        if (await page.evaluate(() => finalPrints.source === window.__closePrint)) throw Error('the next close reused a print of targets that had moved since');
      }
      console.log(`${engine.name()}: scene ${scene + 1} copy drives its outgoing morph, forward and reverse${scene === 3 ? '; the close holds its composition and lets go' : ''}`);
    }
  } finally { await browser.close(); }
}
