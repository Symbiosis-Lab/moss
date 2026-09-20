#!/usr/bin/env node
// The intro title dissolves as the same pigment wash the scenes use, not a
// noise mask. This asserts the owner's word "completely": at scene 1's rest
// no ink remains and the live h1 is not visible, and the reverse consolidates
// it back to solid. Desktop only — mobile, reduced motion and the no-JS/
// static fallback keep the title as plain scrolling text (F below).
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-intro-title.mjs <url>');
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const locales = ['', 'zh-hans/', 'zh-hant/'];
const desktopViewports = [{ width: 1440, height: 900 }, { width: 1100, height: 700 }];

// "Ink" is window.__titleInk(): the title canvas's own alpha, weighted
// against where the source print's own ink was and normalized by it, read
// back with preserveDrawingBuffer (already true on every sim context, so the
// buffer survives between frames — no need to coordinate with the exact
// frame draw() ran in). A weighted read matters because a bloom spreads
// pigment outward and can raise total coverage while the title is lifting; a
// plain average over every pixel is not monotonic through that phase, but
// how much ink is left where the letters actually were is. At either rest
// the canvas is hidden by design, so window.__titleInk() falls back to the
// h1's own opacity there.
/* eslint-disable no-undef */
function measureTitle() {
  const canvas = document.getElementById('gl-title');
  const h1 = document.querySelector('#intro h1');
  return {
    ink: window.__titleInk(),
    h1Opacity: Number(getComputedStyle(h1).opacity),
    canvasVisible: !!canvas && getComputedStyle(canvas).display !== 'none',
    titleSteps: window.__state().titleSteps,
  };
}
/* eslint-enable no-undef */

// The step-clock harness (__stepClock/__stepLimit) belongs to the shared
// pour() wash; the title's own driver runs off requestAnimationFrame, so
// settling means polling the step counter until it stops moving rather than
// asking the harness to hold a frame budget.
async function waitTitleSettled(page) {
  // A poll tick is not exactly one real animation frame (Playwright's own
  // round trip has overhead), so confirm the count twice more a real 100ms
  // apart before trusting it — a one-off rare flake showed the same step
  // count decided "stable" a frame before the next batch actually landed.
  for (let confirmations = 0; confirmations < 2; ) {
    await page.evaluate(() => { window.__titleSettleAt = -1; window.__titleSettleTicks = 0; });
    await page.waitForFunction(() => {
      const cur = window.__state().titleSteps;
      if (cur === window.__titleSettleAt) window.__titleSettleTicks++;
      else { window.__titleSettleAt = cur; window.__titleSettleTicks = 0; }
      return window.__titleSettleTicks >= 8;
    }, null, { timeout: 8000, polling: 'raf' });
    const before = await page.evaluate(() => window.__state().titleSteps);
    await page.waitForTimeout(100);
    const after = await page.evaluate(() => window.__state().titleSteps);
    confirmations = before === after ? confirmations + 1 : 0;
  }
}

