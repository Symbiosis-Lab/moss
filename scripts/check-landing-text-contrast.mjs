#!/usr/bin/env node
// Item D: while a mobile wash covers the composition, each word of a
// scene's own copy (h2, .lede -- never a button, never #intro's header)
// must read at or above its own WCAG contrast floor against the pixels
// actually behind it (3:1 for a heading word, 4.5:1 for body), and must
// not change colour more than twice while the reader travels one direction
// through a leg. Driven by real CDP touch events (Input.dispatchTouchEvent,
// the same technique check-landing-mobile.mjs's swipe() uses), not
// scrollTo: the point is proving the real input path, not just progressAt().
import { loadPlaywright, resolveBaseURL, whenReady, PRESETS } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const playwright = await loadPlaywright();
const assert = (condition, message) => { if (!condition) throw new Error(message); };

const browser = await playwright.chromium.launch({ headless: true });
const page = await browser.newPage(PRESETS.phone);
await page.goto(baseURL, { waitUntil: 'domcontentloaded' });
await whenReady(page);
await page.evaluate(() => { window.__landing.still(0); scrollTo(0, 0); });
await page.waitForTimeout(500);
await page.waitForFunction(() => [0, 1, 2, 3].every((i) => window.__landing.prints[i]), null, { timeout: 30000 }).catch(() => {});

// One continuous drag per direction (a real finger never lifts mid-leg): a
// fresh touchStart/touchMove/touchEnd per sampled step, tried first, moved
// so little on a leg as short as 64px (MOBILE_LEG0_RAMP_SPAN) that Chromium
// never registered it as a scroll at all (measured live: scrollY never
// left its start). drag() opens the gesture once and leaves it open for
// moveTo() to move within; end() closes it once, after the last sample.
function makeDrag(x = 190, y0 = 420) {
  let cdp = null, y = y0;
  return {
    async start() { cdp = await page.context().newCDPSession(page); await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] }); },
    async moveBy(dy) { y += dy; await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y }] }); await page.waitForTimeout(16); },
    async end() { await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] }); await page.waitForTimeout(30); },
  };
}

