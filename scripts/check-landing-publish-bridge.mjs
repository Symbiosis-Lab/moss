#!/usr/bin/env node
// Desktop item 2: the solid Publish control (publishBridge() inside pour())
// carries between scene 3 and scene 4 rather than washing. The owner's ask
// is a timing relation to scene 4's text, not a pixel target, so this steps
// the shared wash clock directly (window.__landing.stepClock/stepLimit,
// the same mechanism check-landing-mobile-handoff.mjs already uses for
// scene 2) instead of racing real scroll: t=T_WET (1.6 sim-seconds, DT=
// 1/120 -- both site/landing.js's own constants) is where the reading line
// reaches scene 4's text (pour()'s own `arrived` check), so that is "the
// text gets into place"; half of that, t=0.8, is "half the text's travel".
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL: url, close } = await resolveBaseURL(process.argv[2]);
const { chromium } = await loadPlaywright();
const assert = (ok, message) => { if (!ok) throw new Error(message); };

const DT = 1 / 120, T_WET = 1.6, T_TOTAL = 2.1;
const stepsFor = (simSeconds) => Math.round(simSeconds / DT);

try {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    await page.goto(url);
    await whenReady(page);
    // Desktop scroll is carry/spring-driven and needs a gesture to arm
    // movement at all -- a wheel delta would also set scene intent, so an
    // empty mouse down/up arms it without pointing it anywhere (same as
    // check-landing-transitions.mjs).
    await page.mouse.move(720, 20);
    await page.mouse.down();
    await page.mouse.up();
    await page.evaluate(() => scrollTo(0, window.__landing.restY(2)));
    await page.waitForFunction(() => window.__landing.state().shown === 2 && !window.__landing.state().running, null, { timeout: 15000 });

    // stepClock/stepLimit=0 arms *before* the jump: desktop's own scroll
    // watcher sets `target` from scrollY every frame (window.__landing.wash()
    // alone races it and loses -- the watcher puts target back to 2 the very
    // next frame), so scrolling to scene 4's own rest is what makes it agree,
    // and the freeze is already in place before that join can take a step.
    await page.evaluate(() => { window.__landing.stepClock = true; window.__landing.stepLimit = 0; });
    await page.evaluate(() => scrollTo(0, window.__landing.restY(3)));
    await page.waitForFunction(() => window.__landing.state().target === 3 && window.__landing.state().running, null, { timeout: 15000 });
    await page.waitForTimeout(100);
    const atStart = await page.locator('#publish-bridge').boundingBox();

    // Half of the text's own travel (t=T_WET/2): the owner's 35-65% band.
    await page.evaluate((n) => { window.__landing.stepLimit = n; }, stepsFor(T_WET / 2));
    await page.waitForFunction((n) => window.__landing.state().steps >= n, stepsFor(T_WET / 2));
    const atHalf = await page.locator('#publish-bridge').boundingBox();

    // The text's own arrival (t=T_WET): within a few pixels of final place.
    await page.evaluate((n) => { window.__landing.stepLimit = n; }, stepsFor(T_WET));
    await page.waitForFunction((n) => window.__landing.state().steps >= n, stepsFor(T_WET));
    const atArrival = await page.locator('#publish-bridge').boundingBox();

    // Run the wash out (stepClock stays on throughout -- turning it off
    // mid-flight starves acc, which only a real scroll or stepClock ever
    // advances, and the wash never reaches T_TOTAL) to read the live
    // control's own resting place.
    await page.evaluate((n) => { window.__landing.stepLimit = n; }, stepsFor(T_TOTAL) + 8);
    await page.waitForFunction(() => window.__landing.state().shown === 3 && !window.__landing.state().running, null, { timeout: 30000 });
    await page.evaluate(() => { window.__landing.stepClock = false; });
    // The live control lives inside #vd (publishBridge() reads it off
    // vdFrame.contentDocument), a same-origin iframe a plain locator can't
    // cross.
    const final = await page.frameLocator('#vd').locator('.moss-publish-button').boundingBox();

    assert(atStart && atHalf && atArrival && final, `boxes missing: ${JSON.stringify({ atStart, atHalf, atArrival, final })}`);
    const pAt = (box) => (box.width - atStart.width) / (final.width - atStart.width);
    const pHalf = pAt(atHalf);
    assert(pHalf >= 0.35 && pHalf <= 0.65, `half of the text's travel: button at ${(pHalf * 100).toFixed(1)}% of its path, want 35-65%`);

    const cx = (box) => box.x + box.width / 2, cy = (box) => box.y + box.height / 2;
    const dist = Math.hypot(cx(atArrival) - cx(final), cy(atArrival) - cy(final));
    const FEW_PX = 4;
    assert(dist <= FEW_PX, `text arrival (t=T_WET): button is ${dist.toFixed(1)}px from its final place, want <= ${FEW_PX}px`);

    console.log(`chromium: publish button at ${(pHalf * 100).toFixed(1)}% of its path at half the text's travel, ${dist.toFixed(1)}px from final place when the text arrives`);
    await page.close();
  } finally { await browser.close(); }
} finally { await close(); }
