#!/usr/bin/env node
// Item D: while a mobile wash covers the composition, each word of a
// scene's own copy (h2, .lede -- never a button, never #intro's header)
// must read at or above its own WCAG contrast floor against the pixels
// actually behind it (3:1 for a heading word, 4.5:1 for body), and must
// not repeatedly switch colour while its previous colour remains readable.
// Crossing distinct dark and light patches can require multiple switches. Driven by real CDP touch events (Input.dispatchTouchEvent,
// the same technique check-landing-mobile.mjs's swipe() uses), not
// scrollTo: the point is proving the real input path, not just progressAt().
import { readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL, whenReady, PRESETS, installPageOverride } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const playwright = await loadPlaywright();
const assert = (condition, message) => { if (!condition) throw new Error(message); };

const engine = process.env.ENGINE || 'chromium';
const browser = await playwright[engine].launch({ headless: true });
const page = await browser.newPage(PRESETS.phone);
// Pause only while measuring one painted frame. The drag and application
// run normally between samples; screenshots cannot race later wash paints.
await page.addInitScript(() => {
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (callback) => {
    const frame = (t) => window.__contrastCapture ? raf(frame) : callback(t);
    return raf(frame);
  };
});
await installPageOverride(page, baseURL, {
  html: process.env.LANDING_HTML_OVERRIDE ? await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8') : undefined,
  jsOverridePath: process.env.LANDING_JS_OVERRIDE,
});
await page.goto(baseURL, { waitUntil: 'domcontentloaded' });
await whenReady(page);
await page.evaluate(() => { window.__landing.still(0); scrollTo(0, 0); });
await page.waitForTimeout(500);

// One continuous drag per direction (a real finger never lifts mid-leg): a
// fresh touchStart/touchMove/touchEnd per sampled step, tried first, moved
// so little on a leg as short as 64px (MOBILE_LEG0_RAMP_SPAN) that Chromium
// never registered it as a scroll at all (measured live: scrollY never
// left its start). drag() opens the gesture once and leaves it open for
// moveTo() to move within; end() closes it once, after the last sample.
function makeDrag(x = 190, y0 = 420) {
  // Playwright exposes neither swipe nor wheel on mobile WebKit. Its pass
  // verifies compositing through native scroll positions; Chromium covers touch.
  if (engine === 'webkit') return {
    async start() {},
    async moveBy(dy) { await page.evaluate(d => scrollBy(0, -d), dy); await page.waitForTimeout(16); },
    async end() { await page.waitForTimeout(30); },
  };
  let cdp = null, y = y0;
  return {
    async start() { cdp = await page.context().newCDPSession(page); await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] }); },
    async moveBy(dy) { y += dy; await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y }] }); await page.waitForTimeout(16); },
    async end() { await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] }); await page.waitForTimeout(30); },
  };
}

// Read geometry and colour together, then hold that frame while capturing
// its background. A viewport screenshot has an integer pixel origin;
// fractional canvas clips otherwise shift the word crops by a pixel.
const sampleFrame = async () => {
  const words = await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => {
    window.__contrastCapture = true;
    const cv = document.getElementById('gl').getBoundingClientRect();
    const rb = window.__landing.readback?.();
    const sampled = [...document.querySelectorAll('.word')].map((w, id) => {
      const r = w.getBoundingClientRect();
      if (r.right <= cv.left || r.left >= cv.right || r.bottom <= cv.top || r.top >= cv.bottom) return null;
      // Diagnostic only: the assertion below uses independent screenshot pixels.
      const rbLbg = rb?.pixels ? bgLuminanceUnder(r, cv, rb) : null;
      return { id, text: w.textContent, left: r.left, top: r.top, right: r.right, bottom: r.bottom,
        color: getComputedStyle(w).color, required: w.closest('h2') ? 3 : 4.5,
        inked: rbLbg != null, rbLbg };
    }).filter(Boolean);
    // Visibility preserves layout while removing each glyph from its own
    // background measurement.
    for (const w of document.querySelectorAll('.word')) w.style.visibility = 'hidden';
    resolve(sampled);
  })));
  try {
    return { shot: await page.screenshot(), words };
  } finally {
    await page.evaluate(() => {
      for (const w of document.querySelectorAll('.word')) w.style.visibility = '';
      window.__contrastCapture = false;
    });
  }
};