// Every checked frame's ground truth: a screenshot of the canvas itself
// (not a re-derivation from prints or dens -- the point is what a reader's
// eye actually meets), plus each overlapping word's own box and live
// colour. Read together so the pixel crop and the DOM read are of the same
// paint.
const VIEWPORT = { width: 390, height: 844 };
const sampleFrame = async () => {
  const canvasBox = await page.evaluate(() => {
    const r = document.getElementById('gl').getBoundingClientRect();
    return { x: r.left, y: r.top, width: r.width, height: r.height };
  });
  if (canvasBox.width < 2 || canvasBox.height < 2) return null;
  // The canvas can extend past the viewport (measured: x as low as -20 on
  // this layout), but screenshot's own clip cannot -- clamped here, and
  // *this* clamped rect, not canvasBox itself, is what a pixel offset must
  // be read against. Using canvasBox.x/y directly here was the bug this
  // comment replaces: every word's sampled background was off by exactly
  // however far the canvas hung off the left edge, worst for the words
  // closest to it.
  const clip = {
    x: Math.max(0, canvasBox.x), y: Math.max(0, canvasBox.y),
    width: Math.min(canvasBox.x + canvasBox.width, VIEWPORT.width) - Math.max(0, canvasBox.x),
    height: Math.min(canvasBox.y + canvasBox.height, VIEWPORT.height) - Math.max(0, canvasBox.y),
  };
  if (clip.width < 2 || clip.height < 2) return null;
  // The word spans sit visually on top of the canvas, so a plain screenshot
  // of this clip is the wash AND every glyph's own ink blended together --
  // averaging that over a word's whole box (bgLuminanceUnder, below) reads
  // some of the word's own colour back as "background", which for a word
  // already light (mid-fade or freshly flipped) drags the measured
  // background up toward white and understates the true contrast against
  // what is actually behind it. Found live: a lede word 90%+ through its
  // fade read screenshotLbg 0.18-0.24 and failed 4.5:1 at ratio ~3.6, while
  // the same pixels' own wash-only reading (rbLbg, sampled off the GPU
  // readback the same word carries) was 0.11 -- against which its displayed
  // colour clears 4.5:1 at a real ratio of 5.5. Hiding every word for this
  // one screenshot (visibility, not display, so layout and the canvas
  // beneath are untouched) is what makes the crop actually "the pixels
  // behind it" rather than a mix of that and the text asking the question.
  await page.evaluate(() => { for (const w of document.querySelectorAll('.word')) w.style.visibility = 'hidden'; });
  const shot = await page.screenshot({ clip });
  await page.evaluate(() => { for (const w of document.querySelectorAll('.word')) w.style.visibility = ''; });
  // inked: does the wash canvas itself (window.__landing.readback(), the
  // same small buffer updateWordContrast reads) actually have paint under
  // this word, at all -- #gl is padded past the print's own edges (--pad),
  // and the print is a fixed image the live, still-scrolling text passes
  // through, so a word's live position is often over the canvas's own
  // bounding box but not over anything it painted (measured live: found
  // fully transparent, a=0, under a real heading word mid-leg). Item D's
  // own words are "read the darkness of the DISPLAYED wash": nothing
  // displayed there is nothing to hold the flip to, so an uninked word is
  // out of this check's scope the same way it is out of
  // updateWordContrast's own (that function's lbg==null just continues).
  const words = await page.evaluate(() => {
    const cv = document.getElementById('gl').getBoundingClientRect();
    const rb = window.__landing.readback?.();
    return [...document.querySelectorAll('.word')].map((w) => {
      const r = w.getBoundingClientRect();
      const overlaps = r.right > cv.left && r.left < cv.right && r.bottom > cv.top && r.top < cv.bottom;
      if (!overlaps) return null;
      let inked = false, rbLbg = null;
      if (rb?.pixels) {
        const nx0 = (r.left - cv.left) / cv.width, nx1 = (r.right - cv.left) / cv.width;
        const ny0 = (r.top - cv.top) / cv.height, ny1 = (r.bottom - cv.top) / cv.height;
        const x0 = Math.max(0, Math.floor(nx0 * rb.w)), x1 = Math.min(rb.w, Math.ceil(nx1 * rb.w));
        const y0 = Math.max(0, Math.floor((1 - ny1) * rb.h)), y1 = Math.min(rb.h, Math.ceil((1 - ny0) * rb.h));
        let opaque = 0, total = 0, sum = 0;
        const bg = rb.bg || [255, 255, 255];
        const srgbToLin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
        const relLum = (r2, g2, b2) => 0.2126 * srgbToLin(r2) + 0.7152 * srgbToLin(g2) + 0.0722 * srgbToLin(b2);
        for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
          total++; const i = (y * rb.w + x) * 4, a = rb.pixels[i + 3] / 255;
          // Composite over the page's own bg (rb.bg, the shader's own
          // uTint), not a straight divide-by-alpha: SHOWK's own output is
          // premultiplied but not by textbook colour*alpha (its o.rgb is
          // uTint*(T-(1-a))), so only standard over-compositing matches
          // what the browser's own canvas compositing -- and so a real
          // screenshot -- actually shows (matches bgLuminanceUnder's own
          // fix in site/landing.js).
          if (rb.pixels[i + 3] > 12) { opaque++; sum += relLum(rb.pixels[i] + bg[0] * (1 - a), rb.pixels[i + 1] + bg[1] * (1 - a), rb.pixels[i + 2] + bg[2] * (1 - a)); }
        }
        inked = total > 0 && opaque / total > 0.3;
        rbLbg = opaque ? sum / opaque : null;
      }
      const heading = !!w.closest('h2');
      return { text: w.textContent, left: r.left, top: r.top, right: r.right, bottom: r.bottom,
        color: getComputedStyle(w).color, required: heading ? 3 : 4.5, inked, rbLbg };
    }).filter(Boolean);
  });
  return { shot, clip, words };
};

