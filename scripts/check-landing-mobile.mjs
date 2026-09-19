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
  max: document.documentElement.scrollHeight - innerHeight,
  progress: __state().progress,
  xf: +xfAt().toFixed(3),
  renderedXf: __state().xf,
  target: __state().target,
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

async function checkSceneTiming() {
  const page = await mobilePage();
  const read = () => page.evaluate(() => {
    const text = document.querySelector('#c2 .scene-text').getBoundingClientRect();
    const band = mobileVisualBand();
    return { text: { top: text.top, bottom: text.bottom }, band, progress: progressAt(), state: __state(), writes: window.__landingScrollWrites.length };
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
  const entranceStart = await page.evaluate(() => {
    const col = document.getElementById('col');
    return scrollY + col.getBoundingClientRect().top + parseFloat(getComputedStyle(col).paddingTop) + mobileVisualBand().height - innerHeight;
  });
  const entranceEnd = await page.evaluate(() => scrollY + document.querySelector('#c2 .scene-text').getBoundingClientRect().top - innerHeight);
  const moveTo = async y => {
    await page.mouse.wheel(0, y - await page.evaluate(() => scrollY));
    await page.waitForTimeout(300);
    return read();
  };
  const entering = await moveTo(entranceStart - 8);
  assert(entering.progress === 0, `first visual morphed before fully entering: ${JSON.stringify(entering)}`);
  const entryHalf = await moveTo((entranceStart + entranceEnd) / 2);
  assert(entryHalf.progress > .4 && entryHalf.progress < .6, `first morph did not follow entrance: ${JSON.stringify(entryHalf)}`);
  const pinned = await moveTo(entranceEnd + 2);
  await page.waitForFunction(() => __state().shown === 1 && !__state().running, null, { timeout: 10000 });
  assert(pinned.progress === 1, `scene 2 was not consolidated when its copy entered: ${JSON.stringify(pinned)}`);
  await page.waitForFunction(() => [0, 1, 2, 3].every(i => __onHand[i]), null, { timeout: 30000 });
  const plateauStart = await page.evaluate(() => scrollY);
  const before = await wheelTextTopTo(band.bottom + 24);
  const plateau = await page.evaluate(() => scrollY) - plateauStart;
  assert(plateau >= 100, `scene 2 solid interval too short: ${plateau}px`);
  assert(before.progress === 1 && before.state.shown === 1, `scene 2 advanced before approaching the visual: ${JSON.stringify(before)}`);
  const nextTextHeight = await page.evaluate(() => document.querySelector('#c2 .scene-text').getBoundingClientRect().height);
  const contact = await wheelIncomingTopTo('#c2 .scene-text', band.bottom + 8);
  assert(contact.progress === 1 && contact.state.shown === 1, `scene 2 advanced before next incoming text contact: ${JSON.stringify(contact)}`);
  const nextHalfTop = band.bottom - (band.height + nextTextHeight) / 2;
  const half = await wheelIncomingTopTo('#c2 .scene-text', nextHalfTop);
  assert(half.progress > 1.4 && half.progress < 1.6 && Math.abs(half.state.washT - 1.05) < .05 && half.state.shown === 1,
    `scene 2 wash did not follow next incoming text: ${JSON.stringify({ contact, half })}`);
  await page.waitForTimeout(600);
  const paused = await read();
  assert(Math.abs(paused.state.washT - half.state.washT) < .01 && Math.abs(paused.state.progress - half.state.progress) < .001 && paused.writes === 0,
    `scene 2 wash advanced without scroll input: ${JSON.stringify({ half, paused })}`);
  const reversed = await wheelIncomingTopTo('#c2 .scene-text', band.bottom + 8);
  assert(reversed.progress < half.progress && reversed.state.washT < half.state.washT - .02 && reversed.writes === 0,
    `scene 2 wash did not reverse with upward scroll: ${JSON.stringify({ half, reversed })}`);
  await page.close();
  return { entryHalf: entryHalf.progress, pinned: pinned.progress, before: +before.progress.toFixed(3), contact: +contact.progress.toFixed(3),
    half: +half.progress.toFixed(3), halfWashT: half.state.washT, pausedWashT: paused.state.washT,
    reversed: +reversed.progress.toFixed(3), reversedWashT: reversed.state.washT, clearedShown: reversed.state.shown };
}

try {
  results.opening = [];
  for (const viewport of [{ width: 390, height: 844 }, { width: 320, height: 568 }]) results.opening.push(await checkOpening(viewport));
  results.sceneTiming = await checkSceneTiming();
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

  const cssPage = await mobilePage('?carry=css');
  await swipe(cssPage, 720, 400);
  await cssPage.waitForTimeout(200);
  const cssState = await mobileState(cssPage);
  assert(cssState.snap === 'none' && !cssState.mobileSnap, `carry=css enabled mobile snapping: ${JSON.stringify(cssState)}`);
  await cssPage.close();
  results.mobileCss = cssState;

  const desktop = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await installOverride(desktop);
  await desktop.goto(base.href, { waitUntil: 'domcontentloaded' });
  await ready(desktop);
  await desktop.mouse.wheel(0, 120);
  await desktop.waitForTimeout(1200);
  const desktopState = await desktop.evaluate(() => ({ y: scrollY, rest: __restY(0), carry: __state().carry }));
  assert(desktopState.carry === 'intent' && Math.abs(desktopState.y - desktopState.rest) < 3, `desktop carry regressed: ${JSON.stringify(desktopState)}`);
  results.desktop = desktopState;
  await desktop.close();
} finally {
  await browser.close();
}

console.log(JSON.stringify({ baseUrl: base.href, ...results }, null, 2));
