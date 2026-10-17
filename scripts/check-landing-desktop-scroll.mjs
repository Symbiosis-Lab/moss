#!/usr/bin/env node
// Desktop paging, driven with input a browser really handles (trusted wheel
// events and keys) in Chromium and WebKit. Nothing scrolls before input and
// there is no CSS snap. A wheel notch, a nudge, a trackpad swipe of any size
// and a slow drag each end the page at rest on a scene past where they began,
// never between scenes and never back; a drag too short to count returns to
// where it began. A swipe's own push is never cancelled, only its glide
// afterwards; a phone-width page is never cancelled at all,
// and ?snap=off leaves the page unpaged. Keys step one scene, also while the
// editor demo holds focus; End rests on the close, and a notch up from the
// footer rests on the close. Reduced motion lands a key step at once.
import { loadPlaywright, whenReady } from './landing-harness.mjs';

const base = process.argv[2];
if (!base) throw new Error('Usage: node scripts/check-landing-desktop-scroll.mjs <site-url>');
const { chromium, webkit } = await loadPlaywright();
const assert = (ok, message) => { if (!ok) throw new Error(message); };
setTimeout(() => { console.error('Error: check-landing-desktop-scroll did not finish within 4 minutes'); process.exit(1); }, 240000).unref();
const NOTCH = 100;
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
// The wheel events a trackpad swipe sends at 60 Hz: the fingers speeding up, then the glide the
// system adds after they lift (an exponential decay, 325ms).
const swipe = (frames, from, to) => {
  const deltas = [];
  for (let i = 0; i < frames; i++) deltas.push(from + (to - from) * i / (frames - 1));
  for (let k = 1, v; (v = to * Math.exp(-k * (1000 / 60) / 325)) >= .5; k++) deltas.push(v);
  return deltas;
};
const SHORT = swipe(8, 2, 10), ORDINARY = swipe(12, 4, 30), LARGE = swipe(14, 6, 60);   // about 80, 760 and 1600px in all
const drag = (count, step) => Array.from({ length: count }, () => step);                 // fingers moving, no glide

