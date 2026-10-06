#!/usr/bin/env node
// A mobile wash that fails must not stop the art answering the scroll: a
// frame that throws ends its transition at the target, and a lost GPU context
// turns later joins into cuts. Both leave the shown scene following the reader.
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-wash-recovery.mjs <url>');
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const withDebug = (u) => { const x = new URL(u); x.searchParams.set('washdbg', ''); return x.href; };

// Carry the reader, in small steps, until section `scene`'s copy has passed the
// visual by `fraction` of its passage (1.05: past it; below 0: short of it), then settle.
async function cross(page, scene, fraction = 1.05) {
  await page.evaluate(async ({ scene, fraction }) => {
    const text = scenesEl[scene].firstElementChild.getBoundingClientRect(), band = mobileVisualBand();
    const to = scrollY + text.top - band.bottom + fraction * (band.height + text.height), from = scrollY;
    let cover = 0, startedAt = null;
    for (let k = 1; k <= 40; k++) {
      scrollTo(0, from + (to - from) * k / 40); await new Promise((r) => requestAnimationFrame(r));
      cover = Math.max(cover, +getComputedStyle(document.getElementById('stage')).getPropertyValue('--wash-cover') || 0);
      // how far into this section's passage the join was first seen running
      if (startedAt === null && __state().running) startedAt = __state().progress - scene;
    }
    window.__crossCover = cover; window.__crossStarted = startedAt;
  }, { scene, fraction });
  await page.waitForTimeout(1500);
  return page.evaluate(() => ({ ...__state(), cover: window.__crossCover, startedAt: window.__crossStarted }));
}

for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    for (const failure of ['throwing frame', 'lost context', 'missing print']) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      // mobile defers scene 3's print until the reader has left scene 1
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      // rest in scene 2, its copy just short of the visual, until its neighbours' prints are warmed
      await cross(page, 1, -.08);
      await page.waitForFunction(() => __state().shown === 1 && !__state().running && __state().primed, null, { timeout: 20000 });
      if (failure === 'throwing frame') await page.evaluate(() => { const sim = __playback.sim, play = sim.play; sim.play = () => { sim.play = play; throw new Error('injected wash failure'); }; });
      else if (failure === 'lost context') await page.evaluate(() => document.getElementById('gl').getContext('webgl2').getExtension('WEBGL_lose_context').loseContext());
      // the next scene's print not taken yet: the wash must not wait for a capture
      else await page.evaluate(() => { sheets[2] = null; });
      const after = await cross(page, 1);
      assert(!after.running && after.shown === 2 && after.target === 2,
        `${engine.name()}: ${failure} left the art behind the scroll: ${JSON.stringify({ shown: after.shown, target: after.target, running: after.running })}`);
      // a dead canvas must not be raised over the live scene: the art would vanish for the crossing
      if (failure === 'lost context') assert(after.cover === 0, `${engine.name()}: the wash cover rose over a lost GPU context: ${after.cover}`);
      // without the incoming print the outgoing scene still dissolves as the copy arrives,
      // and the print is taken again once the reader stands in the scene
      if (failure === 'missing print') {
        assert(after.startedAt !== null && after.startedAt < .2 && after.cover > .5,
          `${engine.name()}: a missing print held the wash back: ${JSON.stringify({ startedAt: after.startedAt, cover: after.cover })}`);
        await page.waitForFunction(() => !!sheets[2], null, { timeout: 15000 });
      }
      // and the next boundary still follows the reader
      const next = await cross(page, 2);
      assert(!next.running && next.shown === next.target && next.shown >= 3,
        `${engine.name()}: after a ${failure} the next join did not follow: ${JSON.stringify({ shown: next.shown, target: next.target, running: next.running })}`);
      console.log(`${engine.name()}: ${failure} ends at its target; the next join follows`);
      await page.close();
    }
  } finally { await browser.close(); }
}
