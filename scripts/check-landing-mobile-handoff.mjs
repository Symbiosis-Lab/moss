#!/usr/bin/env node
// Rewritten to drive by real scroll position, not the shared step clock
// (unit7-scroll-and-flicker-spec.md part A: the morph is a pure function of
// p = clamp01(progressAt() - from), the reader's own scroll position -- a
// check that pins scrollY and hand-advances window.__landing.stepClock/
// stepLimit can no longer see cover or the scene switch move at all, since
// neither reads the clock any more). Every assertion below is the same
// claim the step-clock version made -- source preserved before the wash
// starts, covered before the switch, switched only under opaque cover, the
// logo group arriving solid with it, the target revealed gradually, the
// Publish control growing through the crossing -- just asked at a scroll
// progress p instead of a step count N. This is a drive-mechanism change,
// not a loosened bar: every threshold below is the same one the removed
// version used, or is read off the fix's own .35/.55/.82 breakpoints.
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const { chromium, webkit } = await loadPlaywright();
const assert = (condition, message) => { if (!condition) throw new Error(message); };

// Two consecutive reads of the pigment-facing state 50ms apart, unchanged --
// the same convergence-by-polling shape check-landing-intro-title.mjs uses
// for its own clock (waitTitleSettled), because a fixed sleep either wastes
// time on a fast engine or is too short on a slow one. Cover itself is
// instant under the fix (a pure function of progress), but the simulation
// clock `t` behind the Publish control's easing and the drawn pigment still
// converges toward it over a handful of real frames, DT=1/120 and
// STEPS_PER_FRAME=36 a frame -- about seven frames to cross T_TOTAL=2.1s of
// sim time -- so this is what "a frame" has to mean for those to settle.
async function waitSettled(page, timeoutMs = 2000) {
  const read = () => page.evaluate(() => stage.style.getPropertyValue('--wash-cover') + '|' + (cell.style.transform || ''));
  let last = await read();
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    await page.waitForTimeout(50);
    const cur = await read();
    if (cur === last) return;
    last = cur;
  }
}

// restY(i) is where progressAt() reads exactly i (the same geometry pour()'s
// own gapVh reads); the span between two adjacent rests is walked linearly,
// the same approximation the removed version's step count was itself only
// an approximation of. progressAt() is read back afterward and everything
// below judges the ACTUAL leg-relative progress reached, not the nominal p
// asked for -- mobileInkProgress is not perfectly linear in pixels, and the
// fix's own breakpoints (.35/.55/.82) are what matters, not the input.
async function scrollToProgress(page, scene, p) {
  await page.evaluate(({ scene, p }) => {
    const y0 = window.__landing.restY(scene), y1 = window.__landing.restY(scene + 1);
    scrollTo(0, Math.round(y0 + (y1 - y0) * p));
  }, { scene, p });
  await waitSettled(page);
  const progress = await page.evaluate(() => window.__landing.state().progress);
  return progress - scene;
}

try {
for (const engine of [chromium, webkit]) {
  const browser = await engine.launch();
  try {
    for (const scene of [1, 2]) {
      const page = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true });
      const read = () => page.evaluate(() => ({
        scene: stage.dataset.scene, phase,
        cover: Number(stage.style.getPropertyValue('--wash-cover')) || 0,
        fanFilter: fanEl.style.filter, fanZ: fanEl.style.zIndex,
        items: ['box', 'sib-nb', 'sib-sk', 's3-video'].map(id => {
          const e = document.getElementById(id), r = e.getBoundingClientRect(), c = getComputedStyle(e);
          return { id, x: r.x, y: r.y, w: r.width, h: r.height, opacity: c.opacity, visibility: c.visibility };
        })
      }));
      await page.goto(baseURL);
      await whenReady(page, { timeout: 60000 });
      await page.evaluate(() => scrollTo(0, window.__landing.restY(1) + 24));
      await page.waitForFunction(() => [0, 1, 2, 3].every(i => window.__landing.prints[i]), null, { timeout: 30000 });
      await page.evaluate(s => scrollTo(0, window.__landing.restY(s)), scene);
      await page.waitForFunction(s => window.__landing.state().shown === s && !window.__landing.state().running, scene, { timeout: 15000 });

      const before = await read();
      const p0 = await scrollToProgress(page, scene, 0.02);
      const zero = await read();
      assert(JSON.stringify(before) === JSON.stringify(zero), `p=${p0.toFixed(3)}: source changed before the wash started: ${JSON.stringify({ scene, before, zero })}`);

      const buttonStart = scene === 2 ? await page.locator('#publish-bridge').boundingBox() : null;

      const p1 = await scrollToProgress(page, scene, 0.15);
      const rising = await read();
      assert(rising.cover > 0 && rising.cover < 1 && Number(rising.scene) === scene,
        `p=${p1.toFixed(3)}: source was replaced before the wash covered it: ${JSON.stringify(rising)}`);

      const p2 = await scrollToProgress(page, scene, 0.5); // between the fix's .35 and .55 breakpoints: covered, not yet switched
      const covering = await read();
      assert(covering.cover >= 0.98 && Number(covering.scene) === scene,
        `p=${p2.toFixed(3)}: not fully covered before the latch (want cover>=0.98, scene unswitched): ${JSON.stringify(covering)}`);
      if (scene === 2) {
        const scale = await page.evaluate(() => Number(cell.style.transform.match(/scale\(([\d.]+)\)\s*$/)[1]));
        assert(scale < 1.14, `p=${p2.toFixed(3)}: control did not ease in (scale ${scale}, want < 1.14)`);
      }

      const p3 = await scrollToProgress(page, scene, 0.7); // between .55 and .82: covered AND switched
      const covered = await read();
      assert(covered.cover >= 0.98 && Number(covered.scene) === scene + 1,
        `p=${p3.toFixed(3)}: scene did not switch under opaque cover: ${JSON.stringify(covered)}`);
      if (scene === 2) {
        assert(covered.fanFilter === 'none' && covered.fanZ === '101',
          `p=${p3.toFixed(3)}: logo group did not arrive solid after the switch: ${JSON.stringify(covered)}`);
      }

      const p4 = await scrollToProgress(page, scene, 0.95); // past .82: falling, target revealing
      const end = await read();
      assert(end.cover > 0 && end.cover < 1 && Number(end.scene) === scene + 1,
        `p=${p4.toFixed(3)}: target did not reveal gradually: ${JSON.stringify(end)}`);
      if (scene === 2) {
        const buttonEnd = await page.locator('#publish-bridge').boundingBox();
        assert(buttonStart && buttonEnd && buttonEnd.width >= buttonStart.width * 2,
          'solid Publish control did not grow through the wash');
      }

      console.log(`${engine.name()}: scene ${scene + 1} source preserved; target revealed gradually`);
      await page.close();
    }
  } finally { await browser.close(); }
}
} finally { await close(); }
