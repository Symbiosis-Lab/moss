#!/usr/bin/env node
// Desktop scrolling belongs to the browser: it moves and snaps the page, and the
// page only slows the picture. This drives it with input a browser really
// handles (trusted wheel notches and keys) in Chromium and WebKit with the
// pointer in the left margin, clear of every demo frame: nothing scrolls before
// input; a notch rests on the next scene (in Chromium that needs the page's
// rule that carries a short scroll onward, since its snap alone returns a
// single notch to where it started) and the snap is back once it has; a rest
// holds; a notch back returns one scene; two quick notches advance one scene;
// PageDown and PageUp step one scene, also while the editor demo holds focus;
// End rests on the close, ArrowDown goes on to the footer and a notch up from
// there rests on the close; with reduced motion a step lands at once, the snap
// stays and the picture does not trail the page; and the page never cancels a
// wheel event. WebKit only, because headless Chromium draws this page at about
// a frame a second: a swipe (real trackpad timing, fingers then the glide)
// moves one scene and every snap point is back once it rests, however far the
// glide would otherwise carry the page; the wash starts with a notch and lands
// whole about two seconds later; four notches end on scene 4; down-then-up
// ends where it began.
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
const ORDINARY = swipe(12, 4, 30), LARGE = swipe(14, 6, 60);   // about 760px and 1600px in all

async function open(browser, options = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options });
  const page = await context.newPage();
  // counts what the page does to wheel events in the top document, and how many it was offered
  await page.addInitScript(() => {
    if (window !== window.top) return;
    window.__wheelSeen = 0; window.__wheelCancelled = 0;
    addEventListener('wheel', () => { window.__wheelSeen++; }, { capture: true, passive: true });
    const original = Event.prototype.preventDefault;
    Event.prototype.preventDefault = function () { if (this.type === 'wheel') window.__wheelCancelled++; return original.call(this); };
  });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(base);
  await whenReady(page);
  await page.mouse.move(30, 450);
  return { page, context, errors };
}
const restY = (page, scene) => page.evaluate((s) => __restY(s), scene);
// sends a swipe at its own pace: each event leaves when the last was sent, not when it was handled
const sendSwipe = async (page, deltas, sign = 1) => {
  const sent = [];
  for (const delta of deltas) { sent.push(page.mouse.wheel(0, sign * delta)); await sleep(1000 / 60); }
  await Promise.all(sent);
};
// where the page comes to a stop: still for 800ms
const stillAt = async (page) => {
  let last = NaN;
  for (let n = 0, still = 0; n < 200 && still < 8; n++) { await sleep(100); const y = await page.evaluate(() => Math.round(scrollY)); still = y === last ? still + 1 : 0; last = y; }
  return last;
};
const rested = async (page, scene, label) => {
  await page.waitForFunction((s) => Math.abs(scrollY - __restY(s)) <= 1, scene, { timeout: 40000 })
    .catch(async () => { throw new Error(`${label}: the page did not rest on rest ${scene}: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, rests: [-1, 0, 1, 2, 3, 4].map(__restY) })))}`); });
  // a rest that holds: the browser's snap must not hand the page back
  await sleep(450);
  assert(await page.evaluate((s) => Math.abs(scrollY - __restY(s)) <= 1, scene), `${label}: the page rebounded from rest ${scene} to ${await page.evaluate(() => scrollY)}`);
};

