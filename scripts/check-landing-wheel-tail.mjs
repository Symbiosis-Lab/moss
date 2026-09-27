#!/usr/bin/env node
import { pathToFileURL } from 'node:url';
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
if (!process.argv[2]) throw new Error('Usage: node scripts/check-landing-wheel-tail.mjs <site-url>');
const browser = await engines[process.env.ENGINE || 'webkit'].launch();
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto(process.argv[2]);
  await page.waitForFunction(() => window.__state?.().ready, null, { timeout: 30000 });
  await page.mouse.move(1400, 160);
  // One decreasing gesture with a sparse, quantized tail. Its total physical
  // travel is shorter than the intro; the spring must not turn it into new input.
  for (const delta of [30, 20, 12, 6, 3, 2, ...Array(22).fill(1)]) {
    await page.mouse.wheel(0, delta);
    await page.waitForTimeout(140);
  }
  await page.waitForTimeout(1400);
  const atFirst = await page.evaluate(() => Math.abs(scrollY - __restY(0)) <= 1);
  if (!atFirst) throw new Error(`Momentum skipped scene one: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, state: __state() })))}`);
  // A new, however light, deliberate gesture still advances immediately.
  await page.mouse.wheel(0, 1);
  await page.waitForFunction(() => Math.abs(scrollY - __restY(1)) <= 1, null, { timeout: 15000 });
  await page.waitForTimeout(400);
  await page.mouse.wheel(0, -1);
  await page.waitForFunction(() => Math.abs(scrollY - __restY(0)) <= 1, null, { timeout: 15000 });
  console.log('Momentum tail stays on scene one; fresh light gestures advance and reverse.');
} finally {
  await browser.close();
}
