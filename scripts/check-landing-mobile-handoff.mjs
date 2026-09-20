#!/usr/bin/env node
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const { chromium, webkit } = await loadPlaywright();
try {
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    for (const scene of [1, 2]) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(baseURL);
      await whenReady(page, { timeout: 60000 });
      await page.evaluate(() => scrollTo(0, window.__landing.restY(1) + 24));
      await page.waitForFunction(() => [0, 1, 2, 3].every(i => window.__landing.prints[i]), null, { timeout: 30000 });
      await page.evaluate(s => scrollTo(0, window.__landing.restY(s) + 24), scene);
      await page.waitForFunction(s => window.__landing.state().shown === s && !window.__landing.state().running, scene, { timeout: 15000 });
      const read = () => page.evaluate(() => ({
        scene: stage.dataset.scene, phase,
        cover: Number(stage.style.getPropertyValue('--wash-cover')),
        items: ['box', 'sib-nb', 'sib-sk', 's3-video'].map(id => {
          const e = document.getElementById(id), r = e.getBoundingClientRect(), c = getComputedStyle(e);
          return { id, x: r.x, y: r.y, w: r.width, h: r.height, opacity: c.opacity, visibility: c.visibility };
        })
      }));
      const before = await read();
      await page.evaluate(s => { window.__landing.stepClock = true; window.__landing.stepLimit = 0; window.__landing.wash(s + 1); }, scene);
      await page.waitForTimeout(100);
      const zero = await read();
      if (JSON.stringify(before) !== JSON.stringify(zero)) throw Error(`source changed at zero progress: ${JSON.stringify({ scene, before, zero })}`);
      const buttonStart = scene === 2 ? await page.locator('#publish-bridge').boundingBox() : null;
      await page.evaluate(() => { window.__landing.stepLimit = 12; });
      await page.waitForFunction(() => window.__landing.state().steps >= 12);
      const start = await read();
      if (!(start.cover > 0 && start.cover < 1 && Number(start.scene) === scene)) throw Error('source was replaced before the wash covered it');
      await page.evaluate(() => { window.__landing.stepLimit = 240; });
      await page.waitForFunction(() => window.__landing.state().steps >= 240, null, { timeout: 15000 });
      const end = await read();
      if (!(end.cover > 0 && end.cover < 1 && Number(end.scene) === scene + 1)) throw Error(`target did not reveal gradually: ${JSON.stringify(end)}`);
      if (scene === 2) {
        const buttonEnd = await page.locator('#publish-bridge').boundingBox();
        if (!buttonStart || !buttonEnd || buttonEnd.width < buttonStart.width * 2) throw Error('solid Publish control did not grow through the wash');
      }
      await page.evaluate(() => { window.__landing.stepClock = false; });
      console.log(`${engine.name()}: scene ${scene + 1} source preserved; target revealed gradually`);
      await page.close();
    }
  } finally { await browser.close(); }
}
} finally { await close(); }
