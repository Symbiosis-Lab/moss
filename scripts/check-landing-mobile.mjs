#!/usr/bin/env node

import { readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL, whenReady, installPageOverride } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const base = new URL(baseURL);
const playwright = await loadPlaywright();

const overrideHtml = process.env.LANDING_HTML_STDIN
  ? await new Promise((resolve, reject) => {
      let text = '';
      process.stdin.setEncoding('utf8');
      process.stdin.on('data', (chunk) => { text += chunk; });
      process.stdin.on('end', () => resolve(text));
      process.stdin.on('error', reject);
    })
  : process.env.LANDING_HTML_OVERRIDE
    ? await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8')
    : null;

const browser = await playwright.chromium.launch({ headless: true });
const results = {};
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const installOverride = (page) => installPageOverride(page, base.href, { html: overrideHtml, jsOverridePath: process.env.LANDING_JS_OVERRIDE });
const instrumentScroll = () => {
  window.__landingScrollWrites = [];
  const nativeScrollTo = window.scrollTo.bind(window);
  window.scrollTo = (...args) => { window.__landingScrollWrites.push(args); return nativeScrollTo(...args); };
};

async function mobilePage(search = '', viewport = { width: 390, height: 844 }) {
  const page = await browser.newPage({ viewport, isMobile: true, hasTouch: true });
  await installOverride(page);
  await page.addInitScript(instrumentScroll);
  const url = new URL(base.href); url.search = search;
  await page.goto(url.href, { waitUntil: 'domcontentloaded' });
  await whenReady(page);
  return page;
}
async function swipe(page, fromY, toY, steps = 6) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: 190, y: fromY }] });
  for (let i = 1; i <= steps; i++) {
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: 190, y: fromY + (toY - fromY) * i / steps }] });
    await page.waitForTimeout(30);
  }
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
}
const mobileState = (page) => page.evaluate(() => ({
  y: scrollY,
  max: document.documentElement.scrollHeight - innerHeight,
  progress: window.__landing.state().progress,
  xf: +xfAt().toFixed(3),
  renderedXf: window.__landing.state().xf,
  target: window.__landing.state().target,
  snap: getComputedStyle(document.documentElement).scrollSnapType,
  mobileSnap: document.documentElement.hasAttribute('data-mobile-snap'),
  writes: window.__landingScrollWrites.length,
}));
const openingGeometry = (page) => page.evaluate(() => {
  const rect = (selector) => {
    const value = document.querySelector(selector).getBoundingClientRect();
    return { top: value.top, bottom: value.bottom, height: value.height };
  };
  const titleStyle = getComputedStyle(document.querySelector('#intro h1'));
  const copy = rect('#c1 .scene-text');
  const visual = rect('#vis');
  return { y: scrollY, title: rect('#intro h1'), intro: rect('#intro'), copy, visual,
    gap: visual.top - copy.bottom, opacity: Number(titleStyle.opacity), mask: titleStyle.maskImage,
    webkitMask: titleStyle.webkitMaskImage, writes: window.__landingScrollWrites.length };
});

async function checkOpening(viewport) {
  const page = await mobilePage('', viewport);
  const start = await openingGeometry(page);
  assert(Math.abs(start.intro.bottom - start.copy.top) < 2, `intro and scene 1 are not compact at ${viewport.width}x${viewport.height}: ${JSON.stringify(start)}`);
  assert(start.gap >= 20 && start.gap <= 48, `copy-to-visual gap is not compact at ${viewport.width}x${viewport.height}: ${JSON.stringify(start)}`);
  await swipe(page, Math.min(500, viewport.height - 60), Math.min(470, viewport.height - 90), 6);
  await page.waitForTimeout(120);
  const moved = await openingGeometry(page);
  const distance = moved.y - start.y;
  assert(distance > 8, `opening did not move under native touch at ${viewport.width}x${viewport.height}`);
  assert(Math.abs((start.title.top - moved.title.top) - distance) < 3, `intro title did not scroll with the document at ${viewport.width}x${viewport.height}: ${JSON.stringify({ start, moved })}`);
  assert(moved.opacity === 1 && moved.mask === 'none' && moved.webkitMask === 'none', `intro title faded or masked while scrolling at ${viewport.width}x${viewport.height}: ${JSON.stringify(moved)}`);
  assert(Math.abs(moved.gap - start.gap) < 2, `copy animation changed its gap before the visual pinned at ${viewport.width}x${viewport.height}: ${JSON.stringify({ start, moved })}`);
  assert(moved.visual.top > 86 && moved.visual.top >= moved.copy.bottom + 20, `copy overlaps the visual before pinning at ${viewport.width}x${viewport.height}: ${JSON.stringify(moved)}`);
  assert(moved.writes === 0, `opening geometry used scrollTo at ${viewport.width}x${viewport.height}: ${JSON.stringify(moved)}`);
  await page.close();
  return { viewport, startGap: +start.gap.toFixed(1), travel: +distance.toFixed(1), titleTravel: +(start.title.top - moved.title.top).toFixed(1), movedVisualTop: +moved.visual.top.toFixed(1) };
}

