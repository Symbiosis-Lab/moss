#!/usr/bin/env node
// A phone's copy is the page and stays solid while the picture behind it changes scene, and the close
// holds scene 4's composition from the moment it sets off until the reader is back in scene 4.
import { pathToFileURL } from 'node:url';
const modulePath = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium, webkit } = await import(modulePath.startsWith('/') ? pathToFileURL(modulePath).href : modulePath);
const url = process.argv[2];
if (!url) throw Error('Usage: check-landing-text-track.mjs <url>');
const assert = (ok, message) => { if (!ok) throw Error(message); };

// Scrolls to `to` and calls `each` every frame until the picture has arrived.
const drive = `async (to, each) => {
  scrollTo({ top: __restY(to), behavior: 'instant' });
  const t0 = performance.now(); let saw = false;
  for (;;) {
    await new Promise(requestAnimationFrame);
    const s = __state();
    if (performance.now() - t0 > 40000) throw Error('the picture never arrived at scene ' + (to + 1) + ': ' + JSON.stringify(s));
    if (s.leg) saw = true;
    each(s);
    if (saw && !s.leg && s.shown === to) return;
  }
}`;

for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
    await page.goto(url);
    await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 60000 });
    await page.evaluate(() => scrollTo({ top: __restY(1), behavior: 'instant' }));
    await page.waitForFunction(() => __state().shown === 1 && !__state().leg && [0, 1, 2, 3].every((i) => __onHand[i]), null, { timeout: 40000 });

    const faded = await page.evaluate(`(async () => {
      const drive = ${drive}, faded = [];
      for (const [from, to] of [[1, 2], [2, 3], [3, 2], [2, 1], [1, 0], [0, 1]]) {
        await drive(to, () => {
          for (const s of [from, to]) if (getComputedStyle(page).opacity !== '1' || getComputedStyle(scenesEl[s].firstElementChild).opacity !== '1') faded.push(from + '->' + to);
        });
      }
      return [...new Set(faded)];
    })()`);
    assert(!faded.length, `the copy faded with the picture behind it on ${faded.join(', ')}`);

    // Into the close and back: the composition stays where the print took it, then lets go.
    const close = await page.evaluate(`(async () => {
      const drive = ${drive};
      const snap = () => JSON.stringify(__orbit().nodes.map((n) => [Math.round(n.x * 10), Math.round(n.y * 10), !!n.popAt, !!n.exiting]));
      let held = null, moved = false, frames = 0, print = null;
      await drive(3, () => {});
      await drive(4, (s) => { if (!s.leg) return; held ??= snap(); frames++; print ??= deployOut; if (snap() !== held) moved = true; });
      await drive(3, () => {});
      const released = await new Promise((resolve) => { const t0 = performance.now(); (function poll() { if (__orbit().nodes.every((n) => n.popAt)) resolve(true); else if (performance.now() - t0 > 30000) resolve(false); else setTimeout(poll, 100); })(); });
      let again = null;
      await drive(4, (s) => { if (s.leg) again ??= deployOut; });
      return { frames, moved, released, fresh: !!again && again !== print };
    })()`);
    assert(close.frames > 3, 'no leg ran into the close');
    assert(!close.moved, 'scene 4 targets moved while the close held their print');
    assert(close.released, 'scene 4 stopped releasing targets after the close let go');
    assert(close.fresh, 'the next close reused a print of targets that had moved since');
    console.log(`${engine.name()}: copy stays solid through every leg; the close holds scene 4's composition and lets go`);
    await page.close();
  } finally { await browser.close(); }
}
