#!/usr/bin/env node
import { loadPlaywright, resolveBaseURL, whenReady } from './landing-harness.mjs';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const engines = await loadPlaywright();
const browser = await engines[process.env.ENGINE || 'webkit'].launch();
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto(baseURL);
  await whenReady(page);
  await page.mouse.move(1400, 160);
  // One decreasing gesture with a sparse, quantized tail. Its total physical
  // travel is shorter than the intro; the spring must not turn it into new input.
  for (const delta of [30, 20, 12, 6, 3, 2, ...Array(22).fill(1)]) {
    await page.mouse.wheel(0, delta);
    await page.waitForTimeout(140);
  }
  await page.waitForTimeout(1400);
  const atFirst = await page.evaluate(() => Math.abs(scrollY - window.__landing.restY(0)) <= 1);
  if (!atFirst) throw new Error(`Momentum skipped scene one: ${JSON.stringify(await page.evaluate(() => ({ y: scrollY, state: window.__landing.state() })))}`);
  // A new, however light, deliberate gesture still advances immediately.
  await page.mouse.wheel(0, 1);
  await page.waitForFunction(() => Math.abs(scrollY - window.__landing.restY(1)) <= 1, null, { timeout: 15000 });
  await page.waitForTimeout(400);
  await page.mouse.wheel(0, -1);
  await page.waitForFunction(() => Math.abs(scrollY - window.__landing.restY(0)) <= 1, null, { timeout: 15000 });
  console.log('Momentum tail stays on scene one; fresh light gestures advance and reverse.');
} finally {
  await browser.close();
  await close();
}
