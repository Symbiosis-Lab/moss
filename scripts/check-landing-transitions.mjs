#!/usr/bin/env node
// Run against plain compiled output or the deployed site, not only moss's preview wrapper.
import { loadPlaywright, resolveBaseURL, trackErrors, whenReady } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const base = new URL(baseURL);
const engines = await loadPlaywright();
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const locales = process.env.LOCALES === 'en' ? [''] : ['', 'zh-hant/', 'zh-hans/'];

try {
for (const name of (process.env.ENGINE || 'chromium,webkit').split(',')) {
  const browser = await engines[name].launch();
  try {
    for (const locale of locales) {
      const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: 'no-preference' });
      const errors = trackErrors(page);
      await page.goto(new URL(locale, base).href);
      await whenReady(page);
      const initial = await page.evaluate(() => ({ state: window.__landing.state(), sheets: window.__landing.prints.slice(0, 4).map(Boolean) }));
      assert(initial.state.sim && initial.state.primed && initial.sheets.every(Boolean), `${name}/${locale}: wash captures unavailable: ${JSON.stringify(initial)}`);
      // The idle clause, restated as R8 actually states it: simulation
      // steps stay at zero, and captures run at most one per second (warm()'s
      // own WARM_MS pacing) and only of the scene on screen -- not just "the
      // visible scene never changed", which a capture of some other scene
      // running in the background would still satisfy.
      await page.evaluate(() => {
        window.__stageChanges = [];
        new MutationObserver(records => {
          for (const record of records) if (record.attributeName === 'data-scene') __stageChanges.push(record.target.dataset.scene);
        }).observe(document.getElementById('stage'), { attributes: true });
      });
      const idleMs = 2400;
      const samples = [];
      for (let elapsed = 0; elapsed < idleMs; elapsed += 100) {
        samples.push(await page.evaluate(() => ({ steps: window.__landing.state().steps, captureMs: window.__landing.state().captureMs })));
        await page.waitForTimeout(100);
      }
      assert(await page.evaluate(() => __stageChanges.length === 0), `${name}/${locale}: idle capture switched the visible scene`);
      assert(samples.every((s) => s.steps === 0), `${name}/${locale}: simulation steps moved at rest: ${JSON.stringify(samples.map((s) => s.steps))}`);
      const captureCount = samples.filter((s, i) => i > 0 && s.captureMs !== samples[i - 1].captureMs).length;
      const maxCaptures = Math.ceil(idleMs / 1000) + 1; // WARM_MS=1000 paces one capture a second; +1 for the sampling grid's own edges
      assert(captureCount <= maxCaptures, `${name}/${locale}: ${captureCount} captures in ${idleMs}ms, over the one-a-second budget`);

      // Arm the carry without supplying a direction. A wheel delta is scene
      // intent now, even at one pixel, so using one here would move the exact
      // rest below toward the following scene instead of merely enabling it.
      await page.mouse.move(720, 20);
      await page.mouse.down();
      await page.mouse.up();
      // Each leg is driven by real scroll, a few pixels a frame, not by one
      // jump to the next rest: a transition is presented by position now, so
      // a jump shows its last frame on the first one and a wash is never on
      // screen long enough to be seen.
      for (const scene of [1, 2, 3, 2, 0]) {
        const previous = await page.evaluate(() => window.__landing.state());
        const prior = previous.washes;
        // The Publish control stays solid and travels between the preview and the
        // orbit in both directions, so neither leg of that swap is a pigment wash.
        const carriesPublish = Math.min(previous.shown, scene) === 2 && Math.max(previous.shown, scene) === 3;
        // One wheel tick in the leg's direction says which way the reader is
        // going (a scrollTo alone never does), then the page goes back to its rest.
        await page.mouse.wheel(0, Math.sign(scene - previous.shown) * 40);
        await page.evaluate((from) => scrollTo(0, window.__landing.restY(from)), previous.shown);
        const seen = await page.evaluate(async (scene) => {
          const goal = window.__landing.restY(scene), frame = () => new Promise((r) => requestAnimationFrame(r));
          let y = scrollY, washing = 0, canvas = 0;
          // A pointer held on the scrollbar is direct manipulation: the carry
          // spring leaves the page where the scroll puts it, as it would for
          // a reader dragging the thumb.
          const hold = (type) => dispatchEvent(new PointerEvent(type, { clientX: 1e5, clientY: 10, pointerType: 'mouse', button: 0 }));
          hold('pointerdown');
          while (Math.abs(goal - y) > 0.5) {
            y += Math.sign(goal - y) * Math.min(12, Math.abs(goal - y));
            scrollTo(0, Math.round(y));
            await frame();
            const stage = document.getElementById('stage'), style = getComputedStyle(document.getElementById('gl'));
            if (stage.classList.contains('morphing')) {
              washing++;
              if (style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) > 0) canvas++;
            }
          }
          hold('pointerup');
          return { washing, canvas };
        }, scene);
        if (!carriesPublish) {
          await page.waitForFunction(prior => window.__landing.state().washes > prior, prior, { timeout: 15000 });
          assert(seen.washing > 3, `${name}/${locale}: a scroll through the leg to scene ${scene} showed the wash on ${seen.washing} frames`);
          assert(seen.canvas === seen.washing, `${name}/${locale}: wash ran without a visible canvas on ${seen.washing - seen.canvas} of ${seen.washing} frames`);
        }
        await page.waitForFunction(scene => {
          const state = window.__landing.state();
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
} finally { await close(); }
