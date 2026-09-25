#!/usr/bin/env node
// Desktop item 2: the solid Publish control (publishBridge() inside pour())
// carries between scene 3 and scene 4 rather than washing. The owner's ask
// is a timing relation to scene 4's text, not a pixel target. A transition
// is presented by scroll position now (p, 0 to 1 across the leg), and the
// bridge is drawn at t = p * T_TOTAL sim-seconds (pour()'s washT), so this
// holds the page at the positions that name those times: t = T_WET (1.6
// sim-seconds, site/landing.js's own constant) is where the reading line
// reaches scene 4's text, "the text gets into place"; half of that, t = 0.8,
// is "half the text's travel".
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL: url, close } = await resolveBaseURL(process.argv[2]);
const { chromium } = await loadPlaywright();
const assert = (ok, message) => { if (!ok) throw new Error(message); };

const T_WET = 1.6, T_TOTAL = 2.1;

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
    // and every print this scene's legs need on hand, as a reader resting here has them
    await page.waitForFunction(() => window.__landing.state().primed, null, { timeout: 30000 });

    // A pointer held on the scrollbar keeps the carry spring off the page,
    // so each position stays where it is put while it is read. The page is
    // walked forward a few pixels a frame, as a reader's scroll moves it,
    // and read where the leg's progress first reaches each p: jumping about
    // instead would carry the leg to its end and start another.
    // The bridge starts exactly over the live control, so the control at
    // rest is its start. (A held pointer is direct manipulation: the leg
    // itself starts a little way into the gap between the two texts.)
    const atStart = await page.frameLocator('#vd').locator('.moss-publish-button').boundingBox();
    await page.evaluate(() => dispatchEvent(new PointerEvent('pointerdown', { clientX: 1e5, clientY: 10, pointerType: 'mouse', button: 0 })));
    const frames = (n) => page.evaluate((n) => new Promise((r) => { const f = () => (--n > 0 ? requestAnimationFrame(f) : r()); requestAnimationFrame(f); }), n);
    let y = await page.evaluate(() => scrollY);
    const y3 = await page.evaluate(() => window.__landing.restY(3));
    const holdAt = async (p) => {
      while (y < y3 && (await page.evaluate(() => window.__landing.state().progress)) - 2 < p) {
        y += 3; await page.evaluate((y) => scrollTo(0, y), y); await frames(1);
      }
      await page.waitForFunction(() => window.__landing.state().running && window.__landing.morph.current()?.exact !== false, null, { timeout: 60000 });
      return page.locator('#publish-bridge').boundingBox();
    };
    // Half of the text's own travel (t=T_WET/2): the owner's 35-65% band.
    const atHalf = await holdAt(T_WET / 2 / T_TOTAL);
    // The text's own arrival (t=T_WET): within a few pixels of final place.
    const atArrival = await holdAt(T_WET / T_TOTAL);
    // Let go at scene 4's rest to read the live control's own resting place.
    await page.evaluate((y) => scrollTo(0, y), y3);
    await page.evaluate(() => dispatchEvent(new PointerEvent('pointerup', { clientX: 1e5, clientY: 10, pointerType: 'mouse', button: 0 })));
    await page.waitForFunction(() => window.__landing.state().shown === 3 && !window.__landing.state().running, null, { timeout: 30000 });
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
