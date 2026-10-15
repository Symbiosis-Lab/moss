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
// (1.05: past it; below 0: short of it), in 40 even steps or as a fling of `ms` that slows to a
// stop (0: in one frame), then settle for `settle` ms.
async function cross(page, scene, fraction = 1.05, { fling = false, settle = 1500, ms = 800 } = {}) {
  await page.evaluate(async ({ scene, fraction, fling, ms }) => {
    const text = scenesEl[scene].firstElementChild.getBoundingClientRect(), band = mobileVisualBand();
    const to = scrollY + text.top - band.bottom + fraction * (band.height + text.height), from = scrollY, t0 = performance.now();
    let cover = 0, startedAt = null;
    for (let k = 1, u = 0; u < 1; k++) {
      u = fling ? (ms ? Math.min(1, (performance.now() - t0) / ms) : 1) : k / 40;
      scrollTo(0, from + (to - from) * (fling ? 1 - (1 - u) ** 3 : u)); await new Promise((r) => requestAnimationFrame(r));
      cover = Math.max(cover, +getComputedStyle(document.getElementById('stage')).getPropertyValue('--wash-cover') || 0);
      // how far into this section's passage the join was first seen running
      if (startedAt === null && __state().running) startedAt = __state().progress - scene;
    }
    window.__crossCover = cover; window.__crossStarted = startedAt;
  }, { scene, fraction, fling, ms });
  await page.waitForTimeout(settle);
  return page.evaluate(() => ({ ...__state(), cover: window.__crossCover, startedAt: window.__crossStarted }));
}