for (const [engineName, engine] of [['chromium', chromium], ['webkit', webkit]]) {
  const browser = await engine.launch();
  try {
    for (const locale of locales) {
      for (const viewport of desktopViewports) {
        const label = `${engineName} ${locale || 'en'} ${viewport.width}x${viewport.height}`;
        const page = await browser.newPage({ viewport });
        const errors = [];
        // Uncaught exceptions only, matching check-landing-transitions.mjs:
        // the embedded editor/shell demo iframes log benign console errors
        // (background polling 404s) unrelated to the title, on every run.
        page.on('pageerror', (e) => errors.push(e.message));
        await page.goto(new URL(locale, url).href);
        await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
        // setupTitleDissolve deliberately waits for the page to come to rest
        // before touching WebGL (a compile stall borrows time from the
        // desktop scroll spring otherwise); wait for its own decision rather
        // than assuming it beats a fixed timeout.
        await page.waitForFunction(() => window.__state().titleReady, null, { timeout: 10000 });

        // A: cold load, no input.
        await page.waitForTimeout(1500);
        const cold = await page.evaluate(measureTitle);
        assert(cold.ink === 1 && cold.h1Opacity === 1 && !cold.canvasVisible, `${label}: A cold load is not solid text: ${JSON.stringify(cold)}`);
        assert(cold.titleSteps === 0, `${label}: A cold load already took steps: ${JSON.stringify(cold)}`);

        // B: scrub to 25/50/75% of the way to scene 1's rest.
        const restY0 = await page.evaluate(() => window.__restY(0));
        const readings = [];
        const areas = [];
        for (const frac of [0.25, 0.5, 0.75]) {
          await page.evaluate((y) => scrollTo(0, y), restY0 * frac);
          await waitTitleSettled(page);
          readings.push(await page.evaluate(measureTitle));
          areas.push(await page.evaluate(() => window.__titleArea()));
        }
        assert(readings[0].ink < cold.ink, `${label}: B 25% did not dissolve at all: ${JSON.stringify(readings)}`);
        assert(readings[0].ink > readings[1].ink && readings[1].ink > readings[2].ink, `${label}: B ink did not strictly decrease: ${JSON.stringify(readings)}`);
        assert(readings[1].ink > 0 && readings[1].ink < 1, `${label}: B 50% is not strictly between solid and empty: ${JSON.stringify(readings[1])}`);

        // G: reads as watercolor, not a faint stain, at 25% and 50%: peak
        // alpha at least K1 and pigmented area (alpha > 0.1) at least K2x
        // the print's own solid glyph area. K1=150 and K2=1.5 come from
        // scripts/README.md-style measurement, not taste: driving the main
        // wash scene 0->1 with __wash(1)/__stepClock and reading its own
        // canvas found its darkest trough (the mid-transition smear, step
        // ~160/252) at peak alpha 122-145 and area/source-inked-area ~0.31 —
        // K1/K2 sit below that floor, and this title's own measured 25/50%
        // values (chromium/webkit x en/zh-hans/zh-hant x both viewports)
        // ranged peak alpha 254-255 and area/solid-glyph-area 2.27-18.
        const K1 = 150, K2 = 1.5;
        for (let i = 0; i < 2; i++) {
          const a = areas[i];
          const ratio = a.coveredFrac / a.solidFrac;
          assert(a.peakAlpha >= K1, `${label}: G peak alpha ${a.peakAlpha} below K1=${K1} at ${[25, 50][i]}%: ${JSON.stringify(a)}`);
          assert(ratio >= K2, `${label}: G pigmented area ${ratio.toFixed(2)}x solid glyph area, below K2=${K2} at ${[25, 50][i]}%: ${JSON.stringify(a)}`);
        }

        // C: at rest on scene 1, completely gone.
        await page.evaluate((y) => scrollTo(0, y), restY0);
        await waitTitleSettled(page);
        const rest = await page.evaluate(measureTitle);
        assert(rest.ink === 0, `${label}: C ink remains at scene 1's rest: ${JSON.stringify(rest)}`);
        assert(rest.h1Opacity === 0, `${label}: C the live h1 is still visible at scene 1's rest: ${JSON.stringify(rest)}`);

        // D: scrub back to the top, consolidating.
        await page.evaluate(() => scrollTo(0, 0));
        await waitTitleSettled(page);
        const back = await page.evaluate(measureTitle);
        assert(back.ink >= 0.98, `${label}: D ink did not return within 2%: ${JSON.stringify(back)}`);
        assert(back.h1Opacity === 1, `${label}: D the live h1 did not return: ${JSON.stringify(back)}`);
        assert(errors.length === 0, `${label}: D console or page errors during the run: ${errors.join('; ')}`);

        // E: resting on scene 1, no further steps.
        await page.evaluate((y) => scrollTo(0, y), restY0);
        await waitTitleSettled(page);
        const beforeIdle = await page.evaluate(() => window.__state().titleSteps);
        await page.waitForTimeout(1500);
        const afterIdle = await page.evaluate(() => window.__state().titleSteps);
        assert(beforeIdle === afterIdle, `${label}: E steps increased at rest: ${beforeIdle} -> ${afterIdle}`);

        console.log(`${label}: cold solid, scrub dissolves both ways and reads as a bleed (not a stain), rest is completely gone, idle takes no steps`);
        await page.close();
      }
    }
  } finally { await browser.close(); }
}

// F: mobile, and reduced motion, keep today's plain scrolling text.
for (const [engineName, engine] of [['chromium', chromium], ['webkit', webkit]]) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, reducedMotion: 'reduce' });
    await page.goto(url);
    await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
    const state = await page.evaluate(() => ({
      canvas: !!document.getElementById('gl-title'),
      h1Opacity: Number(getComputedStyle(document.querySelector('#intro h1')).opacity),
    }));
    assert(!state.canvas, `${engineName}: F a title canvas was created on mobile + reduced motion`);
    assert(state.h1Opacity === 1, `${engineName}: F the title is not plain visible text: ${JSON.stringify(state)}`);
    console.log(`${engineName} mobile+reduced-motion: no title canvas, plain visible text`);
    await page.close();
  } finally { await browser.close(); }
}