async function open(browser, { query = '', ...options } = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(base + query);
  await whenReady(page);
  // registered after the page's own, so it sees what the page did to each event
  await page.evaluate(() => { window.__wheels = []; addEventListener('wheel', (e) => __wheels.push({ dy: e.deltaY, cancelled: e.defaultPrevented }), { capture: true, passive: true }); });
  await page.mouse.move(30, 450);
  return { page, context, errors };
}
const restY = (page, scene) => page.evaluate((s) => __restY(s), scene);
// each event leaves when the last was sent, not when it was handled
const send = async (page, deltas, sign = 1) => {
  const sent = [];
  for (const delta of deltas) { sent.push(page.mouse.wheel(0, sign * delta)); await sleep(1000 / 60); }
  await Promise.all(sent);
};
const rested = async (page, scene, label) => {
  await page.waitForFunction((s) => Math.abs(scrollY - __restY(s)) <= 1, scene, { timeout: 40000 })
    .catch(async () => { throw new Error(`${label}: the page did not rest on rest ${scene}: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, rests: [-1, 0, 1, 2, 3, 4].map(__restY) })))}`); });
  await sleep(450);
  assert(await page.evaluate((s) => Math.abs(scrollY - __restY(s)) <= 1, scene), `${label}: the page did not hold rest ${scene}, it is at ${await page.evaluate(() => scrollY)}`);
};
// [time, y] per frame from now until the page has been still for 900ms
const watchScroll = (page) => page.evaluate(() => new Promise((done) => {
  const frames = []; let last = -1, since = performance.now();
  const frame = (now) => {
    frames.push([now, scrollY]);
    if (scrollY !== last) { last = scrollY; since = now; }
    if (now - since > 900) done(frames); else requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}));
// stands the page on a scene, picture and all, and lets it be still long enough to count as a rest
const stand = async (page, scene) => {
  await watchScroll(page);   // an earlier gesture's settle would carry the page off again
  await page.evaluate((s) => scrollTo({ top: __restY(s), behavior: 'instant' }), scene);
  await page.waitForFunction((s) => { const t = __state(); return !t.leg && t.shown === Math.max(0, s); }, scene, { timeout: 60000 })
    .catch(async () => { throw new Error(`the picture did not settle on scene ${scene + 1}: ${JSON.stringify(await page.evaluate(() => __state()))}`); });
  await sleep(900);
};
// the longest time the page stood still between leaving where it began and arriving
const stall = (frames) => {
  const end = frames.at(-1)[1]; let arrived = frames.length;
  while (arrived > 0 && Math.abs(frames[arrived - 1][1] - end) <= 2) arrived--;
  let from = null, worst = 0;
  for (let i = 1; i < arrived; i++) {
    const [t, y] = frames[i], moved = Math.abs(y - frames[i - 1][1]) >= .5;
    if (moved || Math.abs(y - frames[0][1]) <= 2) from = null;
    else { from ??= frames[i - 1][0]; worst = Math.max(worst, t - from); }
  }
  return worst;
};

for (const [name, type] of [['chromium', chromium], ['webkit', webkit]]) {
  const browser = await type.launch(name === 'chromium' ? { args: ['--enable-gpu', '--use-angle=metal', '--ignore-gpu-blocklist'] } : {});
  try {
    const { page, context, errors } = await open(browser);
    await sleep(1000);
    assert(await page.evaluate(() => scrollY === 0), `${name}: the page scrolled before any input`);
    const snap = await page.evaluate(() => getComputedStyle(document.documentElement).scrollSnapType);
    assert(snap === 'none', `${name}: the page has a CSS scroll snap: ${snap}`);

    await page.mouse.wheel(0, NOTCH);
    await rested(page, 0, `${name}: one notch from the title`);
    await page.mouse.wheel(0, NOTCH);
    await rested(page, 1, `${name}: one notch from scene 1`);
    await page.mouse.wheel(0, -NOTCH);
    await rested(page, 0, `${name}: one notch back up from scene 2`);

    // keys step one scene, also while the editor demo holds focus for its caret (scene 1)
    await page.waitForFunction(() => document.activeElement?.id === 'ed', null, { timeout: 15000 })
      .catch(() => { throw new Error(`${name}: the editor demo no longer takes focus, so this check no longer covers keys arriving in its frame`); });
    await page.keyboard.press('PageDown');
    await rested(page, 1, `${name}: PageDown with the editor demo focused`);
    await page.keyboard.press('PageDown');
    await rested(page, 2, `${name}: PageDown from scene 2`);
    await page.keyboard.press('PageUp');
    await rested(page, 1, `${name}: PageUp from scene 3`);
    await page.keyboard.press('PageUp');
    await rested(page, 0, `${name}: PageUp from scene 2`);
    await page.waitForFunction(() => document.activeElement?.id === 'ed', null, { timeout: 15000 });
    await page.keyboard.press('PageUp');
    await rested(page, -1, `${name}: PageUp from scene 1 with the editor demo focused`);
    assert(!errors.length, `${name}: page errors: ${errors.join('; ')}`);
    await context.close();

    // a second notch while the first is settling is taken by the settle and does not carry the page a scene further
    const quick = await open(browser);
    await stand(quick.page, 1);
    await Promise.all([quick.page.mouse.wheel(0, NOTCH), quick.page.mouse.wheel(0, NOTCH)]);
    await rested(quick.page, 2, `${name}: two quick notches from scene 2`);
    const notches = await quick.page.evaluate(() => __wheels);
    assert(notches.length === 2 && notches.every((w) => w.cancelled), `${name}: the second of two quick notches was left to the browser: ${JSON.stringify(notches)}`);
    await sleep(1200);
    assert(await quick.page.evaluate(() => Math.abs(scrollY - __restY(2)) <= 1), `${name}: two quick notches went on past scene 3 to ${await quick.page.evaluate(() => scrollY)}`);

    // End rests on the close; the footer lies a key press below it, and one notch up from there is the close again
    await quick.page.keyboard.press('End');
    await rested(quick.page, 4, `${name}: End`);
    // Chromium only: WebKit's ArrowDown moves one 40px step or none, so it is no evidence of where the footer begins
    if (name === 'chromium') {
      await quick.page.keyboard.press('ArrowDown');
      await quick.page.waitForFunction(() => scrollY > __restY(4) + 30, null, { timeout: 40000 })
        .catch(async () => { throw new Error(`${name}: ArrowDown from the close did not leave it: y ${await quick.page.evaluate(() => scrollY)}, close ${await restY(quick.page, 4)}`); });
      await sleep(800);
      await quick.page.mouse.wheel(0, -NOTCH);
      await rested(quick.page, 4, `${name}: one notch up from below the close`);
    }
    await quick.context.close();

    // reduced motion: a key step lands at once, and the wheel is the browser's
    const calm = await open(browser, { reducedMotion: 'reduce' });
    const target = await restY(calm.page, 0);
    await calm.page.evaluate(() => { window.__ys = []; addEventListener('scroll', () => __ys.push(Math.round(scrollY)), { passive: true }); });
    await calm.page.keyboard.press('PageDown');
    await calm.page.waitForFunction((y) => Math.abs(scrollY - y) <= 1, target, { timeout: 40000 });
    await sleep(300);
    const seen = await calm.page.evaluate(() => __ys);
    assert(seen.length && seen.every((y) => Math.abs(y - target) <= 1), `${name}: with reduced motion a key step travelled instead of landing at once: ${seen}`);
    await calm.context.close();

    // Gestures end on a scene past where they began, in the gesture's direction, without a long stall.
    const g = await open(browser);
    const rests = await g.page.evaluate(() => [0, 1, 2, 3, 4].map(__restY));
    const gestures = [
      ['a notch', [NOTCH], 1, 1], ['a 30px nudge', [30], 1, 1], ['a short swipe', SHORT, 1, 1], ['an ordinary swipe', ORDINARY, 1, 1],
      ['a large swipe', LARGE, 1, 1, 2], ['an ordinary swipe', ORDINARY, 1, 2], ['a short swipe up', SHORT, -1, 2], ['an ordinary swipe up', ORDINARY, -1, 2],
      ['a 3s slow drag', drag(180, 4), 1, 1], ['a 40px drag', drag(10, 4), 1, 1],
    ];
    for (const [label, deltas, sign, from, far = 1] of gestures) {
      await stand(g.page, from);
      const watching = watchScroll(g.page);
      await send(g.page, deltas, sign);
      const frames = await watching, end = frames.at(-1)[1];
      const landed = rests.findIndex((r) => Math.abs(r - end) <= 6);
      assert(landed >= 0 && (landed - from) * sign >= far, `${name}: ${label} from scene ${from + 1} ended at ${Math.round(end)}, not on a scene ${far} or more past it (scenes at ${rests.map(Math.round)})`);
      assert(stall(frames) <= 500, `${name}: ${label} from scene ${from + 1} stalled for ${Math.round(stall(frames))}ms on the way`);
    }
    // a drag too short to count returns to where it began
    await stand(g.page, 1);
    await send(g.page, drag(4, 3));
    await rested(g.page, 1, `${name}: a 12px drag from scene 2`);

    // a swipe's own push is the reader's; only its glide is taken
    await stand(g.page, 1);
    await g.page.evaluate(() => { __wheels.length = 0; });
    await send(g.page, ORDINARY);
    const wheels = await g.page.evaluate(() => __wheels);
    assert(wheels.slice(0, 4).every((w) => !w.cancelled), `${name}: the page cancelled the start of a swipe: ${JSON.stringify(wheels.slice(0, 4))}`);
    assert(wheels.some((w) => w.cancelled), `${name}: the page left a swipe's glide to fight its settle`);

    // one coalesced event in a steady drag is no glide: nothing is taken while the fingers move
    await stand(g.page, 1);
    await g.page.evaluate(() => { __wheels.length = 0; });
    await send(g.page, [...drag(6, 4), 8, ...drag(30, 4)]);
    const steady = await g.page.evaluate(() => __wheels);
    assert(steady.every((w) => !w.cancelled), `${name}: a steady drag with one coalesced event was taken for a glide: ${JSON.stringify(steady.map((w) => w.cancelled ? 'x' : '.').join(''))}`);

    // a stray small push against a settle still leaves the page on a scene, not between two
    await stand(g.page, 1);
    const stray = watchScroll(g.page);
    await send(g.page, ORDINARY.slice(0, 20));
    await sleep(120); await g.page.mouse.wheel(0, -6);
    const strayEnd = (await stray).at(-1)[1];
    assert(rests.some((r) => Math.abs(r - strayEnd) <= 1), `${name}: a stray push during a settle left the page between scenes, at ${Math.round(strayEnd)} (scenes at ${rests.map(Math.round)})`);

    // a glide's tail that arrives late, on a busy page, is still the glide's and cannot carry the page past its scene
    await stand(g.page, 1);
    const late = watchScroll(g.page);
    await send(g.page, ORDINARY.slice(0, 22));
    for (const d of [3, 2, 2, 1, 1, 1]) { await sleep(150); await g.page.mouse.wheel(0, d); }
    const lateEnd = (await late).at(-1)[1];
    assert(rests.findIndex((r) => Math.abs(r - lateEnd) <= 1) >= 2, `${name}: a glide's late tail left the page off the scene it was going to, at ${Math.round(lateEnd)} (scenes at ${rests.map(Math.round)})`);
    assert(!g.errors.length, `${name}: page errors: ${g.errors.join('; ')}`);
    await g.context.close();

    // ?snap=off: a notch is the browser's alone
    const free = await open(browser, { query: '?snap=off' });
    await free.page.mouse.wheel(0, NOTCH);
    await sleep(1500);
    const freed = await free.page.evaluate(() => ({ y: scrollY, rests: [-1, 0, 1].map(__restY), cancelled: __wheels.filter((w) => w.cancelled).length }));
    assert(freed.y > 0 && freed.rests.every((r) => Math.abs(r - freed.y) > 20) && !freed.cancelled, `${name}: with ?snap=off the page still paged: ${JSON.stringify(freed)}`);
    await free.context.close();

    // a phone-width page scrolls freely
    const narrow = await open(browser, { viewport: { width: 800, height: 900 } });
    await narrow.page.mouse.wheel(0, NOTCH);
    await sleep(1500);
    const phone = await narrow.page.evaluate(() => ({ y: scrollY, cancelled: __wheels.filter((w) => w.cancelled).length }));
    assert(phone.y > 0 && !phone.cancelled, `${name}: a notch on a phone-width page was cancelled or did not scroll: ${JSON.stringify(phone)}`);
    await narrow.context.close();
    console.log(`${name}: no snap and nothing scrolls unasked; notches, nudges, swipes and drags end on a scene past their start; a short drag returns; only a glide is cancelled; keys, End and reduced motion hold; ?snap=off and phone widths scroll freely`);
  } finally {
    await browser.close();
  }
}
