#!/usr/bin/env node

import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const input = process.argv[2];
if (!input) {
  console.error('Usage: node scripts/check-landing-mobile.mjs <preview-url>');
  process.exit(2);
}
const base = new URL(input);
const requestedModule = process.env.PLAYWRIGHT_MODULE || 'playwright';
const moduleSpecifier = requestedModule.startsWith('/') ? pathToFileURL(requestedModule).href : requestedModule;
let playwright;
try {
  playwright = await import(moduleSpecifier);
} catch (error) {
  throw new Error(`Could not load Playwright from ${requestedModule}. Set PLAYWRIGHT_MODULE to playwright/index.mjs in an existing install.\n${error}`);
}

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
const installOverride = async (page) => {
  if (!overrideHtml) return;
  await page.route(base.href, (route) => route.fulfill({ status: 200, contentType: 'text/html', body: overrideHtml }));
};
const instrumentScroll = () => {
  window.__landingScrollWrites = [];
  const nativeScrollTo = window.scrollTo.bind(window);
  window.scrollTo = (...args) => { window.__landingScrollWrites.push(args); return nativeScrollTo(...args); };
};
const ready = async (page) => {
  await page.waitForSelector('html[data-ready="1"]', { timeout: 30000 });
  await page.waitForFunction(() => window.__state?.().ready && window.__restY, null, { timeout: 30000 });
};

async function mobilePage(search = '', viewport = { width: 390, height: 844 }) {
  const page = await browser.newPage({ viewport, isMobile: true, hasTouch: true });
  await installOverride(page);
  await page.addInitScript(instrumentScroll);
  const url = new URL(base.href); url.search = search;
  await page.goto(url.href, { waitUntil: 'domcontentloaded' });
  await ready(page);
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
  return { y: scrollY, height: innerHeight, title: rect('#intro h1'), intro: rect('#intro'), copy, visual,
    gap: copy.top - visual.bottom, opacity: Number(titleStyle.opacity), mask: titleStyle.maskImage,
    webkitMask: titleStyle.webkitMaskImage, writes: window.__landingScrollWrites.length };
});

async function checkOpening(viewport) {
  const page = await mobilePage('', viewport);
  await page.waitForFunction(() => [...document.querySelectorAll('.plate img')].every(im => im.complete && im.naturalWidth), null, { timeout: 15000 });
  const plates = await page.evaluate(() => [...document.querySelectorAll('.plate')].map(pl => { const im = pl.querySelector('img'); return +((pl.offsetWidth / pl.offsetHeight) / (im.naturalWidth / im.naturalHeight)).toFixed(3); }));
  assert(plates.length && plates.every(r => Math.abs(r - 1) < .02), `a scene 1 plate does not have its image's shape at ${viewport.width}x${viewport.height}: ${JSON.stringify(plates)}`);
  const start = await openingGeometry(page);
  // The first screen is the title and scene 1's art; its copy follows the art, as every later copy does.
  assert(Math.abs(start.intro.bottom - start.visual.top) < 2, `scene 1's art does not follow the title at ${viewport.width}x${viewport.height}: ${JSON.stringify(start)}`);
  assert(start.visual.bottom <= start.height, `scene 1's art is not on the first screen at ${viewport.width}x${viewport.height}: ${JSON.stringify(start)}`);
  assert(start.gap >= 20 && start.gap <= 48, `visual-to-copy gap is not compact at ${viewport.width}x${viewport.height}: ${JSON.stringify(start)}`);
  await swipe(page, Math.min(500, viewport.height - 60), Math.min(470, viewport.height - 90), 6);
  await page.waitForTimeout(120);
  const moved = await openingGeometry(page);
  const distance = moved.y - start.y;
  assert(distance > 8, `opening did not move under native touch at ${viewport.width}x${viewport.height}`);
  assert(Math.abs((start.title.top - moved.title.top) - distance) < 3, `intro title did not scroll with the document at ${viewport.width}x${viewport.height}: ${JSON.stringify({ start, moved })}`);
  assert(moved.opacity === 1 && moved.mask === 'none' && moved.webkitMask === 'none', `intro title faded or masked while scrolling at ${viewport.width}x${viewport.height}: ${JSON.stringify(moved)}`);
  assert(Math.abs(moved.gap - start.gap) < 2, `copy animation changed its gap before the visual pinned at ${viewport.width}x${viewport.height}: ${JSON.stringify({ start, moved })}`);
  assert(moved.visual.top > 86 && moved.copy.top >= moved.visual.bottom + 20, `copy overlaps the visual before pinning at ${viewport.width}x${viewport.height}: ${JSON.stringify(moved)}`);
  assert(moved.writes === 0, `opening geometry used scrollTo at ${viewport.width}x${viewport.height}: ${JSON.stringify(moved)}`);
  await page.close();
  return { viewport, startGap: +start.gap.toFixed(1), travel: +distance.toFixed(1), titleTravel: +(start.title.top - moved.title.top).toFixed(1), movedVisualTop: +moved.visual.top.toFixed(1) };
}

