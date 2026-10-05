#!/usr/bin/env node
// Desktop's title dissolves as ink and is gone by scene 1, and each join plays
// from the page's position between two rests, which the browser scrolls and
// snaps: the picture follows the page at a pace slow enough to watch, the next
// scene is whole exactly when the picture arrives, and the close darkens in
// place through its own recording without stopping halfway. Keys step scenes,
// also from the editor demo, until the reader works in that demo.
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
      try { if (window.__state) { const s = __state(); __pace.push({ t: now, y: scrollY, washT: s.washT, running: s.running, shown: s.shown, q: finalDissolve, xf: s.xf, stage: document.getElementById('stage').getBoundingClientRect().top, v: s.view }); } } catch (e) {}
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
    // the page snaps to its rests, so a position between them is held with the snap off, and read once the picture has caught up
    const nosnap = document.head.appendChild(Object.assign(document.createElement('style'), { textContent: 'html { scroll-snap-type: none !important }' }));
    const arrived = async () => { while (Math.abs(__state().view - scrollY) > 1) await new Promise((r) => setTimeout(r, 50)); };
    const look = async (u) => {
      for (const t0 = performance.now(); !(titleWash && titleWash.sim.recorded(titleWash.rec));) {
        if (performance.now() - t0 > 15000) throw new Error('the title\'s wash was never recorded');
        await new Promise((r) => setTimeout(r, 50));
      }
      const c = titleWash.canvas;
      scrollTo(0, Math.round(rest * u)); await arrived(); await new Promise((r) => setTimeout(r, 250));
      const g = document.createElement('canvas').getContext('2d'); g.canvas.width = c.width; g.canvas.height = c.height; g.drawImage(c, 0, 0);
      const d = g.getImageData(0, 0, c.width, c.height).data; let ink = 0;
      for (let i = 3; i < d.length; i += 4) ink = Math.max(ink, d[i]);
      return { shown: getComputedStyle(c).display !== 'none', ink, text: getComputedStyle(openingTitle).color };
    };
    const mid = await look(.4), end = await look(.97), at = await look(1);
    scrollTo(0, 0); await arrived(); await new Promise((r) => setTimeout(r, 250)); nosnap.remove();
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
  // A step is a key: the browser scrolls the page to the next rest, and what is recorded is the picture's
  // position (v), which follows it.
  const gesture = async (from, to) => {
    const mark = await page.evaluate(() => performance.now());
    await page.keyboard.press(to > from ? 'PageDown' : 'PageUp');
    await page.waitForFunction((to) => __state().shown === to && !__state().running && Math.abs(scrollY - __restY(to)) <= 1 && Math.abs(__state().view - scrollY) <= 1, to, { timeout: 20000 })
      .catch(async (error) => { throw new Error(`the step to scene ${to + 1} did not finish: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, rests: [0, 1, 2, 3, 4].map(__restY), state: __state() })))}`, { cause: error }); });
    await page.waitForTimeout(300);
    return { rests: await page.evaluate(([a, b]) => [__restY(a), __restY(b)], [from, to]), frames: await page.evaluate((mark) => __pace.filter((r) => r.t >= mark), mark) };
  };
  // scene 1's rest, then one gesture into scene 2
  await page.keyboard.press('PageDown');
  await page.waitForFunction(() => Math.abs(scrollY - __restY(0)) <= 1 && !__state().running, null, { timeout: 15000 });
  await page.waitForFunction(() => [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
  const join = await gesture(0, 1);
  const [r0, r1] = join.rests;
  // the picture sets off the frame the page moves and arrives still, about two seconds later
  const set = join.frames.find((r) => r.y !== r0), arrival = join.frames.find((r) => Math.abs(r.v - r1) <= 1);
  const travel = arrival.t - set.t, pace = await page.evaluate(() => WASH_PACE * innerHeight);
  assert(travel >= 1600, `scene 1 to 2 was travelled by the picture in ${Math.round(travel)}ms, too fast to watch`);
  // read over a quarter second, so a frame that arrives late does not read as a burst of speed
  const speeds = join.frames.map((r, i) => { const from = join.frames.slice(0, i).findLast((f) => r.t - f.t >= 250); return from ? Math.abs(r.v - from.v) / (r.t - from.t) * 1000 : 0; });
  assert(Math.max(...speeds) <= pace * 1.15 + 20, `the picture travelled at ${Math.round(Math.max(...speeds))}px/s, faster than its pace of ${Math.round(pace)}`);
  assert(join.frames.some((r) => r.running && Math.abs(r.v - r0) < 60), 'the wash did not start with the gesture');
  const T = await page.evaluate(() => T_TOTAL);
  // the wash is drawn in the frame after the picture moves, so one frame of lag is the position's own
  const at = (r) => Math.min(1, Math.max(0, (r.v - r0) / (r1 - r0)));
  const drift = join.frames.map((r, i) => r.running && i ? Math.min(Math.abs(r.washT / T - at(r)), Math.abs(r.washT / T - at(join.frames[i - 1]))) : null).filter((d) => d != null);
  assert(drift.length && Math.max(...drift) < .04, `the wash did not follow the picture between the rests: worst drift ${Math.max(...drift).toFixed(3)}`);
  const early = join.frames.find((r) => r.washT >= T - .01 && r.v < r1 - 3);
  assert(!early, `scene 2 was whole ${early && Math.round(r1 - early.v)}px before the picture arrived`);
  console.log(`webkit 1440×900: scene 1 to 2: the picture travels over ${Math.round(travel)}ms at no more than its pace; the wash tracks it and is whole on arrival`);
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
  // Clicking the demo's Publish (through the page, the way a reader does)
  // opens the app's publish receipt for the example site: checking first, the
  // pending changes as rows naming the site's real pages, then the verdict.
  const receiptAt = await page.evaluate(() => {
    gen++; const sh = shFrame.contentWindow.__shell; sh.hold(); sh.setPending({ edited: 2, added: 1 });
    const f = shFrame.getBoundingClientRect(), k = f.width / shFrame.offsetWidth, b = shFrame.contentDocument.querySelector('.moss-publish-button').getBoundingClientRect();
    return { x: f.left + (b.left + b.width / 2) * k, y: f.top + (b.top + b.height / 2) * k };
  });
  await page.mouse.click(receiptAt.x, receiptAt.y);
  const receiptRead = () => page.evaluate(() => {
    const m = shFrame.contentDocument.querySelector('.moss-modal--receipt.visible');
    return m && { title: m.querySelector('.moss-modal-title').textContent, rows: [...m.querySelectorAll('.receipt-row')].map((r) => [r.querySelector('.receipt-label')?.textContent, r.querySelector('.receipt-host')?.textContent || r.querySelector('.receipt-detail')?.textContent]),
      marks: [...m.querySelectorAll('svg.mark')].map((s) => s.dataset.state), primary: m.querySelector('.moss-btn-primary').textContent };
  });
  const receipt = await receiptRead();
  assert(receipt && receipt.title === 'Checking your site is live…' && receipt.primary === 'View page', `Publish did not open the checking receipt: ${JSON.stringify(receipt)}`);
  assert(JSON.stringify(receipt.rows) === JSON.stringify([['Uploaded', '3 files uploaded'], ['Added', 'Illustrations of the Book of Job'], ['Live', 'Checking…']]), `the receipt's rows are not the demo's changes: ${JSON.stringify(receipt.rows)}`);
  await page.waitForFunction(() => shFrame.contentDocument.querySelector('.moss-modal--receipt .moss-modal-title')?.textContent === 'Your site is live', null, { timeout: 5000 })
    .catch(async () => { throw new Error(`the receipt's verdict never landed: ${JSON.stringify(await receiptRead())}`); });
  const settled = await receiptRead();
  assert(settled.marks.every((st) => st === 'done'), `the receipt's marks did not all settle: ${settled.marks}`);
  await page.keyboard.press('Escape');
  await page.waitForFunction(() => !shFrame.contentDocument.querySelector('.moss-modal--receipt.visible'), null, { timeout: 3000 });
  // a click while the ring is still growing reports the changes the ring shows
  await page.evaluate(() => { const sh = shFrame.contentWindow.__shell; sh.setPending({}); sh.growTo({ edited: 4, added: 2 }, 2400); });
  await page.waitForTimeout(1500);
  await page.mouse.click(receiptAt.x, receiptAt.y);
  const mid = await receiptRead();
  assert(mid && mid.rows[0][1] !== 'Nothing new — nothing else changed' && mid.rows.some(([label]) => label === 'Added'), `a Publish clicked while the ring grew reported the ring before it: ${JSON.stringify(mid)}`);
  await page.keyboard.press('Escape');
  await page.waitForFunction(() => !shFrame.contentDocument.querySelector('.moss-modal--receipt.visible'), null, { timeout: 3000 });
  console.log('webkit 1440×900: scene 2\'s Publish opens the example site\'s publish receipt, which settles to live');
  await page.mouse.click(30, 450);   // back on the page: keys in a demo the reader has used are the demo's
  // A wash that lands has already shown scene 3 whole in its print, so when the
  // print is hidden the sketch and notebook must already stand at full opacity:
  // an entrance fade replayed under the live scene is the art vanishing for
  // about a third of a second right after it consolidated.
  await page.evaluate(() => {
    window.__handoff = [];
    const tick = (now) => { const s = __state(); __handoff.push({ t: now, shown: s.shown, running: s.running, print: getComputedStyle(document.getElementById('gl')).display !== 'none', sk: +getComputedStyle(document.getElementById('sib-sk')).opacity, nb: +getComputedStyle(document.getElementById('sib-nb')).opacity }); requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
  });
  await page.evaluate(() => scrollTo(0, __restY(2)));
  await page.waitForFunction(() => __state().shown === 2 && !__state().running, null, { timeout: 30000 });
  await page.waitForTimeout(1500);
  const handoff = await page.evaluate(() => { const end = __handoff.find((f) => f.shown === 2 && !f.running); return end ? __handoff.filter((f) => f.t >= end.t && f.t <= end.t + 1000) : null; });
  assert(handoff && handoff.length > 10, 'no frames recorded after the wash into scene 3 ended');
  // the check means nothing unless the recording saw the print up and then gone
  const seen = await page.evaluate(() => { const up = __handoff.findIndex((f) => f.print); return { up, down: up < 0 ? -1 : __handoff.findIndex((f, i) => i > up && !f.print), hidden: __handoff.filter((f) => !f.print).length }; });
  assert(seen.up >= 0 && seen.down > seen.up, `the scroll into scene 3 did not show its print and then hide it, so the handoff was not exercised: ${JSON.stringify(seen)}`);
  const dim = handoff.filter((f) => !f.print && (f.sk < .99 || f.nb < .99));
  assert(!dim.length, `scene 3's art faded in again after its wash landed: ${dim.length} frames with the print hidden, lowest sketch ${Math.min(...dim.map((f) => f.sk))}, notebook ${Math.min(...dim.map((f) => f.nb))}`);
  console.log('webkit 1440×900: scene 3\'s sketch and notebook are whole the frame its wash lands');
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
  const moving = close.frames.filter((r) => r.v > c3 + 2 && r.v < c4 - 20);
  let still = 0, longest = 0;
  for (let i = 1; i < moving.length; i++) { still = moving[i].v === moving[i - 1].v ? still + (moving[i].t - moving[i - 1].t) : 0; longest = Math.max(longest, still); }
  assert(longest < 120, `the close stopped for ${Math.round(longest)}ms halfway through`);
  const along = (r) => (r.v - c3) / (c4 - c3);
  const lag = close.frames.map((r, i) => r.q > 0 && r.q < 1 && i ? Math.min(Math.abs(r.q - along(r)), Math.abs(r.q - along(close.frames[i - 1]))) : null).filter((d) => d != null);
  assert(lag.length && Math.max(...lag) < .02, `the close did not follow the picture between scene 4 and 5: worst ${Math.max(...lag).toFixed(3)}`);
  const closing = close.frames.filter((r) => r.q > 0 && r.q < 1), drifted = closing.find((r) => Math.abs(r.stage - close.frames[0].stage) > 1);
  assert(closing.length && !drifted, `scene 4 did not dissolve in place: its visual moved ${drifted && Math.round(close.frames[0].stage - drifted.stage)}px`);
  const early5 = close.frames.find((r) => r.q >= .999 && r.v < c4 - 3);
  assert(!early5, 'scene 5 stood before the picture had arrived at it');
  // and back: the same place, and scene 4's visual pinned again where it was held
  await page.waitForTimeout(800);
  const back = await gesture(4, 3);
  const moved = back.frames.find((r) => Math.abs(r.stage - close.frames[0].stage) > 1);
  assert(!moved, `scene 4 did not gather in place on the way back: its visual moved ${moved && Math.round(close.frames[0].stage - moved.stage)}px`);
  // the visual is held from a quarter screen before scene 4's rest, where the pin already stands; back at scene 3 it is let go
  await gesture(3, 2);
  assert(await page.evaluate(() => !document.getElementById('vis').classList.contains('held')), 'scene 4\'s visual was left held after the close let go');
  console.log('webkit 1440×900: the close dissolves and darkens in place with the picture, all the way to scene 5 and back');
  await page.close();
  // Keys are the page's until the reader works in the editor demo, which takes focus for its caret.
  const control = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await control.goto(base);
  await control.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  await control.mouse.move(1300, 450);
  await control.keyboard.press('PageDown');
  await control.waitForFunction(() => Math.abs(scrollY - __restY(0)) <= 1 && !__state().running, null, { timeout: 15000 })
    .catch(() => { throw new Error('the control page did not come to rest on scene 1'); });
  const editorFocus = () => control.waitForFunction(() => document.activeElement?.id === 'ed', null, { timeout: 8000 }).catch(() => { throw new Error('the editor demo no longer takes focus, so this check no longer covers keys arriving in its frame'); });
  const key = async (name, scene) => {
    await control.keyboard.press(name);
    await control.waitForFunction((s) => !__state().running && Math.abs(scrollY - __restY(s)) <= 1, scene, { timeout: 15000 }).catch(() => {});
    return control.evaluate(() => scrollY);
  };
  const rest = (i) => control.evaluate((i) => __restY(i), i);
  await key('PageDown', 1);
  // Shift with an arrow extends a selection: it is the browser's, never a scene step (read in scene 2, where no demo holds focus)
  await control.evaluate(() => { window.__shifted = null; addEventListener('keydown', (e) => { if (e.key === 'ArrowDown') setTimeout(() => { window.__shifted = e.defaultPrevented; }); }, true); });
  await control.keyboard.press('Shift+ArrowDown');
  await control.waitForFunction(() => window.__shifted !== null, null, { timeout: 5000 });
  assert(!(await control.evaluate(() => window.__shifted)), 'Shift+ArrowDown was taken as a scene step instead of extending a selection');
  await key('PageUp', 0); await editorFocus();
  assert(Math.abs(await key('PageDown', 1) - await rest(1)) <= 1, 'PageDown with the editor demo holding focus did not step');
  await key('PageUp', 0); await editorFocus();
  // once the reader works in the editor demo, its keys are its own
  await control.frameLocator('#ed').locator('.cm-content').click({ timeout: 10000 }).catch(() => { throw new Error('could not click into the editor demo'); });
  const typed = await control.evaluate(() => scrollY); await control.keyboard.press('ArrowDown'); await control.waitForTimeout(900);
  assert(await control.evaluate((y) => Math.abs(scrollY - y) <= 1, typed), 'a key typed in a demo the reader is using moved the page');
  // away by the wheel and back: once the page has focused the editor again, keys are the page's
  await control.mouse.move(30, 450);
  for (const [dy, scene] of [[100, 1], [-100, 0]]) {
    await control.mouse.wheel(0, dy);
    await control.waitForFunction((s) => !__state().running && Math.abs(scrollY - __restY(s)) <= 1, scene, { timeout: 15000 });
  }
  await control.waitForTimeout(2600);   // the typing loop's pause after a wash, then its focus
  assert(Math.abs(await key('PageDown', 1) - await rest(1)) <= 1, 'after leaving and coming back, keys stayed with a demo the reader had used');
  console.log('webkit 1440×900: keys step scenes (also from the editor demo) and stay with it once used');
  await control.close();
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
