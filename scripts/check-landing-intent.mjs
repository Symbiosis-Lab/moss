#!/usr/bin/env node
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const playwright = await loadPlaywright();
const locales = ['', 'zh-hans/', 'zh-hant/'];
// Samples scrollY repeatedly over the window instead of sleeping a fixed
// span and checking once at the end -- catches a bounce that happens mid-
// window, not only one still present at the last instant.
async function stableAt(page, checkFn, arg, ms = 500, interval = 25) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (!(await page.evaluate(checkFn, arg))) return false;
    await page.waitForTimeout(interval);
  }
  return true;
}
try {
  for (const engineName of ['chromium', 'webkit']) {
    const browser = await playwright[engineName].launch();
    try {
      for (const locale of locales) {
        for (const viewport of [{ width: 1280, height: 720 }, { width: 1920, height: 1200 }]) {
          const page = await browser.newPage({ viewport });
          await page.goto(new URL(locale, baseURL).href);
          await whenReady(page);
          if (!(await stableAt(page, () => scrollY === 0))) throw new Error(`${engineName} ${locale || 'en'} ${viewport.width}px: the opening moved before input`);
          // Clicking an empty area or pressing a non-navigation key is not a request
          // to leave the intro, even though either can cancel an active spring.
          await page.mouse.click(viewport.width - 40, 150);
          await page.keyboard.press('Shift');
          if (!(await stableAt(page, () => scrollY === 0, null, 1000))) throw new Error(`${engineName} ${locale || 'en'} ${viewport.width}px: non-scroll input left the intro`);
          for (const [direction, goal, ticks] of [[1, 0, 1], [-1, -1, 1], [1, 0, 3], [1, 1, 3], [-1, 0, 3], [-1, -1, 3]]) {
            for (let tick = 0; tick < ticks; tick++) {
              await page.mouse.wheel(0, direction);
              await page.waitForTimeout(35);
            }
            // The visible contract is the chosen rest. Direct/native ownership may
            // clear the internal carryGoal after arriving (notably at document top),
            // so requiring that diagnostic field to remain latched reports a failure
            // while the page is already correctly and stably at rest.
            await page.waitForFunction((g) => Math.abs(scrollY - window.__landing.restY(g)) <= 1, goal, { timeout: 15000 }).catch(async (error) => { throw new Error(`${engineName} ${locale || 'en'} ${viewport.width}px direction=${direction}, goal=${goal}, ticks=${ticks}: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, state: window.__landing.state() })))}`, { cause: error }); });
            const held = await stableAt(page, (g) => Math.abs(scrollY - window.__landing.restY(g)) <= 1, goal, 450);
            if (!held) throw new Error(`${engineName} ${locale || 'en'} ${viewport.width}px: rebounded from rest ${goal}`);
          }
          console.log(`${engineName}/${locale || 'en'} ${viewport.width}×${viewport.height}: single-tick and three-tick gestures settle without rebound`);
          await page.close();
        }
      }
    } finally {
      await browser.close();
    }
  }
} finally {
  await close();
}
