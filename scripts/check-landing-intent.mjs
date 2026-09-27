#!/usr/bin/env node
import { pathToFileURL } from 'node:url';
const base = process.argv[2];
if (!base) throw new Error('Usage: node scripts/check-landing-intent.mjs <site-url>');
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium } = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
const browser = await chromium.launch();
try {
  for (const viewport of [{ width: 1280, height: 720 }, { width: 1920, height: 1200 }]) {
    const page = await browser.newPage({ viewport });
    await page.goto(base);
    await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
    await page.waitForTimeout(500);
    if (await page.evaluate(() => scrollY !== 0)) throw new Error('The opening moved before input');
    // Clicking an empty area or pressing a non-navigation key is not a request
    // to leave the intro, even though either can cancel an active spring.
    await page.mouse.click(viewport.width - 40, 150);
    await page.keyboard.press('Shift');
    await page.waitForTimeout(1000);
    if (await page.evaluate(() => scrollY !== 0)) throw new Error('Non-scroll input left the intro');
    for (const [direction, goal, ticks] of [[1, 0, 1], [-1, -1, 1], [1, 0, 3], [1, 1, 3], [-1, 0, 3], [-1, -1, 3]]) {
      for (let tick = 0; tick < ticks; tick++) {
        await page.mouse.wheel(0, direction);
        await page.waitForTimeout(35);
      }
      // The visible contract is the chosen rest. Direct/native ownership may
      // clear the internal carryGoal after arriving (notably at document top),
      // so requiring that diagnostic field to remain latched reports a failure
      // while the page is already correctly and stably at rest.
      await page.waitForFunction(goal => Math.abs(scrollY - __restY(goal)) <= 1, goal, { timeout: 15000 }).catch(async error => { throw new Error(`${viewport.width}px direction=${direction}, goal=${goal}, ticks=${ticks}: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, state: __state() })))}`, { cause: error }); });
      await page.waitForTimeout(450);
      const held = await page.evaluate(goal => Math.abs(scrollY - __restY(goal)) <= 1, goal);
      if (!held) throw new Error(`Rebounded from rest ${goal}`);
    }
    console.log(`${viewport.width}×${viewport.height}: single-tick and three-tick gestures settle without rebound`);
    await page.close();
  }
} finally {
  await browser.close();
}