// PNG decode without a dependency: Playwright's screenshot buffer is a
// PNG; the one thing needed here is raw RGBA pixels, which `sharp` isn't
// installed for -- reuse the browser's own <canvas> decoder instead of
// pulling in a new package for a local, single-purpose crop read.
async function decodePNG(page, buf) {
  return page.evaluate(async (b64) => {
    const img = new Image();
    const loaded = new Promise((res, rej) => { img.onload = res; img.onerror = rej; });
    img.src = 'data:image/png;base64,' + b64;
    await loaded;
    const c = document.createElement('canvas'); c.width = img.naturalWidth; c.height = img.naturalHeight;
    const g = c.getContext('2d'); g.drawImage(img, 0, 0);
    const d = g.getImageData(0, 0, c.width, c.height).data;
    return { w: c.width, h: c.height, data: Array.from(d) };
  }, buf.toString('base64'));
}

const srgbToLin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const relLum = (r, g, b) => 0.2126 * srgbToLin(r) + 0.7152 * srgbToLin(g) + 0.0722 * srgbToLin(b);
const contrastOf = (l1, l2) => { const a = Math.max(l1, l2), b = Math.min(l1, l2); return (a + 0.05) / (b + 0.05); };
const parseRGB = (str) => { const m = str.match(/[\d.]+/g); return m ? [+m[0], +m[1], +m[2]] : [0, 0, 0]; };

function bgLuminanceUnder(rect, clip, png) {
  const x0 = Math.max(0, Math.round(rect.left - clip.x)), x1 = Math.min(png.w, Math.round(rect.right - clip.x));
  const y0 = Math.max(0, Math.round(rect.top - clip.y)), y1 = Math.min(png.h, Math.round(rect.bottom - clip.y));
  if (x1 <= x0 || y1 <= y0) return null;
  let n = 0, sum = 0;
  for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
    const i = (y * png.w + x) * 4;
    sum += relLum(png.data[i], png.data[i + 1], png.data[i + 2]); n++;
  }
  return n ? sum / n : null;
}

// One leg, one direction: swipes through it in small real touch steps,
// sampling a frame every few, checking each overlapping word's contrast
// and counting its own colour changes.
async function checkLeg(legLabel, totalDy, steps) {
  const seen = new Map();   // word text+left(rounded) -> { colour, flips }
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
    const png = await decodePNG(page, frame.shot);
    for (const w of frame.words) {
      if (!w.inked) continue;   // the wash has nothing displayed here -- out of scope, see sampleFrame's own comment
      const lbg = bgLuminanceUnder(w, frame.clip, png);
      if (lbg == null) continue;
      const [r, g, b] = parseRGB(w.color);
      const ratio = contrastOf(relLum(r, g, b), lbg);
      if (ratio < w.required) bad.push({ leg: legLabel, step: i, text: w.text, required: w.required, ratio: +ratio.toFixed(2), color: w.color, screenshotLbg: +lbg.toFixed(3), productionLbg: w.rbLbg == null ? null : +w.rbLbg.toFixed(3) });
      const key = w.text + '@' + Math.round(w.left / 4);
      const colourKey = r > 200 && g > 200 && b > 200 ? 'bright' : 'dark';
      const prior = seen.get(key);
      if (!prior) seen.set(key, { colour: colourKey, flips: 0 });
      else if (prior.colour !== colourKey) { prior.colour = colourKey; prior.flips++; }
    }
  }
  await drag.end();
  const flicker = [...seen.entries()].filter(([, v]) => v.flips > 2).map(([k, v]) => ({ word: k, flips: v.flips }));
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
      assert(dir.flicker.length === 0, `leg ${leg}->${leg + 1}: a word changed colour more than twice in one direction: ${JSON.stringify(dir.flicker.slice(0, 5))}`);
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
