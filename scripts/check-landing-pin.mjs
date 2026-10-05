#!/usr/bin/env node
import { pathToFileURL } from 'node:url';
import { readFile } from 'node:fs/promises';
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
if (!process.argv[2]) throw new Error('Usage: node scripts/check-landing-pin.mjs <site-url>');
for (const name of ['chromium', 'webkit']) {
  const browser = await engines[name].launch();
  try {
    for (const viewport of [{ width: 1280, height: 720 }, { width: 1440, height: 900 }]) {
      const page = await browser.newPage({ viewport });
      if (process.env.LANDING_HTML_OVERRIDE) {
        const body = await readFile(process.env.LANDING_HTML_OVERRIDE, 'utf8');
        await page.route(new URL(process.argv[2]).href, route => route.fulfill({ contentType: 'text/html', body }));
      }
      await page.goto(process.argv[2]);
      await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
      // The page snaps to its rests, so positions between them are held with the snap off. Each is read
      // before JS gets a frame to compensate, as well as afterward, once the picture has caught up with
      // the page: the pinned visual must not move with the scroll, whatever scene the picture is showing.
      await page.addStyleTag({ content: 'html { scroll-snap-type: none !important }' });
      await page.evaluate(() => scrollTo(0, __restY(0) + 2));
      await page.waitForTimeout(500);
      const samples = await page.evaluate(async () => {
        const out = [];
        for (const offset of [2, 60, 120, 240, 420, 240, 60, 2]) {
          scrollTo(0, __restY(0) + offset);
          const until = performance.now() + 15000;
          while (Math.abs(__state().view - scrollY) > 1) {
            if (performance.now() > until) throw new Error(`the picture never caught up with the page at offset ${offset}: ${JSON.stringify(__state())}`);
            await new Promise((r) => setTimeout(r, 40));
          }
          for (let frame = 0; frame < 2; frame++) {
            const vis = document.querySelector('#vis').getBoundingClientRect();
            out.push({ offset, visTop: vis.top, stage: document.querySelector('#stage').getBoundingClientRect().toJSON() });
            await new Promise(requestAnimationFrame);
          }
        }
        return out;
      });
      // pinned: #vis stays at the top, and the stage stays where it was at the first offset, at every offset and between the frames of one
      const first = samples[0].stage;
      const drifted = samples.filter((sample, i) => Math.abs(sample.visTop) > .5 || ['x', 'y', 'width'].some(k => Math.abs(sample.stage[k] - first[k]) > .5 || (i % 2 && Math.abs(sample.stage[k] - samples[i - 1].stage[k]) > .5)));
      if (drifted.length) throw new Error(`${name}/${viewport.width}: settled visual moved: ${JSON.stringify(drifted)}`);
      console.log(`${name}/${viewport.width}: CSS-pinned visual stays fixed before and after animation frames`);
      await page.close();
    }
  } finally { await browser.close(); }
}