for (const [name, type] of [['chromium', chromium], ['webkit', webkit]]) {
  const browser = await type.launch();
  try {
    const { page, context, errors } = await open(browser);
    await sleep(1000);
    assert(await page.evaluate(() => scrollY === 0), `${name}: the page scrolled before any input`);

    await page.mouse.wheel(0, NOTCH);
    await rested(page, 0, `${name}: one notch from the title`);
    // once a carried notch has come to rest the page hands the scroll back to the browser's snap
    const snap = await page.evaluate(() => ({ carried: document.documentElement.classList.contains('carried'), type: getComputedStyle(document.documentElement).scrollSnapType }));
    assert(!snap.carried && snap.type === 'y mandatory', `${name}: after a notch came to rest the snap was not back: ${JSON.stringify(snap)}`);
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

    assert(await page.evaluate(() => __wheelSeen) >= 1, `${name}: the wheel events never reached the top document, so the cancel count below means nothing`);
    assert(await page.evaluate(() => __wheelCancelled) === 0, `${name}: the page cancelled ${await page.evaluate(() => __wheelCancelled)} wheel events`);
    assert(!errors.length, `${name}: page errors: ${errors.join('; ')}`);
    await context.close();

    // two quick notches from a rest advance one scene, not two
    const quick = await open(browser);
    await quick.page.mouse.wheel(0, NOTCH); await sleep(80); await quick.page.mouse.wheel(0, NOTCH);
    await rested(quick.page, 0, `${name}: two quick notches from the title`);
    await sleep(1200);
    assert(await quick.page.evaluate(() => Math.abs(scrollY - __restY(0)) <= 1), `${name}: two quick notches went on past scene 1 to ${await quick.page.evaluate(() => scrollY)}`);

    // End rests on the close; the footer lies a key press below it, and one notch up from there is the close again, not scene 4
    await quick.page.keyboard.press('End');
    await rested(quick.page, 4, `${name}: End`);
    // Chromium only: WebKit's ArrowDown moves one 40px step and stops short of the footer, and a notch up from
    // there came back to the close in one run and went to scene 4 in another
    if (name === 'chromium') {
      await quick.page.keyboard.press('ArrowDown');
      await quick.page.waitForFunction(() => scrollY > __restY(4) + 30, null, { timeout: 40000 })
        .catch(async () => { throw new Error(`${name}: ArrowDown from the close did not reach the footer: y ${await quick.page.evaluate(() => scrollY)}, close ${await restY(quick.page, 4)}`); });
      await sleep(800);
      await quick.page.mouse.wheel(0, -NOTCH);
      await rested(quick.page, 4, `${name}: one notch up from the footer`);
    }
    await quick.context.close();

    // reduced motion: the same snap, and a key step lands at once
    const calm = await open(browser, { reducedMotion: 'reduce' });
    const target = await restY(calm.page, 0);
    await calm.page.evaluate(() => { window.__ys = []; addEventListener('scroll', () => __ys.push(Math.round(scrollY)), { passive: true }); });
    await calm.page.keyboard.press('PageDown');
    await calm.page.waitForFunction((y) => Math.abs(scrollY - y) <= 1, target, { timeout: 40000 });
    await sleep(300);
    const calmSnap = await calm.page.evaluate(() => ({ type: getComputedStyle(document.documentElement).scrollSnapType, y: scrollY, view: __state().view }));
    assert(calmSnap.type === 'y mandatory', `${name}: with reduced motion the snap is ${calmSnap.type}`);
    assert(Math.abs(calmSnap.view - calmSnap.y) < 1, `${name}: with reduced motion the picture still follows behind the page: ${JSON.stringify(calmSnap)}`);
    const seen = await calm.page.evaluate(() => __ys);
    assert(seen.length && seen.every((y) => Math.abs(y - target) <= 1), `${name}: with reduced motion a key step travelled instead of landing at once: ${seen}`);
    await calm.context.close();
    console.log(`${name}: nothing scrolls unasked; notches and keys step one scene and rest without rebound; reduced motion lands at once; no wheel event was cancelled`);

    if (name !== 'webkit') continue;

    // The picture follows the page: from a notch its wash is running within 300ms and the next scene
    // stands whole about two seconds later, without waiting for a page that is already there.
    const one = await open(browser);
    await one.page.keyboard.press('PageDown');
    await rested(one.page, 0, 'webkit: PageDown from the title');
    await one.page.waitForFunction(() => __state().shown === 0 && !__state().running && [0, 1].every((i) => __onHand[i]), null, { timeout: 30000 });
    await sleep(500);
    const t0 = Date.now();
    await one.page.mouse.wheel(0, NOTCH);
    let startedAt = null, wholeAt = null;
    while (Date.now() - t0 < 6000 && wholeAt == null) {
      const s = await one.page.evaluate(() => __state());
      if (startedAt == null && s.running) startedAt = Date.now() - t0;
      if (s.shown === 1 && !s.running) wholeAt = Date.now() - t0;
      await sleep(40);
    }
    assert(startedAt != null && startedAt <= 300, `webkit: the wash started ${startedAt}ms after the notch, not within 300ms`);
    assert(wholeAt != null && wholeAt >= 1400 && wholeAt <= 3500, `webkit: scene 2 was whole ${wholeAt}ms after the notch, not between 1400 and 3500`);
    console.log(`webkit: from a notch the wash starts after ${startedAt}ms and scene 2 is whole after ${wholeAt}ms`);
    await rested(one.page, 1, 'webkit: the notch from scene 1');

    // down and straight back up: the picture ends whole on the scene it started from
    await one.page.mouse.wheel(0, -NOTCH);
    await rested(one.page, 0, 'webkit: the notch back');
    await one.page.waitForFunction(() => __state().shown === 0 && !__state().running, null, { timeout: 8000 })
      .catch(async () => { throw new Error(`webkit: down and up did not end whole on scene 1: ${JSON.stringify(await one.page.evaluate(() => __state()))}`); });
    await one.page.mouse.wheel(0, NOTCH); await sleep(700); await one.page.mouse.wheel(0, -NOTCH);
    await rested(one.page, 0, 'webkit: down, then up 700ms later');
    await one.page.waitForFunction(() => __state().shown === 0 && !__state().running, null, { timeout: 8000 })
      .catch(async () => { throw new Error(`webkit: down then up did not end whole on scene 1: ${JSON.stringify(await one.page.evaluate(() => __state()))}`); });
    await one.context.close();

    // A trackpad swipe moves one scene, however far its glide would carry the page. Native momentum
    // picks the snap point nearest where it would end, and WebKit's passed scroll-snap-stop: an
    // ordinary swipe moved two scenes from every rest and a large one three, and from the title,
    // a third of a screen above scene 1, almost every swipe went past scene 1. The pointer is
    // over the picture, where scene 1's notebook frame took the wheel events unheard. WebKit
    // only, like the timings below: headless Chromium draws this page at about a frame a second,
    // so a swipe's glide outruns every frame the page gets.
    const swiped = await open(browser);
    await swiped.page.mouse.move(480, 450);
    await sleep(500);
    for (const [label, deltas, sign, scene] of [['an ordinary swipe from the title', ORDINARY, 1, 0], ['a large swipe from scene 1', LARGE, 1, 1], ['an ordinary swipe up from scene 2', ORDINARY, -1, 0]]) {
      await sendSwipe(swiped.page, deltas, sign);
      const y = await stillAt(swiped.page), want = await restY(swiped.page, scene);
      assert(Math.abs(y - want) <= 1, `webkit: ${label} came to rest at ${y}, not on scene ${scene + 1} at ${want}`);
      // once the page rests, every snap point is back and the snap is on
      await swiped.page.waitForFunction(() => [...document.querySelectorAll('#intro, .scene, #five, #footer')].every((el) => getComputedStyle(el).scrollSnapAlign !== 'none') && getComputedStyle(document.documentElement).scrollSnapType === 'y mandatory', null, { timeout: 5000 })
        .catch(async () => { throw new Error(`webkit: after ${label} came to rest the snap points were not all back: ${JSON.stringify(await swiped.page.evaluate(() => ({ points: [...document.querySelectorAll('#intro, .scene, #five, #footer')].map((el) => getComputedStyle(el).scrollSnapAlign), type: getComputedStyle(document.documentElement).scrollSnapType })))}`); });
    }
    await swiped.context.close();

    // four notches 600ms apart from the title, the picture whole on scene 4 within 4s of the page resting
    const four = await open(browser);
    // the four prints on hand first, so the 4s bound below times the wash and not the captures
    await four.page.waitForFunction(() => [0, 1, 2, 3].every((i) => __onHand[i]), null, { timeout: 30000 })
      .catch(async () => { throw new Error(`webkit: the prints for scenes 1 to 4 never came on hand: ${JSON.stringify(await four.page.evaluate(() => [0, 1, 2, 3].map((i) => !!__onHand[i])))}`); });
    for (let i = 0; i < 4; i++) { await four.page.mouse.wheel(0, NOTCH); await sleep(600); }
    await rested(four.page, 3, 'webkit: four notches from the title');
    await four.page.waitForFunction(() => __state().shown === 3 && !__state().running, null, { timeout: 4000 })
      .catch(async () => { throw new Error(`webkit: four notches did not end whole on scene 4: ${JSON.stringify(await four.page.evaluate(() => __state()))}`); });
    await four.context.close();
    console.log('webkit: down and up ends whole on the scene it left; four notches end whole on scene 4');
  } finally {
    await browser.close();
  }
}
