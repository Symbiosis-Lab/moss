#!/usr/bin/env node
// Desktop's title dissolves as ink and is gone by scene 1, and each join plays
// from the page's position between two rests: the next scene is whole exactly
// when its copy has arrived, the carry travels slowly enough to watch it, and
// the close darkens in place through its own recording without stopping
// halfway. The reader keeps the scrollbar, the keys and a flick's speed.
// Scene 2's Publish cue is one ring at a time, and a resting scene redraws
// nothing it does not show. WebKit only: headless Chromium renders the
// desktop simulation in software at about a frame a second.
import { pathToFileURL } from 'node:url';
const base = process.argv[2];
if (!base) throw new Error('Usage: node scripts/check-landing-desktop-pace.mjs <site-url>');
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { webkit } = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
const assert = (ok, message) => { if (!ok) throw new Error(message); };
// a regression that stalls the page must fail here, not hang the runner
setTimeout(() => { console.error('Error: check-landing-desktop-pace did not finish within 4 minutes'); process.exit(1); }, 240000).unref();
const browser = await webkit.launch();
try {
  // A resting scene has the screen to itself: its neighbours' copy stands just
  // off the screen, so no scene is read with the one before or after it.
  for (const viewport of [{ width: 1440, height: 900 }, { width: 1280, height: 720 }, { width: 1920, height: 1200 }]) {
    const alone = await browser.newPage({ viewport });
    await alone.goto(base);
    await alone.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
    const seen = await alone.evaluate(() => {
      const texts = [...document.querySelectorAll('.scene .scene-text')], out = [];
      for (let s = 0; s <= texts.length; s++) {
        scrollTo(0, __restY(s));
        const prev = texts[s - 1]?.getBoundingClientRect(), next = texts[s + 1]?.getBoundingClientRect();
        if (prev && prev.bottom > 0) out.push(`scene ${s + 1} shows scene ${s}'s copy ${Math.round(prev.bottom)}px down from the top`);
        if (next && next.top < innerHeight) out.push(`scene ${s + 1} shows scene ${s + 2}'s copy ${Math.round(innerHeight - next.top)}px up from the bottom`);
      }
      return out;
    });
    assert(!seen.length, `webkit ${viewport.width}×${viewport.height}: ${seen.join('; ')}`);
    await alone.close();
  }
  console.log('webkit 1280×720 to 1920×1200: every resting scene has the screen to itself');
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.addInitScript(() => {
    window.__pace = [];
    const tick = (now) => {
      try { if (window.__state) { const s = __state(); __pace.push({ t: now, y: scrollY, washT: s.washT, running: s.running, shown: s.shown, q: finalDissolve, xf: s.xf, stage: document.getElementById('stage').getBoundingClientRect().top }); } } catch (e) {}
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.goto(base);
  await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  // The title dissolves as ink in its own simulation and is flushed off the
  // sheet by scene 1's rest: its canvas's own pixels, so the stage and copy
  // around it cannot stand in for it.
  await page.waitForFunction(() => titleWash && titleWash.sim.recorded(titleWash.rec), null, { timeout: 15000 });
  const title = await page.evaluate(async () => {
    const rest = __restY(0);
    // a resize at load can retake the title; each look waits for the wash on hand to be recorded
    const look = async (u) => {
      for (const t0 = performance.now(); !(titleWash && titleWash.sim.recorded(titleWash.rec));) {
        if (performance.now() - t0 > 15000) throw new Error('the title\'s wash was never recorded');
        await new Promise((r) => setTimeout(r, 50));
      }
      const c = titleWash.canvas;
      scrollTo(0, Math.round(rest * u)); await new Promise((r) => setTimeout(r, 250));
      const g = document.createElement('canvas').getContext('2d'); g.canvas.width = c.width; g.canvas.height = c.height; g.drawImage(c, 0, 0);
      const d = g.getImageData(0, 0, c.width, c.height).data; let ink = 0;
      for (let i = 3; i < d.length; i += 4) ink = Math.max(ink, d[i]);
      return { shown: getComputedStyle(c).display !== 'none', ink, text: getComputedStyle(openingTitle).color };
    };
    const mid = await look(.4), end = await look(.97), at = await look(1);
    scrollTo(0, 0); await new Promise((r) => setTimeout(r, 250));
    return { mid, end, at, top: { shown: getComputedStyle(titleWash.canvas).display !== 'none', text: getComputedStyle(openingTitle).color } };
  });
  assert(title.mid.shown && title.mid.ink > 100 && title.mid.text === 'rgba(0, 0, 0, 0)', `the title is not dissolving as ink: ${JSON.stringify(title.mid)}`);
  assert(title.end.ink < 12, `the title's wash is not flushed off by scene 1's rest: ${JSON.stringify(title.end)}`);
  assert(!title.at.shown && title.at.text === 'rgba(0, 0, 0, 0)', `something of the title stands at scene 1's rest: ${JSON.stringify(title.at)}`);
  assert(!title.top.shown && title.top.text !== 'rgba(0, 0, 0, 0)', `the title did not come back whole at the top: ${JSON.stringify(title.top)}`);
  console.log('webkit 1440×900: the title dissolves as ink and is gone by scene 1, and gathers again at the top');
  await page.mouse.move(720, 450);
  // The rests are read once the page has arrived: a scene's live layout can
  // settle a pixel or two away from where it measured while the other stood.
  const gesture = async (from, to) => {
    const mark = await page.evaluate(() => performance.now());
    for (let i = 0; i < 3; i++) { await page.mouse.wheel(0, to > from ? 100 : -100); await page.waitForTimeout(40); }
    await page.waitForFunction((to) => __state().shown === to && !__state().running && Math.abs(scrollY - __restY(to)) <= 1, to, { timeout: 20000 });
    await page.waitForTimeout(300);
    return { rests: await page.evaluate(([a, b]) => [__restY(a), __restY(b)], [from, to]), frames: await page.evaluate((mark) => __pace.filter((r) => r.t >= mark), mark) };
  };
  // scene 1's rest, then one gesture into scene 2
  for (let i = 0; i < 3; i++) { await page.mouse.wheel(0, 40); await page.waitForTimeout(30); }
  await page.waitForFunction(() => Math.abs(scrollY - __restY(0)) <= 1 && !__state().running, null, { timeout: 15000 });
  await page.waitForFunction(() => [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
  const join = await gesture(0, 1);
  const T = await page.evaluate(() => T_TOTAL);
  const [r0, r1] = join.rests;
  const arrival = join.frames.find((r) => Math.abs(r.y - r1) <= 1);
  const travel = arrival.t - join.frames[0].t;
  assert(travel >= 1600, `scene 1 to 2 was carried in ${Math.round(travel)}ms, too fast to watch`);
  // the wash is drawn in the frame after the page moves, so one frame of lag is the position's own
  const at = (r) => Math.min(1, Math.max(0, (r.y - r0) / (r1 - r0)));
  const drift = join.frames.map((r, i) => r.running && i ? Math.min(Math.abs(r.washT / T - at(r)), Math.abs(r.washT / T - at(join.frames[i - 1]))) : null).filter((d) => d != null);
  assert(drift.length && Math.max(...drift) < .04, `the wash did not follow the page between the rests: worst drift ${Math.max(...drift).toFixed(3)}`);
  const early = join.frames.find((r) => r.washT >= T - .01 && r.y < r1 - 3);
  assert(!early, `scene 2 was whole ${early && Math.round(r1 - early.y)}px before its copy arrived`);
  console.log(`webkit 1440×900: scene 1 to 2 carried over ${Math.round(travel)}ms; the wash tracks the page and is whole on arrival`);
  // Standing in scene 2, one hairline ring at a time leaves the demo's Publish
  // button and crosses the page on the page's own canvas; the next leaves only
  // once the last has gone. Nothing else is redrawn for nothing meanwhile: the
  // demo's Publish ring grows in place rather than being rebuilt each frame,
  // the button carries no pulse of its own, and the page's per-frame opening
  // work writes nothing and draws no title while the title stands gone.
  const cue = await page.evaluate(async () => {
    const d = shFrame.contentDocument, btn = d.querySelector('.moss-publish-button'), canvas = document.getElementById('publish-cue');
    let rebuilt = 0, rootWrites = 0, titleDraws = 0, ink = 0;
    const watch = new shFrame.contentWindow.MutationObserver((ms) => { for (const m of ms) rebuilt += [...m.addedNodes].filter((n) => n.nodeName === 'svg').length; });
    watch.observe(btn, { childList: true });
    const root = document.documentElement.style, setProperty = root.setProperty, show = titleWash.sim.show;
    root.setProperty = function (name, ...rest) { if (name === '--s' || name === '--opening-lift') rootWrites++; return setProperty.call(this, name, ...rest); };
    titleWash.sim.show = function (...a) { titleDraws++; return show.apply(this, a); };
    const from = cueRings.length, t0 = performance.now();
    while (performance.now() - t0 < 7600) {
      if (!canvas.hidden) { const px = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data; for (let i = 3; i < px.length; i += 4) if (px[i] > ink) ink = px[i]; }
      await new Promise((r) => setTimeout(r, 250));
    }
    watch.disconnect(); delete root.setProperty; titleWash.sim.show = show;
    const f = shFrame.getBoundingClientRect(), b = btn.getBoundingClientRect(), k = f.width / shFrame.offsetWidth;
    const cx = f.left + (b.left + b.width / 2) * k, cy = f.top + (b.top + b.height / 2) * k;
    return { rings: cueRings.slice(from), ink, rebuilt, rootWrites, titleDraws, pulse: getComputedStyle(btn, '::after').animationName, cx, cy, hit: document.elementFromPoint(cx, cy)?.id, fixed: getComputedStyle(canvas).position, ms: CUE_MS };
  });
  const gaps = cue.rings.slice(1).map((g, i) => g.t - cue.rings[i].t);
  assert(cue.rings.length >= 2 && cue.fixed === 'fixed' && cue.ink > 20, `scene 2's Publish cue did not radiate across the page: ${JSON.stringify(cue)}`);
  assert(gaps.every((g) => g >= cue.ms), `a ring left before the last had gone: ${gaps.map(Math.round)}`);
  assert(cue.rings.every((g) => Math.hypot(g.x - cue.cx, g.y - cue.cy) < 2), 'the Publish cue does not radiate from the button');
  assert(cue.rebuilt <= 6, `the demo's Publish ring was rebuilt ${cue.rebuilt} times in 7.6s instead of growing in place`);
  assert(cue.pulse === 'none', `the Publish button carries a pulse of its own: ${cue.pulse}`);
  assert(cue.hit === 'sh', `scene 2's Publish button is covered by #${cue.hit}, so it cannot be clicked`);
  assert(cue.rootWrites === 0 && cue.titleDraws === 0, `scene 2 at rest rewrote the root ${cue.rootWrites} times and drew the gone title ${cue.titleDraws} times`);
  console.log('webkit 1440×900: scene 2\'s Publish cue sends one ring at a time from the button across the page, and nothing else is redrawn for nothing');
  // A window resized under a ring ends it, rather than leaving it stretched
  // over the new window for the rest of its travel; the next is measured anew.
  const since = await page.evaluate(() => performance.now());
  await page.waitForFunction((t) => cueRings.at(-1)?.t > t && !document.getElementById('publish-cue').hidden, since, { timeout: 10000 });
  await page.setViewportSize({ width: 1300, height: 900 });
  await page.waitForTimeout(200);
  const resized = await page.evaluate(() => { const c = document.getElementById('publish-cue'); return { hidden: c.hidden, width: c.width, want: Math.round(innerWidth * (devicePixelRatio || 1)) }; });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(500);
  assert(resized.hidden || resized.width === resized.want, `a ring kept its old window's canvas after a resize: ${JSON.stringify(resized)}`);
  // Standing in scene 3, the sketch and notebook keep running through the
  // retakes of its print. Each retake wakes them for its own frames, so they
  // are read between retakes, never during one.
  await page.evaluate(() => scrollTo(0, __restY(2)));
  await page.waitForFunction(() => __state().shown === 2 && !__state().running, null, { timeout: 30000 });
  const retakes = await page.evaluate(() => captures);
  await page.waitForFunction((n) => captures >= n + 2 && !inFlight, retakes, { timeout: 20000 });
  const live = await page.evaluate(() => document.getElementById('nb').contentWindow.__notebook.state().visible);
  assert(live, 'scene 3\'s notebook was left asleep by a retake of the scene it stands in');
  console.log('webkit 1440×900: scene 3\'s artifacts keep running through the retakes of its print');
  // scene 4's rest, then the close
  await page.evaluate(() => scrollTo(0, __restY(3)));
  await page.waitForFunction(() => __state().shown === 3 && !__state().running && sheets[3], null, { timeout: 30000 });
  // Scene 4's logos come out in one quickening cascade, every one within a few
  // seconds, so a reader who moves on soon takes all their colour into the close.
  await page.waitForFunction(() => __orbit().nodes.every((n) => n.popAt), null, { timeout: 15000 });
  const pops = await page.evaluate(() => __orbit().nodes.map((n) => n.popAt).sort((a, b) => a - b));
  const popGaps = pops.slice(1).map((t, i) => t - pops[i]), mean = (a) => a.reduce((x, y) => x + y, 0) / a.length;
  assert(pops.at(-1) - pops[0] <= 3500 && mean(popGaps.slice(0, 5)) > 1.5 * mean(popGaps.slice(-5)), `scene 4's logos did not come out in one quickening cascade: ${popGaps.map(Math.round)}`);
  console.log(`webkit 1440×900: scene 4's ${pops.length} logos come out in ${Math.round(pops.at(-1) - pops[0])}ms, quickening`);
  await page.waitForTimeout(1500);
  const close = await gesture(3, 4);
  const [c3, c4] = close.rests;
  assert(await page.evaluate(() => !!finalPrints?.rec), 'the desktop close did not play its recorded pair');
  const moving = close.frames.filter((r) => r.y > c3 + 2 && r.y < c4 - 20);
  let still = 0, longest = 0;
  for (let i = 1; i < moving.length; i++) { still = moving[i].y === moving[i - 1].y ? still + (moving[i].t - moving[i - 1].t) : 0; longest = Math.max(longest, still); }
  assert(longest < 120, `the close stopped for ${Math.round(longest)}ms halfway through`);
  const along = (r) => (r.y - c3) / (c4 - c3);
  const lag = close.frames.map((r, i) => r.q > 0 && r.q < 1 && i ? Math.min(Math.abs(r.q - along(r)), Math.abs(r.q - along(close.frames[i - 1]))) : null).filter((d) => d != null);
  assert(lag.length && Math.max(...lag) < .02, `the close did not follow the page between scene 4 and 5: worst ${Math.max(...lag).toFixed(3)}`);
  const closing = close.frames.filter((r) => r.q > 0 && r.q < 1), drifted = closing.find((r) => Math.abs(r.stage - close.frames[0].stage) > 1);
  assert(closing.length && !drifted, `scene 4 did not dissolve in place: its visual moved ${drifted && Math.round(close.frames[0].stage - drifted.stage)}px`);
  const early5 = close.frames.find((r) => r.q >= .999 && r.y < c4 - 3);
  assert(!early5, 'scene 5 stood before the page had arrived at it');
  // and back: the same place, and scene 4's visual pinned again where it was held
  await page.waitForTimeout(800);
  const back = await gesture(4, 3);
  const moved = back.frames.find((r) => Math.abs(r.stage - close.frames[0].stage) > 1);
  assert(!moved, `scene 4 did not gather in place on the way back: its visual moved ${moved && Math.round(close.frames[0].stage - moved.stage)}px`);
  assert(await page.evaluate(() => !document.getElementById('vis').classList.contains('held')), 'scene 4\'s visual was left held after the close let go');
  console.log('webkit 1440×900: the close dissolves and darkens in place with the page, all the way to scene 5 and back');
  await page.close();
  // The reader always has the page. A scrollbar drag (written to the page directly,
  // as a drag on an overlay scrollbar is) is never fought and is left alone once it
  // stops; only after a pause does the carry finish the transition it was left in.
  // Keys step scenes at the pace, even with the editor demo holding focus for its
  // caret, until the reader works in that demo. A trackpad flick keeps its speed.
  const control = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await control.goto(base);
  await control.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  await control.mouse.move(720, 450);
  for (let i = 0; i < 3; i++) { await control.mouse.wheel(0, 40); await control.waitForTimeout(30); }
  await control.waitForFunction(() => Math.abs(scrollY - __restY(0)) <= 1 && !__state().running, null, { timeout: 15000 })
    .catch(() => { throw new Error('the control page did not come to rest on scene 1'); });
  const drag = await control.evaluate(async () => {
    // left 60% of the way to scene 2's rest, well inside the transition
    const y0 = scrollY, fought = [], steps = Math.round((__restY(1) - y0) * .6 / 12);
    for (let k = 1; k <= steps; k++) { scrollTo(0, y0 + 12 * k); await new Promise((r) => requestAnimationFrame(r)); fought.push(Math.abs(scrollY - (y0 + 12 * k))); }
    const released = scrollY, t0 = performance.now(); let firstMove = null;
    while (performance.now() - t0 < 6000) { await new Promise((r) => requestAnimationFrame(r)); if (firstMove == null && Math.abs(scrollY - released) > 1) firstMove = performance.now() - t0; }
    return { fought: Math.max(...fought), firstMove, end: scrollY, rest: __restY(1) };
  });
  assert(drag.fought <= 1, `a scrollbar drag was fought by ${drag.fought}px`);
  assert(drag.firstMove == null || drag.firstMove >= 500, `a stopped scrollbar drag was taken over after ${Math.round(drag.firstMove)}ms`);
  assert(Math.abs(drag.end - drag.rest) <= 1, `a drag left mid-transition was not finished the way it went: ${JSON.stringify(drag)}`);
  const key = async (name) => {
    const t0 = Date.now(); await control.keyboard.press(name);
    await control.waitForFunction(() => !__state().running && __state().carryGoal != null && Math.abs(scrollY - __restY(__state().carryGoal)) <= 1, null, { timeout: 15000 }).catch(() => {});
    return { y: await control.evaluate(() => scrollY), ms: Date.now() - t0 };
  };
  // Shift with an arrow extends a selection: it is the browser's, never a scene step
  await control.evaluate(() => { window.__shifted = null; addEventListener('keydown', (e) => { if (e.key === 'ArrowDown') setTimeout(() => { window.__shifted = e.defaultPrevented; }); }, true); });
  await control.keyboard.press('Shift+ArrowDown');
  await control.waitForFunction(() => window.__shifted !== null, null, { timeout: 5000 });
  assert(!(await control.evaluate(() => window.__shifted)), 'Shift+ArrowDown was taken as a scene step instead of extending a selection');
  await control.waitForFunction(() => Math.abs(scrollY - __restY(1)) <= 1 && !__state().running, null, { timeout: 15000 }).catch(() => {});
  const down = await key('PageDown'), rest2 = await control.evaluate(() => __restY(2));
  assert(Math.abs(down.y - rest2) <= 1 && down.ms >= 1500, `PageDown did not carry to the next scene at the pace: ${JSON.stringify({ ...down, rest2 })}`);
  const up = await key('PageUp'), rest1 = await control.evaluate(() => __restY(1));
  assert(Math.abs(up.y - rest1) <= 1, `PageUp did not carry back a scene: ${JSON.stringify({ ...up, rest1 })}`);
  // back in scene 1, where the editor demo takes focus for its caret: keys arriving in its frame still step
  const first = await key('PageUp'), rest0 = await control.evaluate(() => __restY(0));
  assert(Math.abs(first.y - rest0) <= 1, `PageUp did not carry back to scene 1: ${JSON.stringify({ ...first, rest0 })}`);
  const editorFocus = () => control.waitForFunction(() => document.activeElement?.id === 'ed', null, { timeout: 8000 }).catch(() => { throw new Error('the editor demo no longer takes focus, so this check no longer covers keys arriving in its frame'); });
  await editorFocus();
  const fromDemo = await key('PageDown');
  assert(Math.abs(fromDemo.y - rest1) <= 1, `PageDown with the editor demo holding focus did not step: ${JSON.stringify({ ...fromDemo, rest1 })}`);
  await key('PageUp'); await editorFocus();
  // once the reader works in the editor demo, its keys are its own
  await control.frameLocator('#ed').locator('.cm-content').click({ timeout: 10000 }).catch(() => { throw new Error('could not click into the editor demo'); });
  const typed = await control.evaluate(() => scrollY); await control.keyboard.press('ArrowDown'); await control.waitForTimeout(900);
  assert(await control.evaluate((y) => Math.abs(scrollY - y) <= 1, typed), 'a key typed in a demo the reader is using moved the page');
  // away by the wheel and back: once the page has focused the editor again, keys are the page's
  await control.mouse.move(1300, 450);
  for (const dy of [100, -100]) {
    for (let i = 0; i < 3; i++) { await control.mouse.wheel(0, dy); await control.waitForTimeout(40); }
    await control.waitForFunction((s) => !__state().running && Math.abs(scrollY - __restY(s)) <= 1, dy > 0 ? 1 : 0, { timeout: 15000 });
  }
  await control.waitForTimeout(2600);   // the typing loop's pause after a wash, then its focus
  const again = await key('PageDown');
  assert(Math.abs(again.y - rest1) <= 1, `after leaving and coming back, keys stayed with a demo the reader had used: ${JSON.stringify(again)}`);
  await control.mouse.click(1300, 60, { timeout: 10000 });   // back on the page
  const flick = await control.evaluate(async () => {
    const out = []; let last = scrollY, lt = performance.now(), on = true;
    const f = (now) => { if (!on) return; out.push(Math.abs(scrollY - last) / Math.max(1, now - lt) * 1000); last = scrollY; lt = now; requestAnimationFrame(f); }; requestAnimationFrame(f);
    window.__flickRelease = () => { const i = out.length; return () => { on = false; return Math.max(...out.slice(i + 4)); }; };
  });
  for (const d of [20, 40, 60, 80, 80, 80]) { await control.mouse.wheel(0, d); await control.waitForTimeout(16); }
  await control.evaluate(() => { window.__flickPeak = __flickRelease(); });
  await control.waitForTimeout(1200);
  const peak = await control.evaluate(() => __flickPeak()), pace = await control.evaluate(() => CARRY_VMAX * innerHeight);
  assert(peak > 2 * pace, `a trackpad flick was slowed to the pace: ${Math.round(peak)}px/s against ${Math.round(pace)}`);
  console.log('webkit 1440×900: a drag is never fought, keys step scenes (also from the editor demo) and stay with it once used, a flick keeps its speed');
  await control.close();
  // a hard flick from scene 1 passes as many scenes as its speed would coast it
  const hard = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await hard.goto(base);
  await hard.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  await hard.mouse.move(1300, 450);
  for (let i = 0; i < 3; i++) { await hard.mouse.wheel(0, 40); await hard.waitForTimeout(30); }
  await hard.waitForFunction(() => Math.abs(scrollY - __restY(0)) <= 1 && !__state().running, null, { timeout: 15000 });
  for (const d of [60, 100, 140, 160, 160, 160]) { await hard.mouse.wheel(0, d); await hard.waitForTimeout(16); }
  // read once the release has been taken for a fling, not at a fixed delay a loaded machine can outrun
  await hard.waitForFunction(() => carryFling > 0 || !__state().running, null, { timeout: 5000 });
  const flung = await hard.evaluate(() => __state().carryGoal);
  assert(flung >= 3, `a hard flick from scene 1 was stopped at scene ${flung + 1}`);
  await hard.waitForFunction((g) => !__state().running && Math.abs(scrollY - __restY(g)) <= 1, flung, { timeout: 20000 })
    .catch(() => { throw new Error(`a hard flick did not arrive at scene ${flung + 1}`); });
  console.log(`webkit 1440×900: a hard flick from scene 1 passes on to scene ${flung + 1}`);
  await hard.close();
  // a page opened narrow and widened past the phone layout still gets its title's wash
  const narrow = await browser.newPage({ viewport: { width: 800, height: 900 } });
  await narrow.goto(base);
  await narrow.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  await narrow.setViewportSize({ width: 1440, height: 900 });
  await narrow.waitForFunction(() => titleWash && titleWash.sim.recorded(titleWash.rec), null, { timeout: 15000 })
    .catch(() => { throw new Error('widened past the phone layout, the title never got its wash'); });
  console.log('webkit 800→1440: a widened page takes the title\'s wash');
  await narrow.close();
} finally {
  await browser.close();
}
