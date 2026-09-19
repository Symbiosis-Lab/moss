#!/usr/bin/env node
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-mobile-handoff.mjs <url>');
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    for (const scene of [1, 2]) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(url);
      await page.waitForFunction(() => window.__state?.().ready && [0, 1, 2, 3].every(i => __onHand[i]), null, { timeout: 60000 });
      await page.evaluate(s => scrollTo(0, __restY(s) + 24), scene);
      await page.waitForFunction(s => __state().shown === s && !__state().running, scene, { timeout: 15000 });
      const read = () => page.evaluate(() => ({
        scene: stage.dataset.scene, phase,
        cover: Number(stage.style.getPropertyValue('--wash-cover')),
        items: ['box', 'sib-nb', 'sib-sk', 's3-video'].map(id => {
          const e = document.getElementById(id), r = e.getBoundingClientRect(), c = getComputedStyle(e);
          return { id, x: r.x, y: r.y, w: r.width, h: r.height, opacity: c.opacity, visibility: c.visibility };
        })
      }));
      const before = await read();
      await page.evaluate(s => { window.__stepClock = true; window.__stepLimit = 0; __wash(s + 1); }, scene);
      await page.waitForTimeout(100);
      const zero = await read();
      if (JSON.stringify(before) !== JSON.stringify(zero)) throw Error(`source changed at zero progress: ${JSON.stringify({ scene, before, zero })}`);
      await page.evaluate(() => { window.__stepLimit = 12; });
      await page.waitForFunction(() => __state().steps >= 12);
      const start = await read();
      if (!(start.cover > 0 && start.cover < 1 && Number(start.scene) === scene)) throw Error('source was replaced before the wash covered it');
      await page.evaluate(() => { window.__stepLimit = 216; });
      await page.waitForFunction(() => __state().steps >= 216, null, { timeout: 15000 });
      const end = await read();
      if (!(end.cover > 0 && end.cover < 1 && Number(end.scene) === scene + 1)) throw Error(`target did not reveal gradually: ${JSON.stringify(end)}`);
      await page.evaluate(() => { window.__stepClock = false; });
      console.log(`${engine.name()}: scene ${scene + 1} source preserved; target revealed gradually`);
      await page.close();
    }
  } finally { await browser.close(); }
}