// Decode and measure in the browser instead of serializing every RGBA byte
// across the Playwright connection for every sampled frame.
async function measureFrame(frame) {
  return page.evaluate(async ({ b64, words }) => {
    const img = new Image();
    const loaded = new Promise((res, rej) => { img.onload = res; img.onerror = rej; });
    img.src = 'data:image/png;base64,' + b64;
    await loaded;
    const c = document.createElement('canvas'); c.width = img.naturalWidth; c.height = img.naturalHeight;
    const g = c.getContext('2d'); g.drawImage(img, 0, 0);
    const pixels = g.getImageData(0, 0, c.width, c.height).data;
    const linear = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
    const luminance = (r, g, b) => 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    return words.filter(w => w.inked).map(w => {
      const x0 = Math.max(0, Math.floor(w.left)), x1 = Math.min(c.width, Math.ceil(w.right));
      const y0 = Math.max(0, Math.floor(w.top)), y1 = Math.min(c.height, Math.ceil(w.bottom));
      if (x1 <= x0 || y1 <= y0) return null;
      let sum = 0, area = 0;
      for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
        const i = (y * c.width + x) * 4;
        const weight = (Math.min(x + 1, w.right) - Math.max(x, w.left)) * (Math.min(y + 1, w.bottom) - Math.max(y, w.top));
        sum += weight * luminance(pixels[i], pixels[i + 1], pixels[i + 2]);
        area += weight;
      }
      const lbg = sum / area;
      const [r, g, b] = w.color.match(/[\d.]+/g).map(Number), foreground = luminance(r, g, b);
      const ratio = (Math.max(lbg, foreground) + 0.05) / (Math.min(lbg, foreground) + 0.05);
      return { ...w, lbg, ratio, colourKey: r > 200 && g > 200 && b > 200 ? 'bright' : 'dark' };
    }).filter(Boolean);
  }, { b64: frame.shot.toString('base64'), words: frame.words });
}

// One leg, one direction: swipes through it in small real touch steps,
// sampling a frame every few, checking each overlapping word's contrast
// and counting its own colour changes.
function foregroundOf(color) {
  const c = color.match(/[\d.]+/g).map(v => { v = Number(v) / 255; return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; });
  return .2126 * c[0] + .7152 * c[1] + .0722 * c[2];
}
async function checkLeg(legLabel, totalDy, steps) {
  const seen = new Map();   // stable span identity -> { colour, flips }
  const bad = [];
  const drag = makeDrag();
  await drag.start();
  for (let i = 0; i < steps; i++) {
    // finger delta is the scroll delta's own negation (dragging a finger up
    // scrolls the page down, the same convention check-landing-mobile.mjs's
    // swipe() uses); moveBy leaves the gesture open, so a leg only 64px
    // long (MOBILE_LEG0_RAMP_SPAN) still moves as one continuous drag,
    // clearing whatever touch-slop a fresh touchStart/End pair per step
    // would have re-triggered every time.
    await drag.moveBy(-totalDy / steps);
    const frame = await sampleFrame();
    if (!frame || !frame.words.length) continue;
    for (const w of await measureFrame(frame)) {
      const { ratio, lbg, colourKey } = w;
      if (ratio < w.required) bad.push({ leg: legLabel, step: i, text: w.text, required: w.required, ratio: +ratio.toFixed(2), color: w.color, screenshotLbg: +lbg.toFixed(3), productionLbg: +w.rbLbg.toFixed(3) });
      const key = w.text + '@' + w.id;
      const prior = seen.get(key);
      if (!prior) seen.set(key, { colour: colourKey, flips: 0, avoidable: 0, maxAvoidable: 0, foreground: foregroundOf(w.color) });
      else if (prior.colour !== colourKey) {
        const oldRatio = (Math.max(lbg, prior.foreground) + .05) / (Math.min(lbg, prior.foreground) + .05);
        prior.avoidable = oldRatio >= w.required ? prior.avoidable + 1 : 0;
        prior.maxAvoidable = Math.max(prior.maxAvoidable, prior.avoidable);
        prior.colour = colourKey; prior.flips++; prior.foreground = foregroundOf(w.color);
      }
    }
  }
  await drag.end();
  const flicker = [...seen.entries()].filter(([, v]) => v.maxAvoidable > 2).map(([k, v]) => ({ word: k, flips: v.flips }));
  return { bad, flicker, wordsSeen: seen.size };
}

