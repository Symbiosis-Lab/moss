#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL, whenReady, installPageOverride } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const engines = await loadPlaywright();
const overrideHtml = process.env.LANDING_HTML_OVERRIDE ? await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8') : null;
try {
for (const name of ['chromium', 'webkit']) {
  const browser = await engines[name].launch();
  try {
    for (const viewport of [{ width: 1280, height: 720 }, { width: 1440, height: 900 }]) {
      const page = await browser.newPage({ viewport });
      await installPageOverride(page, baseURL, { html: overrideHtml, jsOverridePath: process.env.LANDING_JS_OVERRIDE });
      await page.goto(baseURL);
      await whenReady(page);
      await page.evaluate(() => scrollTo(0, window.__landing.restY(0) + 2));
      await page.waitForTimeout(500);
      const samples = await page.evaluate(async () => {
        const out = [];
        for (const offset of [2, 60, 120, 240, 420, 240, 60, 2]) {
          scrollTo(0, window.__landing.restY(0) + offset);
          // Inspect before JS gets a frame to compensate, as well as afterward.
          for (let frame = 0; frame < 2; frame++) {
            const stage = document.querySelector('#stage').getBoundingClientRect();
            const vis = document.querySelector('#vis').getBoundingClientRect();
            out.push({ x: stage.x, y: stage.y, width: stage.width, visTop: vis.top });
            await new Promise(requestAnimationFrame);
          }
        }
        return out;
      });
      const first = samples[0];
      if (samples.some(s => Math.abs(s.visTop) > .5 || ['x', 'y', 'width'].some(k => Math.abs(s[k] - first[k]) > .5))) {
        throw new Error(`${name}/${viewport.width}: settled visual moved: ${JSON.stringify(samples)}`);
      }
      console.log(`${name}/${viewport.width}: CSS-pinned visual stays fixed before and after animation frames`);
      await page.close();
    }
  } finally { await browser.close(); }
}
} finally { await close(); }

// I-pin closing (unit 4, review-phases-2-4.md Job 2 item 5; owner: "the
// publish button should not scroll up"): on desktop, #vis's box must not
// move as xf runs 0 to 1 -- the same CSS-pinned-visual invariant as above,
// applied to the one boundary it used to break for. #vis's sticky range is
// #col's own stretched height (set by #copy's content) minus #vis's own
// height; #copy's content alone used to end right where #five's offsetTop
// falls, so the pin broke there, hundreds of px before the crossfade band
// even starts.
try {
for (const name of ['chromium', 'webkit']) {
  const browser = await engines[name].launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    await installPageOverride(page, baseURL, { html: overrideHtml, jsOverridePath: process.env.LANDING_JS_OVERRIDE });
    await page.goto(baseURL);
    await whenReady(page);
    const bandNear = await page.evaluate(() => document.getElementById('five').offsetTop - innerHeight * 0.8);
    const samples = [];
    for (const xf of [0, 0.25, 0.5, 0.75, 1]) {
      const y = Math.round(bandNear + xf * (900 * 0.6));
      await page.evaluate((y) => scrollTo(0, y), y);
      await page.waitForTimeout(150);
      const vis = await page.evaluate(() => document.getElementById('vis').getBoundingClientRect());
      samples.push({ xf, y, top: vis.top });
    }
    if (samples.some((s) => Math.abs(s.top) > .5)) {
      throw new Error(`${name}: I-pin closing -- #vis moved during the crossfade: ${JSON.stringify(samples)}`);
    }
    console.log(`${name}: I-pin closing #vis's box does not move between xf 0 and 1`);
    await page.close();
  } finally { await browser.close(); }
}
} finally { await close(); }
