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

// Carry the reader until section `scene`'s copy has passed the visual by `fraction` of its passage
// (1.05: past it; below 0: short of it), in 40 even steps or as an 800 ms fling that slows to a
// stop, then settle for `settle` ms.
async function cross(page, scene, fraction = 1.05, { fling = false, settle = 1500 } = {}) {
  await page.evaluate(async ({ scene, fraction, fling }) => {
    const text = scenesEl[scene].firstElementChild.getBoundingClientRect(), band = mobileVisualBand();
    const to = scrollY + text.top - band.bottom + fraction * (band.height + text.height), from = scrollY, t0 = performance.now();
    let cover = 0, startedAt = null;
    for (let k = 1, u = 0; u < 1; k++) {
      u = fling ? Math.min(1, (performance.now() - t0) / 800) : k / 40;
      scrollTo(0, from + (to - from) * (fling ? 1 - (1 - u) ** 3 : u)); await new Promise((r) => requestAnimationFrame(r));
      cover = Math.max(cover, +getComputedStyle(document.getElementById('stage')).getPropertyValue('--wash-cover') || 0);
      // how far into this section's passage the join was first seen running
      if (startedAt === null && __state().running) startedAt = __state().progress - scene;
    }
    window.__crossCover = cover; window.__crossStarted = startedAt;
  }, { scene, fraction, fling });
  await page.waitForTimeout(settle);
  return page.evaluate(() => ({ ...__state(), cover: window.__crossCover, startedAt: window.__crossStarted }));
}