// The browser scrolls a wheel off the main thread, so a wheel is followed by a wait for the page to
// have moved and come to a stop, not by a fixed pause.
async function wheel(page, delta) {
  const from = await page.evaluate(() => scrollY);
  await page.mouse.wheel(0, delta);
  if (Math.abs(delta) > 1) await page.waitForFunction((y) => Math.abs(scrollY - y) > 1, from, { timeout: 5000 }).catch(() => {});
  for (let last = NaN, y = await page.evaluate(() => scrollY), n = 0; y !== last && n < 40; n++) { last = y; await page.waitForTimeout(100); y = await page.evaluate(() => scrollY); }
}
const wheelTo = async (page, scene) => wheel(page, await page.evaluate((s) => __restY(s) - scrollY, scene));
const settled = (page, scene) => page.waitForFunction((s) => { const st = __state(); return st.shown === s && !st.leg; }, scene, { timeout: 40000 });
const quiet = (page) => page.evaluate(async () => {
  const seen = new Set(), t0 = performance.now(), y = scrollY;
  while (performance.now() - t0 < 1500) { await new Promise(requestAnimationFrame); const s = __state(); seen.add(`${s.shown}/${s.target}/${s.running}`); }
  return { seen: [...seen], moved: scrollY - y, writes: window.__landingScrollWrites.length };
});
// Scrolls until the next scene's copy has crossed `q` of the pinned visual.
async function toInk(page, scene, q) {
  for (let i = 0; i < 4; i++) {
    const delta = await page.evaluate(({ scene, q }) => { const t = scenesEl[scene].firstElementChild; return (q - mobileInkProgress(t)) * (mobileVisualBand().height + t.getBoundingClientRect().height); }, { scene, q });
    if (Math.abs(delta) < 4) break;
    await wheel(page, delta);
  }
  await page.waitForTimeout(250);
}

// The picture chases the scene being read and finishes after the reader stops.
async function checkChase() {
  const page = await mobilePage();
  await wheelTo(page, 1);
  await settled(page, 1);
  await page.waitForFunction(() => [0, 1, 2, 3].every((i) => __onHand[i]), null, { timeout: 40000 });

  const rest = await quiet(page);
  assert(rest.seen.length === 1 && rest.seen[0] === '1/1/false' && rest.moved === 0 && rest.writes === 0, `a resting reader's picture changed or the page wrote the scroll position: ${JSON.stringify(rest)}`);

  // Past the halfway mark the page keeps the scene it is on until it is clearly closer to the next.
  const target = () => page.evaluate(() => __state().target);
  await toInk(page, 1, .55);
  assert(await target() === 1, 'the target turned at the halfway mark');
  await toInk(page, 1, .75);
  assert(await target() === 2, 'the target did not follow the reader past the hysteresis');
  await toInk(page, 1, .45);
  assert(await target() === 2, 'the target turned back at the halfway mark');
  await toInk(page, 1, .3);
  assert(await target() === 1, 'the target did not turn back once the reader was clearly nearer scene 2');
  await settled(page, 1);

  // A flick: the picture is still on its way when the reader's thumb lifts, then arrives without any scroll.
  await wheelTo(page, 2);
  const lagging = await page.evaluate(() => __state());
  const stopped = Date.now();
  assert(lagging.leg && lagging.shown === 1, `the picture was not still travelling when the reader stopped: ${JSON.stringify(lagging)}`);
  await settled(page, 2);
  const arrivedMs = Date.now() - stopped;
  assert(arrivedMs < 4500, `the picture took ${arrivedMs}ms to arrive`);
  const after = await quiet(page);
  assert(after.moved === 0 && after.writes === 0, `arriving moved or wrote the scroll position: ${JSON.stringify(after)}`);

  // Reversing mid-leg plays the picture back; it never reaches the scene the reader turned away from.
  await page.evaluate(() => { window.__log = []; (function tick() { const s = __state(); __log.push([s.shown, s.leg ? s.leg.p : null]); requestAnimationFrame(tick); })(); });
  await page.mouse.wheel(0, await page.evaluate(() => __restY(3) - scrollY));
  await page.waitForFunction(() => __state().leg?.p > .1, null, { timeout: 15000 });
  await wheelTo(page, 2);
  await settled(page, 2);
  const log = await page.evaluate(() => __log);
  const peak = Math.max(...log.map(([, p]) => p ?? 0));
  assert(!log.some(([shown]) => shown === 3), 'the picture arrived at the scene the reader had turned away from');
  assert(log.slice(log.findIndex(([, p]) => p === peak)).some(([, p]) => p != null && p < peak - .05), `the leg did not run back: peak ${peak}`);
  await page.close();
  return { rest: rest.seen, arrivedMs, peak };
}

try {
  results.opening = [];
  for (const viewport of [{ width: 390, height: 844 }, { width: 320, height: 568 }]) results.opening.push(await checkOpening(viewport));
  results.chase = await checkChase();
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

  // A phone scrolls freely: a touch moves the page, the page does not move it after release and does not snap.
  const before = await mobileState(page);
  await swipe(page, 720, 400);
  await page.waitForTimeout(250);
  const released = await mobileState(page);
  assert(released.y > before.y + 20, `a swipe did not scroll the phone layout: ${before.y} -> ${released.y}`);
  await page.waitForTimeout(700);
  const held = await mobileState(page);
  assert(held.snap === 'none' && !held.mobileSnap, `touch armed mobile snapping: ${JSON.stringify(held)}`);
  assert(held.writes === 0, `mobile code called scrollTo ${held.writes} time(s)`);
  assert(Math.abs(held.y - released.y) < 2, `page moved after native release: ${released.y} -> ${held.y}`);
  results.mobile = { releasedY: released.y, writes: held.writes };
  await page.close();

  // A wheel notch on the phone layout belongs to the browser alone.
  const wheelPage = await mobilePage();
  const wheelFrom = await wheelPage.evaluate(() => scrollY);
  await wheel(wheelPage, 100);
  const wheeled = await mobileState(wheelPage);
  assert(wheeled.y > wheelFrom, `a wheel notch did not scroll the phone layout: ${wheelFrom} -> ${wheeled.y}`);
  assert(wheeled.writes === 0, `a wheel notch made the page write the scroll position ${wheeled.writes} time(s) on the phone layout`);
  await wheelPage.close();
} finally {
  await browser.close();
}

console.log(JSON.stringify({ baseUrl: base.href, ...results }, null, 2));
