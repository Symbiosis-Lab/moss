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
      await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 60000 });
      await page.evaluate(() => scrollTo(0, __restY(1) + 24));
      await page.waitForFunction(() => [0, 1, 2, 3].every(i => __onHand[i]), null, { timeout: 30000 });
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
      const buttonStart = scene === 2 ? await page.locator('#publish-bridge').boundingBox() : null;
      await page.evaluate(() => { window.__stepLimit = 12; });
      await page.waitForFunction(() => __state().steps >= 12);
      const start = await read();
      if (!(start.cover > 0 && start.cover < 1 && Number(start.scene) === scene)) throw Error('source was replaced before the wash covered it');
      await page.evaluate(() => { window.__stepLimit = 240; });
      await page.waitForFunction(() => __state().steps >= 240, null, { timeout: 15000 });
      const end = await read();
      if (!(end.cover > 0 && end.cover < 1 && Number(end.scene) === scene + 1)) throw Error(`target did not reveal gradually: ${JSON.stringify(end)}`);
      if (scene === 2) {
        const buttonEnd = await page.locator('#publish-bridge').boundingBox();
        if (!buttonStart || !buttonEnd || buttonEnd.width < buttonStart.width * 2) throw Error('solid Publish control did not grow through the wash');
      }
      await page.evaluate(() => { window.__stepClock = false; });
      console.log(`${engine.name()}: scene ${scene + 1} source preserved; target revealed gradually`);
      await page.close();
    }
    // A neighbour's print is taken by switching the live stage under a held
    // canvas; the scene on screen must not change size meanwhile (scene 4's
    // phone zoom, keyed on the stage's scene, once flashed over scene 2).
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
    await page.goto(url);
    await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 60000 });
    await page.evaluate(() => scrollTo(0, __restY(1) + 24));
    await page.waitForFunction(() => __state().shown === 1 && !__state().running && [0, 1, 2, 3].every(i => __onHand[i]), null, { timeout: 30000 });
    const retake = await page.evaluate(async () => {
      const scales = new Set(), cell = document.getElementById('cell');
      let watching = true;
      const tick = () => { if (!watching) return; const m = new DOMMatrix(getComputedStyle(cell).transform); scales.add(Math.hypot(m.a, m.b).toFixed(3)); requestAnimationFrame(tick); };
      tick();
      __onHand[3] = null;
      const t0 = performance.now();
      while (!__onHand[3] && performance.now() - t0 < 20000) await new Promise(r => setTimeout(r, 50));
      await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      watching = false;
      return { retaken: !!__onHand[3], scales: [...scales] };
    });
    if (!retake.retaken) throw Error('scene 4 print was not retaken from scene 2');
    if (retake.scales.length !== 1) throw Error(`scene 2 changed size while scene 4's print was taken: ${JSON.stringify(retake)}`);
    console.log(`${engine.name()}: taking a neighbour's print leaves the scene on screen unchanged`);
    await page.close();
  } finally { await browser.close(); }
}
