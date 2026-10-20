#!/usr/bin/env node
// Desktop's title dissolves as ink and is gone by scene 1. The picture runs
// from scene to scene at the page's pace: it sets off with the page, takes
// about a second after a notch, trails a slow drag, never runs a leg in under
// 0.8 s nor takes more than a few seconds when the page has stopped, and
// turns toward a new target without a jump. The close darkens in place and
// brings in the film as the leg runs. Keys step scenes, also from the editor
// demo, until the reader works in that demo. Scene 2's Publish cue is one ring
// at a time, and a resting scene redraws nothing it does not show. WebKit
// only: headless Chromium renders the desktop simulation in software at about
// a frame a second.
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
      try { if (window.__state) { const s = __state(); __pace.push({ t: now, y: scrollY, shown: s.shown, p: s.leg?.p ?? null, x: s.leg?.x ?? null, to: s.leg?.y ?? null, xf: s.xf, stage: document.getElementById('stage').getBoundingClientRect().top }); } } catch (e) {}
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.goto(base);
  await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  // The title dissolves as ink in its own simulation and is flushed off the
  // sheet by scene 1's rest: its canvas's own pixels, so the stage and copy
  // around it cannot stand in for it.
  await page.waitForFunction(() => titleWash && titleWash.sim.sideRecorded(titleWash.side), null, { timeout: 15000 });
  const title = await page.evaluate(async () => {
    const rest = __restY(0);
    const look = async (u) => {
      for (const t0 = performance.now(); !(titleWash && titleWash.sim.sideRecorded(titleWash.side));) {
        if (performance.now() - t0 > 15000) throw new Error('the title\'s wash was never recorded');
        await new Promise((r) => setTimeout(r, 50));
      }
      const c = titleWash.canvas;
      scrollTo({ top: Math.round(rest * u), behavior: 'instant' }); await new Promise((r) => setTimeout(r, 400));
      const g = document.createElement('canvas').getContext('2d'); g.canvas.width = c.width; g.canvas.height = c.height; g.drawImage(c, 0, 0);
      const d = g.getImageData(0, 0, c.width, c.height).data; let ink = 0;
      for (let i = 3; i < d.length; i += 4) ink = Math.max(ink, d[i]);
      return { shown: getComputedStyle(c).display !== 'none', ink, text: getComputedStyle(openingTitle).color };
    };
    const mid = await look(.4), end = await look(.97), at = await look(1);
    scrollTo({ top: 0, behavior: 'instant' }); await new Promise((r) => setTimeout(r, 400));
    return { mid, end, at, top: { shown: getComputedStyle(titleWash.canvas).display !== 'none', text: getComputedStyle(openingTitle).color } };
  });
  assert(title.mid.shown && title.mid.ink > 100 && title.mid.text === 'rgba(0, 0, 0, 0)', `the title is not dissolving as ink: ${JSON.stringify(title.mid)}`);
  assert(title.end.ink < 12, `the title's wash is not flushed off by scene 1's rest: ${JSON.stringify(title.end)}`);
  assert(!title.at.shown && title.at.text === 'rgba(0, 0, 0, 0)', `something of the title stands at scene 1's rest: ${JSON.stringify(title.at)}`);
  assert(!title.top.shown && title.top.text !== 'rgba(0, 0, 0, 0)', `the title did not come back whole at the top: ${JSON.stringify(title.top)}`);
  console.log('webkit 1440×900: the title dissolves as ink and is gone by scene 1, and gathers again at the top');
  await page.mouse.move(30, 450);   // the left margin, clear of every demo frame

  // standing on a scene: the page there, the picture whole on it, and long enough still to count as a rest
  const stand = async (scene) => {
    await page.evaluate((s) => scrollTo({ top: __restY(s), behavior: 'instant' }), scene);
    await page.waitForFunction((s) => { const t = __state(); return !t.leg && t.shown === s && Math.abs(scrollY - __restY(s)) <= 1; }, scene, { timeout: 30000 })
      .catch(async () => { throw new Error(`the picture did not stand on scene ${scene + 1}: ${JSON.stringify(await page.evaluate(() => __state()))}`); });
    await page.waitForTimeout(900);
  };
  // the prints on hand, so a leg's timing is the pace and not the captures
  await page.waitForFunction(() => [0, 1, 2].every((i) => __onHand[i]), null, { timeout: 60000 });
  // sends the input, waits for the picture to arrive on scene `to`, and returns what the frames showed
  const run = async (to, input, label) => {
    const mark = await page.evaluate(() => performance.now());
    await input();
    await page.waitForFunction((to) => { const s = __state(); return s.shown === to && !s.leg; }, to, { timeout: 30000 })
      .catch(async () => { throw new Error(`${label}: the picture did not arrive on scene ${to + 1}: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, state: __state() })))}`); });
    await page.waitForTimeout(600);
    const frames = await page.evaluate((m) => __pace.filter((r) => r.t >= m), mark);
    const first = frames.find((r) => r.p != null), last = frames.findLast((r) => r.p != null), endY = frames.at(-1).y;
    assert(first, `${label}: no leg ran`);
    const arrivedAt = frames.find((r, i) => frames.slice(i).every((q) => Math.abs(q.y - endY) <= 1)).t;
    return { frames, mark, start: first.t, end: last.t, ms: last.t - first.t, page: arrivedAt, endY };
  };
  const rests = await page.evaluate(() => [0, 1, 2, 3, 4].map(__restY));
  const through = (r, from, to) => (r.y - rests[from]) / (rests[to] - rests[from]);

  // A notch: the picture sets off after the page does and takes about a second, never under a leg's
  // fastest, and is not whole before the page is there.
  await stand(0);
  const notch = await run(1, () => page.mouse.wheel(0, 100), 'a notch');
  assert(notch.start - notch.mark < 800, `the picture set off ${Math.round(notch.start - notch.mark)}ms after a notch`);
  assert(notch.ms >= 700 && notch.ms <= 2500, `a notch's picture ran ${Math.round(notch.ms)}ms, not between 700 and 2500`);
  assert(notch.end >= notch.page - 100, `a notch's picture was whole ${Math.round(notch.page - notch.end)}ms before the page arrived`);
  const back = notch.frames.filter((r) => r.p != null);
  assert(back.every((r, i) => !i || r.p >= back[i - 1].p - .02), 'a notch\'s picture turned back on its way');
  // A slow drag is followed: the picture waits until the page is past halfway to the next scene, and
  // is then at most a moment behind it.
  await stand(1);
  const slow = await run(2, async () => { for (let i = 0; i < 168; i++) { await page.mouse.wheel(0, 4); await page.waitForTimeout(1000 / 60); } }, 'a slow drag');
  const at = slow.frames.find((r) => r.p != null);
  assert(through(at, 1, 2) >= .4, `a slow drag's picture set off when the page was only ${Math.round(through(at, 1, 2) * 100)}% of the way`);
  // read by position, not time: on a busy machine the drag's own last pixels can take seconds
  const whole = slow.frames.findLast((r) => r.p != null);
  assert(through(whole, 1, 2) >= .9 && slow.end <= slow.page + 1500, `a slow drag's picture was whole with the page ${Math.round(through(whole, 1, 2) * 100)}% of the way, ${Math.round(slow.end - slow.page)}ms after it arrived`);
  // A page that has stopped short still finishes: the picture is as far through as the page, but no
  // slower than a leg in a couple of seconds.
  await stand(2);
  const still = await run(3, () => page.evaluate(() => __target(3)), 'a target with the page standing still');
  assert(still.ms >= 1800 && still.ms <= 5000, `the picture took ${Math.round(still.ms)}ms to a target the page did not move toward, not between 1800 and 5000`);
  console.log(`webkit 1440×900: a notch's picture runs ${Math.round(notch.ms)}ms and is whole ${Math.round(notch.end - notch.page)}ms after the page; a slow drag's is whole ${Math.round(slow.end - slow.page)}ms after it; a stopped page's takes ${Math.round(still.ms)}ms`);

  // A leg turned toward another scene goes on from where it stands: the canvas changes by no more at
  // a turn than it does between ordinary frames.
  await stand(0);
  const turns = await page.evaluate(() => new Promise((done) => {
    const gl = document.getElementById('gl'), c = document.createElement('canvas'); c.width = 96; c.height = 96;
    const g = c.getContext('2d', { willReadFrequently: true }), px = [], marks = []; let stage = 0;
    __target(1);
    const tick = () => {
      const s = __state();
      if (s.leg) {
        g.clearRect(0, 0, 96, 96); g.drawImage(gl, 0, 0, 96, 96); px.push(Array.from(g.getImageData(0, 0, 96, 96).data));
        if (stage === 0 && s.leg.p >= .25) { stage = 1; marks.push(px.length); __target(3); }
        else if (stage === 1 && s.leg.p >= .5 && s.leg.recorded) { stage = 2; marks.push(px.length); __target(2); }
        else if (stage === 2 && s.leg.p >= .8) { stage = 3; marks.push(px.length); __target(1); }
      } else if (stage === 3) {
        const d = px.slice(1).map((b, i) => { let t = 0; for (let j = 0; j < b.length; j++) t += Math.abs(px[i][j] - b[j]); return t / b.length; });
        done({ d, marks }); return;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }));
  const median = [...turns.d].sort((x, y) => x - y)[turns.d.length >> 1];
  const worstTurn = Math.max(...turns.marks.map((i) => Math.max(...turns.d.slice(i - 2, i + 2))));
  assert(worstTurn <= median * 2.5 + 1.5, `a leg jumped where it turned: ${worstTurn.toFixed(2)} against a median frame change of ${median.toFixed(2)}`);
  console.log(`webkit 1440×900: a leg turned three times changes the canvas by at most ${worstTurn.toFixed(2)} (median frame ${median.toFixed(2)})`);
  await stand(1);
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
    const root = document.documentElement.style, setProperty = root.setProperty, play = titleWash.sim.playSides;
    root.setProperty = function (name, ...rest) { if (name === '--s' || name === '--opening-lift') rootWrites++; return setProperty.call(this, name, ...rest); };
    titleWash.sim.playSides = function (...a) { titleDraws++; return play.apply(this, a); };
    const from = cueRings.length, t0 = performance.now();
    while (performance.now() - t0 < 7600) {
      if (!canvas.hidden) { const px = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data; for (let i = 3; i < px.length; i += 4) if (px[i] > ink) ink = px[i]; }
      await new Promise((r) => setTimeout(r, 250));
    }
    watch.disconnect(); delete root.setProperty; titleWash.sim.playSides = play;
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
  await stand(3);
  // Scene 4's logos come out in one quickening cascade, every one within a few
  // seconds, so a reader who moves on soon takes all their colour into the close.
  await page.waitForFunction(() => __orbit().nodes.every((n) => n.popAt), null, { timeout: 15000 });
  const pops = await page.evaluate(() => __orbit().nodes.map((n) => n.popAt).sort((a, b) => a - b));
  const popGaps = pops.slice(1).map((t, i) => t - pops[i]), mean = (a) => a.reduce((x, y) => x + y, 0) / a.length;
  assert(pops.at(-1) - pops[0] <= 3500 && mean(popGaps.slice(0, 5)) > 1.5 * mean(popGaps.slice(-5)), `scene 4's logos did not come out in one quickening cascade: ${popGaps.map(Math.round)}`);
  console.log(`webkit 1440×900: scene 4's ${pops.length} logos come out in ${Math.round(pops.at(-1) - pops[0])}ms, quickening`);
  await page.waitForTimeout(1500);
  // The close is a target like any other: its film comes up as the leg runs and scene 5 stands when it arrives.
  const stepKey = (key) => () => page.keyboard.press(key);
  const close = await run(4, stepKey('PageDown'), 'the close');
  const legs = close.frames.filter((r) => r.p != null);
  assert(legs.length > 5 && legs.every((r) => r.x === 3 && r.to === 4), `the close was not one leg from scene 4 to 5: ${JSON.stringify(legs.filter((r) => r.x !== 3 || r.to !== 4).slice(0, 2))}`);
  // the film eases toward where the leg puts it, so it sits between where the leg put it 400ms ago and where it puts it now, whichever way it runs
  const filmOff = async (frames, along) => {
    const want = await page.evaluate((ps) => ps.map((p) => crossAt(p)), frames.map((r) => along(r.p)));
    return Math.max(...frames.map((r, i) => {
      const past = frames.findLast((q) => q.t <= r.t - 400), before = want[past ? frames.indexOf(past) : 0];
      return Math.max(0, r.xf - Math.max(before, want[i]) - .05, Math.min(before, want[i]) - .05 - r.xf);
    }));
  };
  const forward = await filmOff(legs, (p) => p);
  assert(forward === 0, `the film did not follow the close's leg: worst ${forward.toFixed(3)} outside where the leg puts it`);
  assert(legs.some((r) => r.xf > .2 && r.xf < .8), 'the film never crossed fading in: it was cut');
  assert(close.frames.at(-1).xf >= .99, `scene 5 did not stand with the film whole: ${close.frames.at(-1).xf}`);
  const drifted = legs.find((r) => Math.abs(r.stage - close.frames[0].stage) > 1);
  assert(!drifted, `scene 4 did not dissolve in place: its visual moved ${drifted && Math.round(close.frames[0].stage - drifted.stage)}px`);
  // and back: the film goes as scene 4 gathers, in the same place
  await page.waitForTimeout(800);
  const returned = await run(3, stepKey('PageUp'), 'back from the close');
  const gone = returned.frames.filter((r) => r.p != null);
  const outward = await filmOff(gone, (p) => 1 - p);
  assert(gone.length > 5 && returned.frames.at(-1).xf === 0 && outward === 0, `the film did not go with the leg back to scene 4: worst ${outward.toFixed(3)} outside where the leg puts it`);
  const moved = returned.frames.find((r) => Math.abs(r.stage - close.frames[0].stage) > 1);
  assert(!moved, `scene 4 did not gather in place on the way back: its visual moved ${moved && Math.round(close.frames[0].stage - moved.stage)}px`);
  // the visual is held from a quarter screen before scene 4's rest, where the pin already stands; back at scene 3 it is let go
  await stand(2);
  assert(await page.evaluate(() => !document.getElementById('vis').classList.contains('held')), 'scene 4\'s visual was left held after the close let go');
  console.log(`webkit 1440×900: the close's film follows its leg (${Math.round(close.ms)}ms) and scene 4 dissolves and gathers in place`);
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
  await narrow.waitForFunction(() => titleWash && titleWash.sim.sideRecorded(titleWash.side), null, { timeout: 15000 })
    .catch(() => { throw new Error('widened past the phone layout, the title never got its wash'); });
  console.log('webkit 800→1440: a widened page takes the title\'s wash');
  await narrow.close();
} finally {
  await browser.close();
}