async function checkContinuousMorph() {
  const page = await mobilePage();
  const sample = await page.evaluate(() => WatercolorMorph.frameAt(0.1, 180, {
    A_END: 0.4, B_START: 0.6, ease: 2, wetGainMax: 0.9,
  }, 18));
  assert(sample.f0[0] === sample.f1[0] && sample.f0[1] !== sample.f1[1] && sample.w > 0 && sample.w < 1,
    `outer-phase pigment is still a rounded discrete frame: ${JSON.stringify(sample)}`);
  await page.close();
  return sample;
}

async function checkSceneTiming() {
  const page = await mobilePage();
  // Started here, awaited much further down: the print-warming this waits
  // for is not gated on scroll position, so there is no reason to delay it
  // behind everything this function does before it first needs a print --
  // the earlier request for the whole entrance-ramp shape below cost this
  // wait several real seconds of wheel/settle round trips it used to not
  // have to sit behind. .catch() here, not left to reject on its own: an
  // unrelated assertion failing anywhere above the real `await printsReady`
  // closes the page (the outer finally), which then rejects this dangling
  // promise too -- unhandled, that raced the real error to the console and
  // printed "Target page ... has been closed" in its place (found live).
  // Turning the rejection into a resolved Error value defers surfacing it
  // to the real await, without ever leaving it unhandled in between.
  const printsReady = page.waitForFunction(() => [0, 1, 2, 3].every(i => window.__landing.prints[i]), null, { timeout: 30000 }).catch((e) => e);
  const read = () => page.evaluate(() => {
    const text = document.querySelector('#c2 .scene-text').getBoundingClientRect();
    const band = mobileVisualBand();
    return { text: { top: text.top, bottom: text.bottom }, band, progress: progressAt(), state: window.__landing.state(), writes: window.__landingScrollWrites.length };
  });
  const wheelTextTopTo = async (desired) => {
    const delta = await page.evaluate((value) => document.querySelector('#c2 .scene-text').getBoundingClientRect().top - value, desired);
    await page.mouse.wheel(0, delta);
    await page.waitForTimeout(250);
    return read();
  };
  const wheelIncomingTopTo = async (selector, desired) => {
    const delta = await page.evaluate(({ selector, value }) => document.querySelector(selector).getBoundingClientRect().top - value, { selector, value: desired });
    await page.mouse.wheel(0, delta);
    await page.waitForTimeout(300);
    return read();
  };
  const band = await page.evaluate(() => mobileVisualBand());
  const textHeight = await page.evaluate(() => document.querySelector('#c2 .scene-text').getBoundingClientRect().height);
  const moveTo = async y => {
    await page.mouse.wheel(0, y - await page.evaluate(() => scrollY));
    await page.waitForTimeout(300);
    return read();
  };
  // Owner (2026-09-24): "morph from scene 1 to scene 2 should start a bit
  // later, once text of scene 2 touches the animation." Replaces the old
  // #vis-pin-gated assertions (below this comment used to read progress off
  // the pin point, which finished the leg while #c2's text was still
  // hundreds of px short of the visual) -- mobileEntranceProgress
  // (site/landing.js) now starts 72px before #c2's own text reaches the
  // visual, then ramps over MOBILE_LEG0_RAMP_SPAN (160px). This begins the
  // visual change earlier rather than obtaining a longer dissolve by moving
  // its end later. No 2px scroll step raises progress by
  // more than 0.05; and progress still reaches 1 well (>=40px, the owner's
  // own margin for the leg that follows) before scene 2's text reaches its
  // old resting line, leaving room for leg 1->2's own gate on the same text.
  const touchY = await page.evaluate((band) => {
    const keep = scrollY;
    let y = null;
    const max = document.documentElement.scrollHeight - innerHeight;
    for (let probe = 0; probe <= max; probe++) {
      scrollTo(0, probe);
      if (document.querySelector('#c2 .scene-text').getBoundingClientRect().top <= band.bottom) { y = probe; break; }
    }
    scrollTo(0, keep);
    return y;
  }, band);
  assert(touchY != null, 'scene 2\'s text never reached the visual band across the sampled scroll range');
  const beforeStart = await moveTo(touchY - 80);
  assert(beforeStart.progress <= 0.01, `progress advanced before scene 2's approach band: ${JSON.stringify(beforeStart)}`);
  const grid = await page.evaluate(({ from, to, step }) => {
    const keep = scrollY;
    const rows = [];
    for (let y = from; y <= to; y += step) { scrollTo(0, y); rows.push([y, +progressAt().toFixed(4)]); }
    scrollTo(0, keep);
    return rows;
  }, { from: touchY - 72, to: touchY + 100, step: 2 });
  // touchY-72 is the first whole scroll pixel at which the text's (subpixel)
  // rect.top has crossed the approach band, so it can sit a fraction of a px past
  // the true zero crossing -- <=0.01 (not ===0) tolerates that rounding the
  // same way backAtTouch below does, without hiding a real step.
  assert(grid[0][1] <= 0.01, `progress is not ~0 at the touch point: ${JSON.stringify(grid[0])}`);
  const reachesOne = grid.find((row) => row[1] >= 1);
  assert(reachesOne, `progress never reached 1 across the sampled grid (tail ${JSON.stringify(grid.slice(-5))})`);
  // The step check covers leg 0->1's own ramp (progress<1) only: the single
  // step that crosses into 1 hands off to leg 1->2's own gate on the same
  // #c2 text (mobileInkProgress, unchanged by this fix), which by
  // reachesOne's own margin assertion below is still early in its ramp
  // there -- a leg boundary, not a step inside this leg's ramp.
  const within = grid.filter((row) => row[1] < 1);
  let worst = 0, worstAt = null;
  for (let i = 1; i < within.length; i++) {
    const d = within[i][1] - within[i - 1][1];
    if (d > worst) { worst = d; worstAt = within[i][0]; }
  }
  assert(worst <= 0.05, `a 2px scroll step raises progress by ${worst.toFixed(3)} at y=${worstAt} -- a swap, not a ramp`);
  let worstRev = 0, worstRevAt = null;
  for (let i = within.length - 2; i >= 0; i--) {
    const d = within[i][1] - within[i + 1][1];
    if (d > worstRev) { worstRev = d; worstRevAt = within[i][0]; }
  }
  assert(worstRev <= 0.05, `progress is non-monotonic enough to step ${worstRev.toFixed(3)} scrolling upward at y=${worstRevAt}`);
  const oldRestTop = band.top - textHeight;
  // rect.top moves ~1:1 with scroll, so the scrollY at which #c2's text
  // would cross oldRestTop is the touch scrollY plus the rect.top distance
  // still to cover from the touch point.
  const restScrollY = touchY + (band.bottom - oldRestTop);
  assert(restScrollY - reachesOne[0] >= 40, `progress only reaches 1 ${(restScrollY - reachesOne[0]).toFixed(1)}px before scene 2's text rests (want >=40)`);
  // The grid above calls the instrumented scrollTo (instrumentScroll,
  // above) several hundred times to sample it; window.__landingScrollWrites
  // counts from page load, not from whenever a caller starts watching it,
  // so left alone this makes every writes===0 assertion from here on fail
  // permanently -- not because anything wrote during ITS OWN window, but
  // because the grid already wrote hundreds of times during a window
  // nobody downstream meant to include.
  await page.evaluate(() => { window.__landingScrollWrites.length = 0; });
  // A real touch drive over a few px, not just the static grid above:
  // confirms a genuine wheel gesture also ramps rather than steps, and
  // reverses, right at the touch point.
  const justPastTouch = await moveTo(touchY - 70);
  assert(justPastTouch.progress > 0 && justPastTouch.progress <= 0.05, `a 2px real scroll past the touch point reads ${justPastTouch.progress} -- not a ramp`);
  const backAtTouch = await moveTo(touchY - 74);
  assert(backAtTouch.progress <= 0.01, `reversing 4px back across the touch point did not return progress to ~0: ${JSON.stringify(backAtTouch)}`);
  // Just past where progress first reaches 1 (my own ramp finishing, q not
  // yet under way): targetAt() rounds progress to the nearest scene, so
  // shown settles at 1 anywhere in [0.5, 1.5) -- reachesOne[0] sits at the
  // low end of that window.
  await moveTo(reachesOne[0] + 2);
  await page.waitForFunction(() => window.__landing.state().shown === 1 && !window.__landing.state().running, null, { timeout: 10000 });
  const printsResult = await printsReady;
  if (printsResult instanceof Error) throw printsResult;
  const nextTextHeight = textHeight;   // scroll-independent; already measured above
  // K (owner item 3b): "scene 2 finishes consolidation a little bit before
  // scene 2 text gets into position" -- mobileInkProgress's own denominator
  // is shortened by 40px (site/landing.js, the earlyBy parameter) for this
  // leg, so q===1 lands 40px of scroll before the text is fully clear of
  // the visual (the old 100% point, text.bottom===band.top). This is leg
  // 1->2's own (unchanged) gate, on the same #c2 text that also gates leg
  // 0->1 above -- both count from the same band.bottom crossing, so testing
  // it continues straight on from reachesOne[0] rather than re-touching first.
  const consolidatedEarly = await wheelIncomingTopTo('#c2 .scene-text', oldRestTop + 40);
  assert(consolidatedEarly.progress >= 1.999, `scene 2 did not consolidate: ${JSON.stringify(consolidatedEarly)}`);
  assert(consolidatedEarly.text.top - oldRestTop >= 39.5, `text is not still ~40px below its old resting line at consolidation: ${JSON.stringify(consolidatedEarly)}`);
  const stillEarly = await wheelIncomingTopTo('#c2 .scene-text', oldRestTop + 48);
  assert(stillEarly.progress < 2, `consolidation reached 1 more than 40px early: ${JSON.stringify(stillEarly)}`);
  // showMobileScene's own deadband (renderMorphAt, site/landing.js) latches
  // shown to whichever side of the leg it last crossed 0.55/0.45 toward;
  // stillEarly sits past 0.55 so shown has already latched to scene 2 (was
  // #c3's job under the old contact-at-band.bottom+8 step here, which sat
  // before the touch point this leg now starts from and so no longer means
  // "reset" -- this is the same reset, just after leg 0->1's own ramp
  // instead of before the touch point). band.bottom-170: past leg 0->1's own
  // MOBILE_LEG0_RAMP_SPAN (160px), so this lands in leg 1->2's own range
  // (progress a little over 1, p a little over 0.088), comfortably under
  // the 0.45 deadband -- band.bottom-20 would still be inside leg 0->1's
  // own ramp and read shown===0, not the reset to 1 this needs.
  await wheelIncomingTopTo('#c2 .scene-text', band.bottom - 170);
  // -40: mirrors mobileInkProgress's own shortened denominator for this leg.
  const nextHalfTop = band.bottom - (band.height + nextTextHeight - 40) / 2;
  let half = await wheelIncomingTopTo('#c2 .scene-text', nextHalfTop);
  // renderMorphAt (site/landing.js) catches the pigment sim up to a jump
  // over several of its own step-budgeted frames (advanceWash), not in one;
  // wheelIncomingTopTo's fixed 300ms is generous against a real device's
  // continuous compositor but not against this harness's own frame pacing,
  // so give it a bounded extra window rather than asserting on a washT
  // caught mid-catch-up. Tolerance and target are unchanged -- this only
  // waits long enough to observe them.
  if (half.state.running) {
    await page.waitForFunction(() => !window.__landing.state().running, null, { timeout: 5000 });
    half = await read();
  }
  assert(half.progress > 1.4 && half.progress < 1.6 && Math.abs(half.state.washT - 1.05) < .05 && half.state.shown === 1,
    `scene 2 wash did not follow next incoming text: ${JSON.stringify({ half })}`);
  await page.waitForTimeout(600);
  const paused = await read();
  assert(Math.abs(paused.state.washT - half.state.washT) < .01 && Math.abs(paused.state.progress - half.state.progress) < .001 && paused.writes === 0,
    `scene 2 wash advanced without scroll input: ${JSON.stringify({ half, paused })}`);
  // band.bottom-170, not +8: that sits safely inside leg 1->2's own
  // ramp because the old pin-gated leg 0->1 had already finished tens of px
  // earlier, well before the touch point. leg 0->1 now occupies band.bottom
  // down to band.bottom-MOBILE_LEG0_RAMP_SPAN (160px) itself, so anything in
  // that span (e.g. -4, or -20 as the reset step above found) reverses into
  // leg 0->1's own territory instead of testing leg 1->2's reversal; -170
  // clears that span.
  const reversed = await wheelIncomingTopTo('#c2 .scene-text', band.bottom - 170);
  assert(reversed.progress < half.progress && reversed.state.washT < half.state.washT - .02 && reversed.writes === 0,
    `scene 2 wash did not reverse with upward scroll: ${JSON.stringify({ half, reversed })}`);
  await page.close();
  return { touchY, entranceSpanPx: reachesOne[0] - touchY, marginBeforeRestPx: +(restScrollY - reachesOne[0]).toFixed(1), consolidatedEarly: +consolidatedEarly.progress.toFixed(3),
    half: +half.progress.toFixed(3), halfWashT: half.state.washT, pausedWashT: paused.state.washT,
    reversed: +reversed.progress.toFixed(3), reversedWashT: reversed.state.washT, clearedShown: reversed.state.shown };
}