for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    for (const failure of ['throwing frame', 'lost context', 'missing print', 'restored context', 'failed capture']) {
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
      else if (failure === 'missing print') await page.evaluate(() => { sheets[2] = null; });
      // The browser hands a lost context back (a phone short of GPU memory does, and iOS Safari does it
      // often): the morph must come back with it, not stay cut until the page is reloaded.
      else if (failure === 'restored context') await page.evaluate(async () => {
        const lose = document.getElementById('gl').getContext('webgl2').getExtension('WEBGL_lose_context'), nap = (ms) => new Promise((r) => setTimeout(r, ms));
        lose.loseContext(); await nap(300); lose.restoreContext(); await nap(800);
      });
      // One print that fails to draw (a first visit still loading its images and fonts) must not switch
      // the morph off for the rest of the visit: the page tries again by itself.
      else {
        await page.evaluate(() => { sheets[2] = null; const real = capture; capture = async () => { capture = real; throw new Error('injected capture failure'); }; });
        await page.waitForFunction(() => printable && !!sheets[2], null, { timeout: 30000 });
      }
      const washesBefore = await page.evaluate(() => __state().washes), dissolvesBefore = await page.evaluate(() => __state().dissolves);
      const after = await cross(page, 1);
      assert(!after.running && after.shown === 2 && after.target === 2,
        `${engine.name()}: ${failure} left the art behind the scroll: ${JSON.stringify({ shown: after.shown, target: after.target, running: after.running })}`);
      // a dead canvas must not be raised over the live scene: the art would vanish for the crossing
      // a join the morph cannot make dissolves from the print of the scene it leaves, not a pop
      if (failure === 'lost context') assert(after.dissolves > dissolvesBefore,
        `${engine.name()}: a join cut for want of the GPU popped instead of dissolving: ${JSON.stringify({ dissolves: after.dissolves, dissolvesBefore })}`);
      if (failure === 'lost context') assert(after.cover === 0, `${engine.name()}: the wash cover rose over a lost GPU context: ${after.cover}`);
      if (failure === 'restored context' || failure === 'failed capture') assert(after.washes > washesBefore && after.cover > .5,
        `${engine.name()}: after a ${failure} the join was still cut, not washed: ${JSON.stringify({ washes: after.washes, washesBefore, cover: after.cover })}`);
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
    // The close carries no print across, so it must not wait for one. A reader who never stood still
    // in scene 4 has no print of it, since a phone captures only while the reader stands in a scene;
    // scene 4 must still dissolve as its copy crosses the visual, with the crossfade following and the
    // Publish control in the close's print. A fling from the top reaches the close before any print is
    // taken; a print missing at scene 4's rest (a capture that failed, or on desktop a plate dropped
    // outside the sheet, which discards every print) held the close back.
    // How long the art may stand solid after the scroll stops, about twice the slowest measured. A
    // fling waits for one tick of the warmer (about 1 s, below); on desktop the picture follows the page
    // at its own pace, about 1.4 s in WebKit and 2.3 to 3.7 s in a software-rendered Chromium.
    const LIMIT_MS = { 'fling from the top': { chromium: 2500, webkit: 2500 }, 'missing scene 4 print': { chromium: 500, webkit: 500 },
      'missing scene 4 print on desktop': { chromium: 7500, webkit: 3000 } };
    for (const close of ['fling from the top', 'missing scene 4 print', 'missing scene 4 print on desktop']) {
      const desktop = close.endsWith('desktop');
      const page = await browser.newPage(desktop ? { viewport: { width: 1440, height: 900 } } : { viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      if (desktop) {
        await page.evaluate(() => scrollTo(0, __restY(3)));
        await page.waitForFunction(() => __state().shown === 3 && !__state().running && __state().primed && Math.abs(__state().view - scrollY) <= 1, null, { timeout: 30000 });
      } else if (close === 'missing scene 4 print') {
        await cross(page, 1, -.08);
        await page.waitForFunction(() => __state().shown === 1 && !__state().running && __state().primed, null, { timeout: 20000 });
        await cross(page, 1); await cross(page, 2);
        await page.waitForFunction(() => __state().shown === 3 && !__state().running, null, { timeout: 20000 });
      }
      if (close !== 'fling from the top') await page.evaluate(async () => {
        // between captures, so none in flight can land after the print is taken away
        while (retaking) await new Promise((r) => requestAnimationFrame(r));
        sheets[3] = null; const real = capture; capture = async (scene) => (scene === 3 ? null : real(scene));
      });
      if (desktop) await page.evaluate(() => {
        // a position between two rests is held with the snap off, as the desktop pace check does
        document.head.appendChild(Object.assign(document.createElement('style'), { textContent: 'html { scroll-snap-type: none !important }' }));
        scrollTo(0, __restY(3) + .75 * (__restY(4) - __restY(3)));
      });
      else await cross(page, 3, .75, { fling: close === 'fling from the top', settle: 0 });
      // A fling also outruns scene 3's print. Where the join into scene 3 lands before the fling ends
      // (WebKit, measured), the next join waits in maybeJoin for that print, which the warmer takes only
      // once the reader has stopped; where it is still running (Chromium, whose frames come slower), its
      // own loop goes on and cuts through to the close.
      const stopped = Date.now(), limit = LIMIT_MS[close][engine.name()];
      const covered = await page.waitForFunction(() => stage.classList.contains('mobile-handoff') && stage.style.getPropertyValue('--wash-cover') === '1'
        && Math.abs(__state().view - scrollY) <= 1, null, { timeout: 15000 }).then(() => true, () => false);
      const latency = Date.now() - stopped;
      const state = await page.evaluate(async () => {
        // two frames with the picture still, so the crossfade has had a frame at its final position
        for (let i = 0; i < 2; i++) await new Promise((r) => requestAnimationFrame(r));
        const cover = stage.classList.contains('mobile-handoff') ? +stage.style.getPropertyValue('--wash-cover') || 0 : 0;
        // The control in the close's print: its disc in the live button's colour, and its icon's ink. This
        // shows drawControl drew; that the drawing matches the live control rests on a frame comparison.
        // The button is looked up here rather than through the page's publishButton(), so the check also
        // runs against a page from before that helper.
        const button = vdFrame.contentDocument.querySelector('.moss-publish-button'), style = getComputedStyle(button), rgb = (c) => { const g = document.createElement('canvas').getContext('2d'); g.fillStyle = c; g.fillRect(0, 0, 1, 1); return [...g.getImageData(0, 0, 1, 1).data]; };
        const disc = rgb(style.backgroundColor), ink = rgb(style.color), src = finalPrints?.source, r = Math.round(button.getBoundingClientRect().width / 2 * PUB_SCALE);
        const near = (a, b, d) => [0, 1, 2].every((i) => Math.abs(a[i] - b[i]) <= d);
        let control = null;
        if (src) {
          const data = src.getContext('2d').getImageData(GEOM.cellW / 2 - printRect.x - r, GEOM.cellH / 2 - printRect.y - r, 2 * r, 2 * r).data, at = (x, y) => [...data.slice((y * 2 * r + x) * 4, (y * 2 * r + x) * 4 + 4)];
          let inked = 0;
          for (let y = 0; y < 2 * r; y++) for (let x = 0; x < 2 * r; x++) if (Math.hypot(x - r, y - r) < r / 2 && near(at(x, y), ink, 60)) inked++;
          const edge = at(Math.round(r * .4), Math.round(r * 1.6));
          control = { disc: near(edge, disc, 12) && edge[3] > 200, inked };
        }
        return { q: +closingProgress().toFixed(3), cover, xf: +xf.toFixed(3), xfAt: +xfAt().toFixed(3), scene4Print: !!sheets[3], control };
      });
      assert(!state.scene4Print, `${engine.name()}: inconclusive, scene 4's print was taken before the close: ${JSON.stringify(state)}`);
      assert(covered, `${engine.name()}: after a ${close} scene 4 stood solid while its copy crossed the visual, 15 s after the scroll stopped: ${JSON.stringify(state)}`);
      assert(latency <= limit, `${engine.name()}: after a ${close} scene 4 stood solid for ${latency}ms after the scroll stopped, more than ${limit}ms: ${JSON.stringify(state)}`);
      assert(state.q > .7 && state.q < .8, `${engine.name()}: ${close} did not stop three quarters into the close: ${JSON.stringify(state)}`);
      assert(Math.abs(state.xf - state.xfAt) < .02, `${engine.name()}: after a ${close} the crossfade did not follow the copy: ${JSON.stringify(state)}`);
      assert(state.control?.disc && state.control.inked > 20, `${engine.name()}: after a ${close} the close's print has no Publish control: ${JSON.stringify(state)}`);
      console.log(`${engine.name()}: ${close} still dissolves scene 4 as it leaves, the crossfade following (covered ${latency}ms after the scroll stopped)`);
      await page.close();
    }
  } finally { await browser.close(); }
}
