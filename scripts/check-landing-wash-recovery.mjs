#!/usr/bin/env node
// When the watercolor cannot run a transition (a frame that throws, a lost GPU context, a print that
// is missing or fails to draw) the art must still end on the scene being read, and the next change
// must still follow the reader.
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-wash-recovery.mjs <url>');
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const withFree = (u) => { const x = new URL(u); x.searchParams.set('snap', 'off'); return x.href; };

const go = (page, scene) => page.evaluate((s) => scrollTo({ top: __restY(s), behavior: 'instant' }), scene);
const state = (page) => page.evaluate(() => ({ ...__state(), washError, simLost }));
async function arrive(page, scene, label) {
  await page.waitForFunction((s) => { const t = __state(); return t.shown === s && t.target === s && !t.leg; }, scene, { timeout: 40000 })
    .catch(async () => { throw new Error(`${label}: the art did not end on scene ${scene + 1}: ${JSON.stringify(await state(page))}`); });
}
// scroll to a scene and wait until its leg is under way
const setOff = async (page, scene) => { await go(page, scene); await page.waitForFunction(() => __state().leg?.p > .1, null, { timeout: 40000 }); };
const lose = (page) => page.evaluate(() => document.getElementById('gl').getContext('webgl2').getExtension('WEBGL_lose_context').loseContext());

const cases = {
  'throwing frame': async (page) => {
    await page.evaluate(() => { sim.playSides = () => { throw new Error('injected'); }; });
    await go(page, 1); await arrive(page, 1, 'throwing frame');
    assert((await state(page)).washError === 'injected', 'throwing frame: the failure was not recorded');
    await go(page, 2); await arrive(page, 2, 'throwing frame, next change');
  },
  'lost context': async (page) => {
    await lose(page);
    const before = (await state(page)).dissolves;
    await go(page, 1); await arrive(page, 1, 'lost context');
    assert((await state(page)).dissolves > before, 'lost context: the change popped instead of dissolving');
  },
  'lost mid-leg': async (page) => {
    await setOff(page, 1); await lose(page); await arrive(page, 1, 'lost mid-leg');
  },
  'restored mid-leg': async (page) => {
    await setOff(page, 1);
    // Frames are held while the context is away, so the leg is still in flight when it returns; the
    // state is read in the restore event itself, so the leg must be landed then, not finish later.
    const at = await page.evaluate(async () => {
      const gl = document.getElementById('gl'), ext = gl.getContext('webgl2').getExtension('WEBGL_lose_context'), raf = window.requestAnimationFrame, held = [];
      window.requestAnimationFrame = (f) => held.push(f);
      await new Promise((r) => raf(r));
      ext.loseContext(); await new Promise((r) => setTimeout(r, 300));
      const at = await new Promise((done) => { gl.addEventListener('webglcontextrestored', () => done({ leg: __state().leg, shown: __state().shown }), { once: true }); ext.restoreContext(); });
      window.requestAnimationFrame = raf; held.forEach((f) => raf(f));
      return at;
    });
    assert(!at.leg && at.shown === 1, `restored mid-leg: the leg was not landed on its scene: ${JSON.stringify(at)}`);
    await arrive(page, 1, 'restored mid-leg');
    await page.waitForFunction(() => !simLost && sim, null, { timeout: 10000 });
    await go(page, 2); await page.waitForFunction(() => __state().leg, null, { timeout: 40000 }).catch(() => { throw new Error('restored mid-leg: later changes do not wash again'); });
    await arrive(page, 2, 'restored mid-leg, next change');
  },
  'missing print': async (page) => {
    await page.evaluate(() => { __onHand[1] = null; });
    await go(page, 1); await arrive(page, 1, 'missing print');
    await page.waitForFunction(() => !!__onHand[1], null, { timeout: 20000 }).catch(() => { throw new Error('missing print: it was never taken again'); });
  },
  'failed capture': async (page) => {
    await page.evaluate(() => { __onHand[1] = null; const real = capture; capture = async () => { capture = real; throw new Error('injected'); }; });
    const before = (await state(page)).dissolves;
    await go(page, 1); await arrive(page, 1, 'failed capture');
    assert((await state(page)).dissolves > before, 'failed capture: the leg did not cut with a dissolve');
  },
  // a cut lands like an arrival: back in scene 4 from a close that failed, its logos are let go
  'cut back from the close': async (page) => {
    await go(page, 3); await arrive(page, 3, 'cut back from the close');
    await setOff(page, 4);
    await page.evaluate(() => { sim.playSides = () => { throw new Error('injected'); }; });
    await go(page, 3); await arrive(page, 3, 'cut back from the close');
    await page.waitForFunction(() => !orbitFrozen && !deployOut && __orbit().running, null, { timeout: 10000 })
      .catch(async () => { throw new Error(`cut back from the close: scene 4's logos stayed frozen: ${JSON.stringify(await page.evaluate(() => ({ frozen: orbitFrozen, held: !!deployOut, running: __orbit().running })))}`); });
  },
  'fling past scenes with no prints': async (page) => {
    await page.evaluate(() => { __onHand[1] = null; __onHand[2] = null; });
    for (const s of [1, 2, 3]) { await go(page, s); await page.waitForTimeout(150); }
    await arrive(page, 3, 'fling past scenes with no prints');
  },
};

for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    for (const [name, run] of Object.entries(cases)) {
      const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
      const errors = [];
      page.on('pageerror', (e) => errors.push(e.message));
      await page.goto(withFree(url));
      await page.waitForFunction(() => document.documentElement.dataset.ready === '1' && __onHand.slice(0, 4).every(Boolean), null, { timeout: 60000 });
      await go(page, 0); await arrive(page, 0, name);
      await run(page);
      assert(!errors.length, `${engine.name()}: ${name}: ${errors.join('; ')}`);
      await page.close();
    }
    console.log(`${engine.name()}: ${Object.keys(cases).join(', ')} all end on the scene being read`);
  } finally { await browser.close(); }
}