// Record every frame from now on: the stage's scene, the scene shown and the target, the cell's
// opacity, a wash's cover (the canvas held over the stage while the warmer switches it to take a
// print of a neighbour counts as full cover) and the most opaque print veil. And the close's: its
// crossfade, how far the close has drawn itself (its dissolve's progress, `close`), how far the
// crossfade runs ahead of the one that progress sets (`lead`; .45 to 1 is the crossfade's span in the
// close, crossAt), and how much of scene 4's art its dissolve still shows (`closeArt`). The dissolve plays a recording made as it goes,
// and a side still being recorded shows its latest frame, so it shows the depth asked of it or the
// last frame recorded, whichever is shallower. takeFrames stops it.
const recordFrames = (page) => page.evaluate(() => {
  window.__frames = [];
  const P = MossWatercolor.DEFAULT_PRESET, lastFrame = Math.ceil(Math.round(P.recDuration / P.dt) / P.recEvery);
  const tick = () => {
    if (!window.__frames) return;
    const cover = stage.classList.contains('morphing') ? 1 : stage.classList.contains('mobile-handoff') ? +stage.style.getPropertyValue('--wash-cover') || 0 : 0;
    const veil = Math.max(0, ...[...cell.querySelectorAll(':scope > canvas:not(#gl)')].map((v) => +getComputedStyle(v).opacity));
    const close = finalWash ? finalWash.goal / T_TOTAL : 0, recorded = finalPrints?.rec ? (finalPrints.rec.sides[0].frames.length - 1) / lastFrame : 0;
    const closeArt = finalWash ? 1 - cover * Math.max(0, Math.min(clamp01((close - CLOSE_HOLD) / (1 - CLOSE_HOLD)) / P.playOut, recorded, 1)) : 1;
    __frames.push({ t: performance.now(), scene: +stage.dataset.scene, shown, target, cell: +getComputedStyle(cell).opacity, cover, veil, xf, close, lead: xf - smooth(.45, 1, close), closeArt });
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});
const takeFrames = (page) => page.evaluate(() => {
  const frames = __frames; window.__frames = null;
  return { frames, end: { ...__state(), at: progressAt(), q: closingProgress(), handoff: stage.classList.contains('mobile-handoff'), cell: getComputedStyle(cell).opacity,
    fading: cell.getAnimations().length, veils: cell.querySelectorAll(':scope > canvas:not(#gl)').length } };
});
// How many frames a reader may wait between `from` and `to` (page times) for a fling to settle that
// crossed `washes` washes and drawings paced by `dissolves` dissolves (a live fade takes two, the
// close one): four frames a wash (a phone wash crosses in no fewer), the dissolves and a rest's worth
// of stillness, in this page's own median frame interval, and some slack. Counted in frames, not milliseconds, because a software-rendered Chromium draws as
// few frames a second as it can, and stalls for up to one, while every crossing is paced in frames
// or in dissolves within a frame's allowance.
const SLACK_FRAMES = 12;
async function frameBudget(page, frames, from, to, washes, dissolves = 2) {
  const { dissolve, rest } = await page.evaluate(() => ({ dissolve: DISSOLVE.duration, rest: IDLE_REST }));
  const gaps = frames.slice(1).map((f, i) => f.t - frames[i].t).sort((a, b) => a - b), median = gaps[gaps.length >> 1] || 1000 / 60;
  const allowed = 4 * washes + Math.ceil(dissolves * dissolve / median) + Math.ceil(rest / median) + SLACK_FRAMES;
  return { used: frames.filter((f) => f.t > from && f.t <= to).length, allowed, median: +median.toFixed(1) };
}
// Rest for `ms` from `from` (a page time), and longer where frames come slow: until as many frames have
// been drawn since `from` as frameBudget allows a fling that crossed `washes` washes, and two more, so a
// page that settles within its budget is seen to settle and to stay. A fixed rest held only four or five
// frames of a software-rendered Chromium on a loaded machine, at 0.65 to 0.8 s a frame, against a budget
// of 22, and failed the page while it was still crossing well inside it. Returns whether the budget was
// reached before `guard` ran out, so a row that waited less than it meant to says so rather than
// reading a page that never had its frames.
async function holdRest(page, from, ms, washes, guard = 30000) {
  await page.waitForTimeout(ms);
  for (const until = Date.now() + guard; Date.now() < until; await page.waitForTimeout(250)) {
    const { used, allowed } = await frameBudget(page, await page.evaluate(() => __frames.map(({ t }) => ({ t }))), from, Infinity, washes);
    if (used >= allowed + 2) return true;
  }
  return false;
}
// How much of the stage's own scene is on screen, and how much stands in for a scene it has left.
const live = (f) => f.cell * (1 - f.cover), standIn = (f) => f.cell * Math.max(f.veil, f.cover);
const leftFaded = (end) => end.cell !== '1' || end.fading || end.veils;

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
    // fling first washes into scenes 2 and 3 and fades scene 3's live art (below), so it is bounded in
    // frames (frameBudget), with 15 s only as a guard against a hang; on desktop the picture follows
    // the page at its own pace, about 1.4 s in WebKit and 2.3 to 3.7 s in a software-rendered Chromium.
    const LIMIT_MS = { 'missing scene 4 print': { chromium: 500, webkit: 500 }, 'missing scene 4 print on desktop': { chromium: 7500, webkit: 3000 } };
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
      else {
        if (close === 'fling from the top') await recordFrames(page);
        await cross(page, 3, .75, { fling: close === 'fling from the top', settle: 0 });
      }
      // A fling also outruns scene 3's print, so the join out of scene 3 fades its live art, and the
      // close follows it.
      const stopped = Date.now(), stoppedAt = await page.evaluate(() => performance.now());
      const covered = await page.waitForFunction(() => stage.classList.contains('mobile-handoff') && stage.style.getPropertyValue('--wash-cover') === '1'
        && Math.abs(__state().view - scrollY) <= 1, null, { timeout: 15000 }).then(() => true, () => false);
      const latency = Date.now() - stopped, coveredAt = await page.evaluate(() => performance.now());
      const budget = close === 'fling from the top' ? await frameBudget(page, (await takeFrames(page)).frames, stoppedAt, coveredAt, 2) : null;
      // the close draws itself toward the reader at its own pace, which the rows below bound
      const followed = await page.waitForFunction(() => Math.abs(xf - xfAt()) < .02, null, { timeout: 5000 }).then(() => true, () => false);
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
      if (budget) assert(budget.used <= budget.allowed, `${engine.name()}: after a ${close} scene 4 stood solid for ${budget.used} frames (${latency}ms) after the scroll stopped, more than ${budget.allowed} at ${budget.median}ms a frame: ${JSON.stringify(state)}`);
      else assert(latency <= LIMIT_MS[close][engine.name()], `${engine.name()}: after a ${close} scene 4 stood solid for ${latency}ms after the scroll stopped, more than ${LIMIT_MS[close][engine.name()]}ms: ${JSON.stringify(state)}`);
      assert(state.q > .7 && state.q < .8, `${engine.name()}: ${close} did not stop three quarters into the close: ${JSON.stringify(state)}`);
      assert(followed, `${engine.name()}: after a ${close} the crossfade had not caught up with the copy 5 s later: ${JSON.stringify(state)}`);
      assert(Math.abs(state.xf - state.xfAt) < .02, `${engine.name()}: after a ${close} the crossfade did not follow the copy: ${JSON.stringify(state)}`);
      assert(state.control?.disc && state.control.inked > 20, `${engine.name()}: after a ${close} the close's print has no Publish control: ${JSON.stringify(state)}`);
      console.log(`${engine.name()}: ${close} still dissolves scene 4 as it leaves, the crossfade following (covered ${budget ? `${budget.used} frames of ${budget.allowed}, ` : ''}${latency}ms after the scroll stopped)`);
      await page.close();
    }
    // A close the reader is already deep in when it starts must still dissolve scene 4, not show that
    // depth on its first frame: read straight off the position, the art went from whole to mostly gone
    // between two frames. A phone reaches it so after a fling from the top, whose crossing into scene 4
    // lands with the reader three quarters into the close, and after a fling from scene 4's rest in a
    // single frame. (A slower fling from the rest is followed by the close frame by frame, so it showed
    // no jump.) What is on screen of scene 4's art is the visual's opacity (the crossfade) times what the
    // dissolve still shows of it (closeArt). It must never go from about whole to mostly gone between
    // two frames; the close's progress must take no fewer frames to reach the reader than the four a
    // crossing it paces (a quarter of the close a frame); the crossfade must never run ahead of the
    // close's own progress, which drives the dissolve with it, so the film never comes up over art that
    // has not begun to dissolve; and the close must stand where the reader stopped within a frame
    // budget (frameBudget), with nothing left faded.
    const WHOLE = .9, GONE = .6;
    for (const [fling, ms] of [['from the top', 800], ['from scene 4\'s rest in one frame', 0]]) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      const top = fling === 'from the top';
      if (!top) {
        await cross(page, 2);
        await page.waitForFunction(() => __state().shown === 3 && __state().target === 3 && !__state().running, null, { timeout: 20000 });
      }
      await recordFrames(page);
      await cross(page, 3, .75, { fling: true, ms, settle: 0 });
      const stopped = await page.evaluate(() => performance.now());
      // standing where the reader stopped: the crossfade at the copy, the dissolve at the reader's depth
      const arrived = await page.waitForFunction(() => finalWash && Math.abs(xf - xfAt()) < .02 && Math.abs(finalWash.goal / T_TOTAL - closingProgress()) < .02,
        null, { timeout: 15000 }).then(() => true, () => false);
      const arrivedAt = await page.evaluate(() => performance.now());
      await page.waitForTimeout(300);
      const { frames, end } = await takeFrames(page);
      const art = (f) => (1 - f.xf) * f.closeArt, at = (f) => ({ at: Math.round(f.t - stopped), xf: +f.xf.toFixed(3), close: +f.close.toFixed(3), closeArt: +f.closeArt.toFixed(3) });
      const pops = frames.slice(1).flatMap((f, i) => (art(frames[i]) >= WHOLE && art(f) <= GONE ? [{ from: at(frames[i]), to: at(f) }] : []));
      const ahead = frames.filter((f) => f.lead > .02).map(at);
      const first = frames.findIndex((f) => f.close > 0), there = frames.findIndex((f, i) => i >= first && Math.abs(f.close - end.q) < .02);
      const floor = Math.ceil(4 * end.q), spread = first < 0 || there < 0 ? 0 : there - first + 1;
      const budget = await frameBudget(page, frames, stopped, arrivedAt, top ? 2 : 0, top ? 3 : 1);
      const where = `a fling into the close ${fling}`;
      assert(arrived, `${engine.name()}: after ${where} the close did not reach the reader in 15 s: ${JSON.stringify(end)}`);
      assert(!pops.length, `${engine.name()}: after ${where} scene 4's art went from whole to mostly gone between two frames: ${JSON.stringify(pops)}`);
      assert(!ahead.length, `${engine.name()}: after ${where} the crossfade ran ahead of the close's dissolve: ${JSON.stringify(ahead.slice(0, 3))}`);
      assert(spread >= floor, `${engine.name()}: after ${where} the close reached the reader's depth in ${spread} frames, fewer than ${floor}`);
      assert(budget.used <= budget.allowed, `${engine.name()}: after ${where} the close reached the reader ${budget.used} frames (${Math.round(arrivedAt - stopped)}ms) after the scroll stopped, more than ${budget.allowed} at ${budget.median}ms a frame`);
      assert(!leftFaded(end), `${engine.name()}: after ${where} the art was left faded: ${JSON.stringify(end)}`);
      console.log(`${engine.name()}: ${where} dissolves scene 4 and reaches the reader's depth over ${spread} frames, ${budget.used} frames of ${budget.allowed}, ${Math.round(arrivedAt - stopped)}ms, after the scroll stopped`);
      await page.close();
    }
    // A reader who flings past the close's end gets there through the close, not over it: standing on
    // scene 5 the moment the position passed the end put the film up over art the dissolve had not
    // reached. The close plays to its end first, and scene 5 stands once it has drawn itself there.
    // The second fling leaves a wash in flight the other way. A wash cannot cross the close's own
    // boundary, so it plays itself out and the close runs after it; pointed across it, the wash walked
    // the stage to scene 5 with the close never drawn and left the film down over scene 5's copy.
    for (const from of ['from scene 4\'s rest', 'while a wash turns back toward scene 3']) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      // The turning wash must be a wash: a reader who walked up through the scenes has the prints of
      // scenes 3 and 4, where one who flung to scene 4 has neither and the join back fades the live art.
      if (from === 'from scene 4\'s rest') await cross(page, 2);
      else { await cross(page, 1, -.08); await cross(page, 1); await cross(page, 2); }
      await page.waitForFunction(() => __state().shown === 3 && __state().target === 3 && !__state().running, null, { timeout: 20000 });
      const prints = from === 'from scene 4\'s rest' || await page.waitForFunction(() => !!sheets[2] && !!sheets[3], null, { timeout: 20000 }).then(() => true, () => false);
      await recordFrames(page);
      // a frame of scene 4 whole on the record, so the jump has a frame to jump from
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
      let turning = true;
      if (from === 'from scene 4\'s rest') await cross(page, 3, 1.05, { fling: true, ms: 0, settle: 0 });
      // the turn and the fling in one go, so the wash out of scene 4 is still running at the fling
      else turning = await page.evaluate(async () => {
        const where = (scene, fraction) => { const t = scenesEl[scene].firstElementChild.getBoundingClientRect(), b = mobileVisualBand(); return scrollY + t.top - b.bottom + fraction * (b.height + t.height); };
        const away = where(2, -.08), past = where(3, 1.05);
        scrollTo(0, away);
        for (let i = 0; i < 2; i++) await new Promise((r) => requestAnimationFrame(r));
        const washing = running() && target < 3;
        scrollTo(0, past);
        return washing;
      });
      const stopped = await page.evaluate(() => performance.now());
      // a hang guard, not a pace claim: the pops and the four-frame floor below are what bound the
      // drawing. A software-rendered Chromium under load draws about a frame a second, so the turning
      // wash and the close together can take ten seconds of frames.
      const stood = await page.waitForFunction(() => __state().shown === 4 && !__state().running, null, { timeout: 30000 }).then(() => true, () => false);
      await page.waitForTimeout(300);
      const { frames, end } = await takeFrames(page);
      const seen = frames.filter((f) => f.t >= stopped), art = (f) => (1 - f.xf) * f.closeArt;
      const at = (f) => ({ at: Math.round(f.t - stopped), xf: +f.xf.toFixed(3), close: +f.close.toFixed(3), closeArt: +f.closeArt.toFixed(3) });
      // every frame, not only those after the stop: the jump was between the frame that showed scene 4
      // whole and the first frame after the fling
      const pops = frames.slice(1).flatMap((f, i) => (art(frames[i]) >= WHOLE && art(f) <= GONE ? [{ from: at(frames[i]), to: at(f) }] : []));
      // the dissolve's own progress, which starts before the crossfade (crossAt holds it at 0 for the
      // close's first 45%), up to the frame the crossfade is fully up
      const first = seen.findIndex((f) => f.close > 0), there = seen.findIndex((f, i) => i >= first && f.xf >= 1);
      const spread = first < 0 || there < 0 ? 0 : there - first + 1;
      const where = `a fling past the close's end ${from}`;
      assert(prints && turning, `${engine.name()}: inconclusive, no wash was running out of scene 4 when the reader flung past the close's end: ${JSON.stringify({ prints, turning })}`);
      assert(stood, `${engine.name()}: ${where} did not come to stand on scene 5 in 15 s: ${JSON.stringify(end)}`);
      assert(!pops.length, `${engine.name()}: ${where} took scene 4's art from whole to mostly gone between two frames: ${JSON.stringify(pops)}`);
      assert(spread >= 4, `${engine.name()}: ${where} ran the close in ${spread} frames, fewer than the four a crossing it paces: ${JSON.stringify(seen.slice(Math.max(0, first - 1), there + 1).map(at))}`);
      assert(end.shown === 4 && end.target === 4 && !end.running && end.xf === 1, `${engine.name()}: ${where} does not stand on scene 5: ${JSON.stringify(end)}`);
      assert(!leftFaded(end), `${engine.name()}: ${where} left the art faded: ${JSON.stringify(end)}`);
      console.log(`${engine.name()}: ${where} plays the close over ${spread} frames, then stands on scene 5`);
      await page.close();
    }
    // A close that loses the GPU mid-way holds its cover over scene 4, and the canvas under it no longer
    // draws. Coming back to scene 4 must take that cover down, or the reader rests before nothing.
    {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      await cross(page, 3, .5, { settle: 0 });
      const covered = await page.waitForFunction(() => finalWash && +stage.style.getPropertyValue('--wash-cover') > .9, null, { timeout: 15000 }).then(() => true, () => false);
      await page.evaluate(() => document.getElementById('gl').getContext('webgl2').getExtension('WEBGL_lose_context').loseContext());
      const back = await cross(page, 3, -.08, { settle: 2500 });
      const state = await page.evaluate(() => ({ lost: simLost, wash: !!finalWash, handoff: stage.classList.contains('mobile-handoff'),
        cover: +stage.style.getPropertyValue('--wash-cover') || 0, cell: getComputedStyle(cell).opacity, xf: +xf.toFixed(3) }));
      assert(covered && state.lost, `${engine.name()}: inconclusive, the close was not covering scene 4 with a lost context: ${JSON.stringify(state)}`);
      assert(!back.running && back.shown === 3 && back.target === 3, `${engine.name()}: a close that lost the GPU left the art behind the scroll: ${JSON.stringify({ shown: back.shown, target: back.target, running: back.running })}`);
      assert(!state.wash && !state.handoff && !state.cover && state.cell === '1' && !state.xf,
        `${engine.name()}: a close that lost the GPU left scene 4 covered by a canvas that no longer draws: ${JSON.stringify(state)}`);
      console.log(`${engine.name()}: a close that loses the GPU lets scene 4 stand again when the reader comes back`);
      await page.close();
    }
    // A close starts from where the crossfade stands. Only a cold load leaves it partway when a close
    // begins, since before boot the crossfade follows the page directly: a reader reaches the bottom
    // while the demos are still loading and comes back three quarters into the close. Drawn from its own
    // end instead, the close back toward scene 4 raised the film over the page and lowered it again. The
    // reader stands still, so from boot on the crossfade must not rise.
    {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.route(/\/ui\/(editor|shell)\.html/, async (route) => { await new Promise((r) => setTimeout(r, 3000)); await route.continue(); });
      await page.goto(withDebug(url), { waitUntil: 'commit' });
      await page.waitForFunction(() => typeof window.__state === 'function', null, { timeout: 20000 });
      await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
      await page.waitForFunction(() => __state().shown === 4 && __state().xf === 1, null, { timeout: 5000 });
      await page.evaluate(() => { const text = scenesEl[3].firstElementChild.getBoundingClientRect(), band = mobileVisualBand(); scrollTo(0, scrollY + text.top - band.bottom + .75 * (band.height + text.height)); });
      await page.waitForFunction(() => Math.abs(xf - xfAt()) < .01, null, { timeout: 5000 });
      const seen = await page.evaluate(async () => {
        const frame = () => new Promise((r) => requestAnimationFrame(r)), parked = { xf, cold: document.documentElement.dataset.ready !== '1' };
        let peak = xf;
        for (const until = performance.now() + 30000; document.documentElement.dataset.ready !== '1' && performance.now() < until;) { await frame(); peak = Math.max(peak, xf); }
        for (const until = performance.now() + 1500; performance.now() < until;) { await frame(); peak = Math.max(peak, xf); }
        return { ...parked, booted: document.documentElement.dataset.ready === '1', peak: +peak.toFixed(3), now: +xf.toFixed(3), shown };
      });
      assert(seen.cold && seen.booted && seen.xf > .3 && seen.xf < .9, `${engine.name()}: inconclusive, the page was not parked partway into the close before boot: ${JSON.stringify(seen)}`);
      assert(seen.peak <= seen.xf + .02, `${engine.name()}: a close begun at boot with the reader three quarters in raised the crossfade from ${seen.xf.toFixed(3)} to ${seen.peak}: ${JSON.stringify(seen)}`);
      console.log(`${engine.name()}: a close begun at boot starts where the crossfade stood (${seen.xf.toFixed(3)}, at most ${seen.peak} after)`);
      await page.close();
    }
    // A frame of the close that throws must not hold the join open for the rest of the visit: the art
    // must follow the reader back to scene 3 afterwards, and the next close must draw.
    {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      await page.evaluate(() => {
        const real = updateFinalDissolve, closing = fade;
        let armed = true, inClose = false;
        fade = (to) => { inClose = true; return closing(to).finally(() => { inClose = false; }); };
        // the close's own progress is forwarded, or every later frame reads as the close's end and releases it
        updateFinalDissolve = (...at) => { if (armed && inClose) { armed = false; window.__closeThrew = true; throw new Error('injected close failure'); } return real(...at); };
      });
      await cross(page, 3, .75, { settle: 2000 });
      const after = await cross(page, 2, -.08, { settle: 3000 });
      assert(await page.evaluate(() => window.__closeThrew), `${engine.name()}: the injected close failure never fired`);
      assert(!after.running && after.shown === 2 && after.target === 2, `${engine.name()}: a close frame that threw left the art behind the scroll: ${JSON.stringify({ shown: after.shown, target: after.target, running: after.running })}`);
      await cross(page, 3, .75, { settle: 0 });
      const drew = await page.waitForFunction(() => finalWash && stage.style.getPropertyValue('--wash-cover') === '1' && Math.abs(xf - xfAt()) < .02, null, { timeout: 15000 }).then(() => true, () => false);
      assert(drew, `${engine.name()}: after a close frame threw, the next close did not dissolve scene 4: ${JSON.stringify(await page.evaluate(() => ({ ...__state(), wash: !!finalWash, cover: stage.style.getPropertyValue('--wash-cover'), q: +closingProgress().toFixed(3) })))}`);
      console.log(`${engine.name()}: a throwing close frame ends the close; the art follows the reader back and the next close dissolves`);
      await page.close();
    }
    // On desktop the Publish bridge between scenes 3 and 4 is a live wash. Turned back to the scene it
    // began on, with the picture coming to rest exactly there, it must finish there: pointed at its own
    // start it once clocked itself by a crossing of no length and held the join, its copy of the Publish
    // control up and the real one hidden, until the reader moved again.
    for (const [from, to] of [[2, 3], [3, 2]]) {
      const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
      await page.evaluate((from) => scrollTo(0, __restY(from)), from);
      await page.waitForFunction((from) => __state().shown === from && !__state().running && __state().primed && Math.abs(__state().view - scrollY) <= 1, from, { timeout: 30000 });
      const turned = await page.evaluate(async ({ from, to }) => {
        document.head.appendChild(Object.assign(document.createElement('style'), { textContent: 'html { scroll-snap-type: none !important }' }));
        scrollTo(0, __restY(from) + .6 * (__restY(to) - __restY(from)));
        await new Promise((r) => setTimeout(r, 300));
        const washing = running();
        scrollTo(0, __restY(from));
        return washing;
      }, { from, to });
      const done = await page.waitForFunction((from) => !running() && shown === from && !document.getElementById('publish-bridge'), from, { timeout: 10000 }).then(() => true, () => false);
      const state = await page.evaluate(() => ({ ...__state(), bridge: !!document.getElementById('publish-bridge'), rest: Math.abs(__state().view - scrollY) <= 1 }));
      assert(turned, `${engine.name()}: no wash was running when the reader turned back toward scene ${from + 1}: ${JSON.stringify(state)}`);
      assert(done, `${engine.name()}: the Publish bridge from scene ${from + 1} toward scene ${to + 1}, turned back to rest on scene ${from + 1}, still held the join 10 s later: ${JSON.stringify(state)}`);
      console.log(`${engine.name()}: desktop: the Publish bridge from scene ${from + 1}, turned back, finishes on scene ${from + 1}`);
      await page.close();
    }
    // A reader who flings through scenes 3 and 4 never stands in either, so a phone has no print of
    // them. The art of the scene left must still go as the reader leaves it, without waiting for a
    // print the reader would have to stop for, and without a frame where it stands whole and then is
    // gone. Each frame reads how much of the leaving scene's art is on screen: the stage while it shows
    // that scene, then whatever stands in for it (standIn). Back from the close, the turn comes once scene 4 has stood; back mid-fade, it
    // comes as soon as the fling into scene 4 ends.
    // How long the leaving art may stand whole (at least 99% of it on screen) after the target has moved
    // past it: two frames (the one that shows it before the join's first, and one more), or 100 ms when
    // frames come bunched, whichever is longer. Both, because either alone is unsteady: in Chromium the
    // frame after the one that first shows scene 3 can come most of a second late, and three frames can
    // also fall within 4 ms. A join that waits for the print stands it whole for 72 to 87 frames, 1.25
    // to 1.5 s.
    const WHOLE_FRAMES = 2, WHOLE_MS = 100;
    for (const fling of ['into scene 4', 'back from the close', 'back mid-fade']) {
      const back = fling !== 'into scene 4', leaving = back ? 3 : 2;
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      await recordFrames(page);
      await cross(page, 3, .75, { fling: true, settle: fling === 'back from the close' ? 2500 : 0 });
      const mark = await page.evaluate(() => performance.now());
      if (back) await cross(page, 2, -.08, { fling: true, settle: 0 });
      const held = await holdRest(page, mark, 2500, 2);
      const { frames, end } = await takeFrames(page);
      const seen = frames.filter((f) => f.t >= (back ? mark : 0)), moved = seen.findIndex((f) => (back ? f.target < 3 : f.target > 2));
      // Back out of the close, the close plays itself back to scene 4 before the join toward scene 3
      // starts. That playback is paced like any crossing, so it costs its own dissolve and no more.
      const closed = fling === 'back from the close' ? seen.findIndex((f, i) => i >= moved && !f.close) : -1;
      const playback = closed < 0 ? null : await frameBudget(page, frames, seen[moved].t, seen[closed].t, 0, 1);
      const whole = moved < 0 ? [] : seen.slice(moved).filter((f) => f.scene === leaving && live(f) >= .99);
      // A pop: the stage shows its scene whole, and on the next frame it shows another scene with nothing
      // standing in for the one it showed, or the same scene gone in place, at any boundary the fling crosses.
      const pops = seen.slice(1).flatMap((f, i) => {
        const was = seen[i], after = was.scene !== f.scene ? standIn(f) : Math.max(live(f), standIn(f));
        return live(was) >= .9 && after <= .1 ? [{ at: Math.round(f.t - mark), scenes: `${was.scene}->${f.scene}`, from: +live(was).toFixed(2), to: +after.toFixed(2) }] : [];
      });
      const stood = whole.length ? Math.round(whole[whole.length - 1].t - seen[moved].t) : 0;
      assert(held, `${engine.name()}: ${fling}: the page drew fewer frames in 30 s than its own budget allows, so the rest proves nothing: ${JSON.stringify(end)}`);
      assert(moved >= 0, `${engine.name()}: ${fling}: the target never moved past scene ${leaving + 1}: ${JSON.stringify(end)}`);
      assert(whole.length <= WHOLE_FRAMES || stood <= WHOLE_MS, `${engine.name()}: ${fling}: scene ${leaving + 1}'s art stood whole for ${whole.length} frames (${stood}ms) after the target moved past it, more than ${WHOLE_FRAMES} frames and ${WHOLE_MS}ms`);
      assert(!pops.length, `${engine.name()}: ${fling}: the art went from whole to nothing between two frames: ${JSON.stringify(pops)}`);
      assert(!leftFaded(end), `${engine.name()}: ${fling}: the art was left faded: ${JSON.stringify(end)}`);
      if (back) assert(end.shown === 2 && end.target === 2 && !end.running && !end.handoff && end.xf === 0,
        `${engine.name()}: ${fling}: scene 3 does not stand at its rest: ${JSON.stringify(end)}`);
      if (fling === 'back from the close') {
        assert(closed > moved, `${engine.name()}: ${fling}: the close never played back to scene 4 after the turn: ${JSON.stringify(end)}`);
        assert(playback.used <= playback.allowed, `${engine.name()}: ${fling}: the close's playback held the join toward scene 3 for ${playback.used} frames (${Math.round(seen[closed].t - seen[moved].t)}ms), more than ${playback.allowed} at ${playback.median}ms a frame`);
      }
      console.log(`${engine.name()}: fling ${fling}: scene ${leaving + 1}'s art leaves with the reader, without a pop (whole for ${whole.length} frames, ${stood}ms, after the target moved past it${playback ? `; the close played back in ${playback.used} frames of ${playback.allowed}, ${Math.round(seen[closed].t - seen[moved].t)}ms` : ''})`);
      await page.close();
    }
    // A reader who stops partway through a crossing with no print must not be left before a scene
    // partly faded, or before bare paper. The fling stops a fifth, a half and four fifths of the way
    // through scene 3's copy, the passage that carries the crossing from scene 3 to scene 4, and rests
    // there (holdRest). Within a frame budget (frameBudget) what the stage shows must stop changing, and
    // what it is left showing must be a whole scene or a wash's cover: the scene whose half of the crossing the
    // reader stopped in (scene 3 at a fifth, scene 4 at four fifths; at the half, the stop lands a hair
    // short of it and the scene is 3). And it must never return to a scene it showed and left during the
    // rest: a scene that landed mid-crossing would be pointed straight back at the one it left, and the
    // join back pointed back in turn.
    // A rest counts after IDLE_REST of stillness, and settling then takes up to two dissolves, one scene
    // out and the other in; in a software-rendered Chromium the washes into scenes 2 and 3 can still be
    // running at the stop. The rest is 3 s, or the frame budget and two frames where frames come slow
    // (holdRest), and 30 s more at most guards against a hang.
    for (const stop of [.2, .5, .8]) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      await page.goto(withDebug(url));
      await page.waitForFunction(() => window.__state?.().ready && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
      await recordFrames(page);
      await cross(page, 2, stop, { fling: true, settle: 0 });
      const stopped = await page.evaluate(() => performance.now());
      const held = await holdRest(page, stopped, 3000, 2);
      const { frames, end } = await takeFrames(page);
      const rest = frames.filter((f) => f.t >= stopped), last = rest[rest.length - 1];
      const look = (f) => [f.scene, f.shown, f.cell.toFixed(2), f.cover.toFixed(2)].join();
      const changed = rest.findLastIndex((f) => look(f) !== look(last)), settledIn = changed < 0 ? 0 : Math.round(rest[changed + 1].t - stopped);
      const budget = await frameBudget(page, frames, stopped, changed < 0 ? stopped : rest[changed + 1].t, 2);
      const scenesShown = rest.map((f) => f.scene).filter((scene, i, all) => scene !== all[i - 1]);
      const expected = end.at - 2 >= .5 ? 3 : 2;
      const at = (f) => f && { at: Math.round(f.t - stopped), scene: f.scene, shown: f.shown, target: f.target, cell: +f.cell.toFixed(3), cover: f.cover };
      const where = `stopped ${stop} of the way through scene 3's copy (progress ${end.progress})`;
      assert(held, `${engine.name()}: ${where}, the page drew fewer frames in 30 s than its own budget allows, so the rest proves nothing: ${JSON.stringify(end)}`);
      assert(live(last) >= .99 || last.cover >= .99, `${engine.name()}: ${where}, the art was left between scenes: ${JSON.stringify(at(last))}`);
      assert(changed < rest.length - 2 && budget.used <= budget.allowed, `${engine.name()}: ${where}, the art was still changing ${budget.used} frames (${settledIn}ms) after the stop, more than ${budget.allowed} at ${budget.median}ms a frame: ${JSON.stringify(at(rest[changed]))}`);
      assert(new Set(scenesShown).size === scenesShown.length, `${engine.name()}: ${where}, the stage went back to a scene it had left during the rest: ${JSON.stringify(scenesShown)}`);
      assert(last.scene === expected && (stop !== .2 || expected === 2) && (stop !== .8 || expected === 3), `${engine.name()}: ${where}, the art settled on scene ${last.scene + 1}, not scene ${expected + 1}`);
      assert(!leftFaded(end), `${engine.name()}: ${where}, the art was left faded: ${JSON.stringify(end)}`);
      console.log(`${engine.name()}: ${where}, the art settles on scene ${last.scene + 1} in ${budget.used} frames of ${budget.allowed}, ${settledIn}ms, and stays there`);
      await page.close();
    }
  } finally { await browser.close(); }
}