const results = {};
try {
  // Legs 0-1 and 1-2 drive through the same visual band the owner's own
  // report was about; 2-3 and 3-4 are covered too (the carry leg's own
  // control text stays outside .scene-text, so this leg mostly exercises
  // "no overlapping words" rather than a flip, which is still worth
  // asserting -- bad/flicker both stay empty when nothing overlaps).
  // {start, end} scrollY per leg -- scrollTo here is positioning to find
  // where a leg lives, not the input under test; the touch swipes inside
  // checkLeg are what's actually asserted against.
  const legBounds = await page.evaluate(() => {
    const max = document.documentElement.scrollHeight - innerHeight;
    const out = [];
    const keep = scrollY;
    for (let leg = 0; leg < 4; leg++) {
      let start = null, end = null;
      for (let y = 0; y <= max; y += 4) {
        scrollTo(0, y);
        const p = progressAt();
        if (start == null && p > leg && p < leg + 1) start = y;
        if (start != null && p >= leg + 1) { end = y; break; }
      }
      out.push(end != null && start != null ? { start, end } : null);
    }
    scrollTo(0, keep);
    return out;
  });
  // A completed recording halfway through a leg still owns the display.
  // Exercise the actual idle capture entry, including a missing neighbour.
  const middle = legBounds[1];
  assert(middle, 'missing preview-to-creations transition');
  await page.evaluate(y => scrollTo(0, y), (middle.start + middle.end) / 2);
  await page.waitForFunction(() => mob.from === 1 && mob.pr && mob.settled && mob.lastCover === 1 && !retaking);
  results.idleOwnership = await page.evaluate(async () => {
    const snapshot = () => ({ scene: stage.dataset.scene, pixels: canvas.toDataURL(), colours: [...document.querySelectorAll('.word')].map(w => w.style.color).join('|') });
    const saved = sheets[DEPLOY], before = snapshot();
    sheets[DEPLOY] = null;
    try {
      await takeOthers([DEPLOY]);
      const after = snapshot();
      return { scene: before.scene === after.scene, pixels: before.pixels === after.pixels, colours: before.colours === after.colours };
    } finally { sheets[DEPLOY] = saved; }
  });
  assert(Object.values(results.idleOwnership).every(Boolean), `idle capture replaced an active wash: ${JSON.stringify(results.idleOwnership)}`);
  results.coldCut = await page.evaluate(() => {
    const saved = sheets[DEPLOY];
    sheets[DEPLOY] = null;
    try {
      renderMorphAt(SHIPS + 0.25);
      return { canvasHidden: getComputedStyle(canvas).display === 'none', wordsReset: [...document.querySelectorAll('.word')].every(w => !w.style.color), correctScene: shown === SHIPS, noPaintedPosition: mob.p === -1, warmingAllowed: !running() };
    } finally { sheets[DEPLOY] = saved; }
  });
  assert(Object.values(results.coldCut).every(Boolean), `missing-print cut retained the previous wash: ${JSON.stringify(results.coldCut)}`);
  for (let leg = 0; leg < 4; leg++) {
    const bounds = legBounds[leg];
    const dy = bounds ? bounds.end - bounds.start : 0;
    if (!bounds || dy < 20) { results[`${leg}-${leg + 1}`] = { skipped: 'leg too short to sample meaningfully' }; continue; }
    const steps = Math.max(6, Math.min(24, Math.round(dy / 15)));
    await page.evaluate((y) => scrollTo(0, y), bounds.start);
    await page.waitForTimeout(200);
    const down = await checkLeg(`${leg}->${leg + 1} down`, dy, steps);
    await page.evaluate((y) => scrollTo(0, y), bounds.end);
    await page.waitForTimeout(200);
    const up = await checkLeg(`${leg}->${leg + 1} up`, -dy, steps);
    results[`${leg}-${leg + 1}`] = { down, up };
    for (const dir of [down, up]) {
      assert(dir.bad.length === 0, `leg ${leg}->${leg + 1}: contrast below floor: ${JSON.stringify(dir.bad.slice(0, 5))}`);
      assert(dir.flicker.length === 0, `leg ${leg}->${leg + 1}: a word repeatedly changed colour while its previous colour remained readable: ${JSON.stringify(dir.flicker.slice(0, 5))}`);
    }
  }
  // A leg with no inked words anywhere passes its own bad/flicker checks
  // vacuously (nothing was ever asserted against) -- this is what an
  // ablation that stops words from ever being wrapped or read would look
  // like from inside the loop above alone, so it gets its own assertion.
  const totalSeen = Object.values(results).reduce((s, r) => s + (r.down?.wordsSeen ?? 0) + (r.up?.wordsSeen ?? 0), 0);
  assert(totalSeen > 0, 'no word was ever sampled over inked canvas in any leg, either direction -- the checks above passed with nothing to check');
} finally {
  await browser.close();
  await close();
}

console.log(JSON.stringify({ baseUrl: baseURL, results }, null, 2));