const moveTextTo = (page, selector, desired) => (async () => {
  const delta = await page.evaluate(({ selector, value }) => document.querySelector(selector).getBoundingClientRect().top - value, { selector, value: desired });
  await page.mouse.wheel(0, delta);
  await page.waitForTimeout(250);
  return page.evaluate((selector) => ({
    top: document.querySelector(selector).getBoundingClientRect().top,
    band: mobileVisualBand(), progress: progressAt(), xf: +xfAt().toFixed(4),
  }), selector);
})();

// K (owner item 4): "scene 2 into 3 ... can start dissolving a bit later,
// after the text touched it". This leg already shares mobileInkProgress
// with scene 1->2 (site/landing.js), whose own span is 0 until the text's
// top edge has crossed the visual's bottom edge (band.bottom) -- this locks
// that in for the leg the owner named, in both scroll directions, rather
// than leaving it proven only by the scene 1->2 leg's own tests.
async function checkScene2to3Touch() {
  const page = await mobilePage();
  const untouched = await moveTextTo(page, '#c3 .scene-text', 435);
  assert(Math.abs(untouched.progress - 2) < 1e-6, `scene 2->3 advanced before #c3's text touched the visual: ${JSON.stringify(untouched)}`);
  const touched = await moveTextTo(page, '#c3 .scene-text', 419);
  assert(touched.progress > 2, `scene 2->3 did not start once #c3's text crossed band.bottom: ${JSON.stringify(touched)}`);
  const back = await moveTextTo(page, '#c3 .scene-text', 435);
  assert(Math.abs(back.progress - 2) < 1e-6, `scrolling #c3's text back below band.bottom did not undo scene 2->3: ${JSON.stringify(back)}`);
  await page.close();
  console.log('scene 2->3 stays put until #c3\'s text touches the visual, both directions');
}

