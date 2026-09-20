#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const engines = await loadPlaywright();
try {
for (const name of ['chromium', 'webkit']) {
  const browser = await engines[name].launch();
  try {
    for (const viewport of [{ width: 1280, height: 720 }, { width: 1440, height: 900 }]) {
      const page = await browser.newPage({ viewport });
      if (process.env.LANDING_HTML_OVERRIDE) {
        const body = await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8');
        await page.route(baseURL, route => route.fulfill({ contentType: 'text/html', body }));
      }
      await page.goto(baseURL);
      await page.waitForFunction(() => window.__landing.state?.().ready, null, { timeout: 30000 });
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