// K (owner item 5b): "when scene 4 transitions to scene 5, start a bit
// later, only start when the text already touches the animation." xfAt()'s
// mobile branch only ever answers above 0 once mobileClosingProgress() --
// itself 0 for as long as #c4's text sits below band.bottom, the same gate
// as every other leg -- has passed 0.45, so the crossfade cannot start
// before that text has touched the visual; this asserts the necessary
// consequence (xf===0 while untouched) rather than re-deriving 0.45.
async function checkClosingTouch() {
  const page = await mobilePage();
  const untouched = await moveTextTo(page, '#c4 .scene-text', 435);
  assert(untouched.xf === 0, `closing crossfade started before #c4's text touched the visual: ${JSON.stringify(untouched)}`);
  const touched = await moveTextTo(page, '#c4 .scene-text', 419);
  assert(touched.xf === 0, `closing crossfade started right at #c4's text touching the visual, before the owner's own margin: ${JSON.stringify(touched)}`);
  const back = await moveTextTo(page, '#c4 .scene-text', 435);
  assert(back.xf === 0, `scrolling #c4's text back did not keep the closing crossfade at 0: ${JSON.stringify(back)}`);
  await page.close();
  console.log('the closing crossfade stays at 0 until #c4\'s text touches the visual, both directions');
}

try {
  results.opening = [];
  for (const viewport of [{ width: 390, height: 844 }, { width: 320, height: 568 }]) results.opening.push(await checkOpening(viewport));
  results.continuousMorph = await checkContinuousMorph();
  results.sceneTiming = await checkSceneTiming();
  await checkScene2to3Touch();
  await checkClosingTouch();
  const page = await mobilePage();
  let state = await mobileState(page);
  assert(state.snap === 'none' && !state.mobileSnap, `mobile starts with snap enabled: ${JSON.stringify(state)}`);
  const affordance = await page.evaluate(() => {
    const visual = document.getElementById('vis');
    const link = document.querySelector('#c1 .btns a');
    window.__copyClicked = false;
    document.addEventListener('click', (event) => {
      if (event.target.closest('#c1 .btns a')) window.__copyClicked = true;
      event.preventDefault(); event.stopImmediatePropagation();
    }, { capture: true, once: true });
    return { visualInert: visual.inert, stagePointer: getComputedStyle(document.getElementById('stage')).pointerEvents,
      linkPointer: getComputedStyle(link).pointerEvents };
  });
  assert(affordance.visualInert && affordance.stagePointer === 'none', `mobile stage remains interactive: ${JSON.stringify(affordance)}`);
  assert(affordance.linkPointer !== 'none', 'mobile copy link is not hit-testable');
  await page.locator('#c1 .btns a').first().click();
  assert(await page.evaluate(() => window.__copyClicked), 'mobile copy link did not receive a click');

  await swipe(page, 720, 650, 10);
  await page.waitForTimeout(250);
  state = await mobileState(page);
  const releasedY = state.y;
  await page.waitForTimeout(700);
  const held = await mobileState(page);
  assert(held.snap === 'none' && !held.mobileSnap, `touch armed mobile snapping: ${JSON.stringify(held)}`);
  assert(held.writes === 0, `mobile code called scrollTo ${held.writes} time(s)`);
  assert(Math.abs(held.y - releasedY) < 2, `page moved after native release: ${releasedY} -> ${held.y}`);

  for (let i = 0; i < 50 && state.xf <= .01; i++) {
    await swipe(page, 650, 520);
    await page.waitForTimeout(35);
    state = await mobileState(page);
  }
  for (let i = 0; i < 12 && state.xf >= .9; i++) {
    await swipe(page, 450, 560);
    await page.waitForTimeout(50);
    state = await mobileState(page);
  }
  assert(state.xf > .01 && state.xf < .9, `did not reach a partial closing scrub: ${JSON.stringify(state)}`);
  const partial = state;
  const partialHeld = await mobileState(page);
  assert(partialHeld.writes === 0, `closing issued scrollTo: ${JSON.stringify(partialHeld)}`);
  await swipe(page, 590, 450);
  await page.waitForTimeout(180);
  const forward = await mobileState(page);
  assert(forward.y > partialHeld.y && forward.xf > partialHeld.xf, `closing did not advance with touch scroll: ${JSON.stringify({ partialHeld, forward })}`);
  await swipe(page, 450, 590);
  await page.waitForTimeout(180);
  const reverse = await mobileState(page);
  assert(reverse.y < forward.y && reverse.xf < forward.xf, `closing did not reverse with touch scroll: ${JSON.stringify({ forward, reverse })}`);
  await page.waitForTimeout(500);
  const reverseHeld = await mobileState(page);
  assert(reverseHeld.writes === 0, `reverse release issued scrollTo: ${JSON.stringify({ reverse, reverseHeld })}`);
  results.mobile = { releasedY, partial, forward, reverse, writes: reverseHeld.writes };
  await page.close();

  const desktop = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await installOverride(desktop);
  await desktop.goto(base.href, { waitUntil: 'domcontentloaded' });
  await whenReady(desktop);
  await desktop.mouse.wheel(0, 120);
  await desktop.waitForTimeout(1200);
  const desktopState = await desktop.evaluate(() => ({ y: scrollY, rest: window.__landing.restY(0) }));
  assert(Math.abs(desktopState.y - desktopState.rest) < 3, `desktop drive did not settle at rest: ${JSON.stringify(desktopState)}`);
  results.desktop = desktopState;
  await desktop.close();
} finally {
  await browser.close();
  await close();
}

console.log(JSON.stringify({ baseUrl: base.href, ...results }, null, 2));
