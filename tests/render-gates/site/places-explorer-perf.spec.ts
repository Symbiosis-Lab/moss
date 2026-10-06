/**
 * Render gate: the places explorer composites instead of repainting.
 *
 * The scratch site comes from tests/e2e/helpers/gate-sites.ts
 * (PLACES_EXPLORER_GATE), built by the playwright config at parse time.
 *
 * A generous bound (not a tight perf budget): today's inline, filtered-SVG
 * world/tile layers re-run their relief/lighting filters on every repaint of
 * the pan transform, measured costing WebKit whole seconds once a deep zoom
 * has several regional tiles on screen (reproduced here too: an ablation of
 * this change, forcing the pre-change live-SVG fallback and reverting the
 * permanent compositor promotion, read a 1052ms frame gap at rest in
 * WebKit). See this file's own `MAX_FRAME_GAP_MS`/`MAX_CLICK_MS` for the
 * bounds and map.ts's module doc for the fuller numbers this change was
 * measured against on a real site.
 *
 *   npx playwright test -c playwright/places-explorer-perf.config.ts
 */
import { test, expect, type Page } from "@playwright/test";

async function gotoReady(page: Page, path: string): Promise<void> {
  await page.goto(path, { waitUntil: "domcontentloaded" });
  await expect(page.locator(".moss-place-map[data-moss-places-explorer]")).toHaveAttribute(
    "data-moss-places-explorer-ready",
    "ready",
    { timeout: 10000 },
  );
}

/** The widest gap between two consecutive animation frames while stepping `.moss-places-world`'s own transform for `ms` — the same additive-jitter technique the perf investigation this gate guards used, layered on top of whatever transform the camera already set rather than replacing it. */
function worstFrameGap(page: Page, ms: number): Promise<number> {
  return page.evaluate((ms) => new Promise<number>((resolve) => {
    const world = document.querySelector<HTMLElement>(".moss-places-world");
    if (!world) { resolve(0); return; }
    const base = world.style.transform;
    let last = performance.now();
    let worst = 0;
    const start = last;
    const tick = (t: number) => {
      worst = Math.max(worst, t - last);
      last = t;
      const dt = t - start;
      world.style.transform = `${base} translate(${Math.sin(dt / 120) * 24}px, ${Math.cos(dt / 160) * 14}px)`;
      if (dt < ms) requestAnimationFrame(tick);
      else resolve(Math.round(worst));
    };
    requestAnimationFrame(tick);
  }), ms);
}

/** A real Playwright click on the zoom-in control, timed end to end. A generous outer timeout on the click call itself — the assertion below is what actually enforces this gate's own bound; a higher ceiling here just keeps a genuinely slow click reported as "too slow" rather than as Playwright's own unrelated actionability-timeout exception. */
async function zoomInClickMs(page: Page): Promise<number> {
  const button = page.locator('.moss-places-control[data-control="zoom-in"]');
  const start = Date.now();
  await button.click({ timeout: 15000 });
  return Date.now() - start;
}

/** Deliberately generous: see this file's own module doc for why this is chosen to fail clearly on the old inline-SVG behaviour (whose own dropped frames were measured in the hundreds of ms to multiple seconds) while passing with margin on the raster one (measured holding under 25ms once warmed up, occasionally into the low hundreds while regional tiles are still finishing their own throttled decode). */
const MAX_FRAME_GAP_MS = 700;
/** The goal's own click target is under 200ms; this gate's bound is the softer "never leave the reader staring at a frozen capsule" line from the brief. */
const MAX_CLICK_MS = 1000;

// A coastal fixture place (Lisbon, 38.722N 9.139W, matching the camera
// gate's own choice) whose cell plus its eight neighbours all carry
// regional tile data — reached via a real 8x zoom-in sequence (`z=8`) in
// the existing camera/ring gates; this one jumps straight to that camera
// via the URL, the same shortcut the original perf spike behind this
// change used to reach "many tiles on screen" without re-driving 8 real
// clicks through a page that may still be catching up from the last one.
const DEEP_ZOOM_PATH = "places/?p=patterson&z=8&x=399.6415&y=144.8651";

test.describe.configure({ mode: "serial" });

test("a continuous pan at rest never drops a frame past the bound", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page, "places/");
  await page.waitForTimeout(250);
  // Two warm-up passes, discarded: the first couple of rAF bursts after a
  // page's own load/ready transition read a large, one-off gap in WebKit
  // regardless of this change (the engine's own post-load settling,
  // reproduced on a trivial unrelated page in the investigation behind
  // this gate, which needed the same three-burst shape to isolate a
  // steady-state reading) — real use never times its own "page just
  // opened", only a continuous pan well after that point, which the third,
  // measured pass isolates.
  await worstFrameGap(page, 1500);
  await worstFrameGap(page, 1500);
  const gap = await worstFrameGap(page, 1500);
  expect(gap, `worst frame gap during a 1.5s pan at rest: ${gap}ms`).toBeLessThanOrEqual(MAX_FRAME_GAP_MS);
});

test("a continuous pan at a deep zoom with regional tiles in view never drops a frame past the bound", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page, DEEP_ZOOM_PATH);
  await page.waitForTimeout(500); // tile fetch/decode + position settle
  // Same two-pass warm-up as the rest-camera test above — navigating
  // straight to a deep zoom also means up to a few dozen regional tiles are
  // still finishing their own (throttled, see tiles.ts) fetch-and-decode
  // during the first moments after ready, which this discards rather than
  // measures.
  await worstFrameGap(page, 1500);
  await worstFrameGap(page, 1500);
  const gap = await worstFrameGap(page, 1500);
  expect(gap, `worst frame gap during a 1.5s pan at deep zoom: ${gap}ms`).toBeLessThanOrEqual(MAX_FRAME_GAP_MS);
});

/** Same multi-burst settle the pan tests above warm up for (their own doc explains why two is what this harness needs) — a click dispatched into that window measures Playwright's own post-load actionability wait, not this app's response; real use never clicks a page in its very first moments either. */
async function settle(page: Page): Promise<void> {
  await worstFrameGap(page, 1500);
  await worstFrameGap(page, 1500);
}

test("a real click on the zoom control resolves quickly at rest", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page, "places/");
  await settle(page);
  const ms = await zoomInClickMs(page);
  expect(ms, `zoom-in click latency at rest: ${ms}ms`).toBeLessThanOrEqual(MAX_CLICK_MS);
});

test("a real click on the zoom control resolves quickly at a deep zoom with tiles in view", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page, DEEP_ZOOM_PATH);
  await page.waitForTimeout(500);
  await settle(page);
  const ms = await zoomInClickMs(page);
  expect(ms, `zoom-in click latency at deep zoom: ${ms}ms`).toBeLessThanOrEqual(MAX_CLICK_MS);
});

test("a plain-wheel burst paints the final camera and settles once", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await gotoReady(page, "places/");
  await page.waitForTimeout(500);
  const result = await page.evaluate(async () => {
    const viewport = document.querySelector<HTMLElement>(".moss-places-viewport")!;
    const world = document.querySelector<HTMLElement>(".moss-places-world")!;
    const rect = viewport.getBoundingClientRect();
    const before = world.style.transform;
    let settlements = 0;
    const replaceState = history.replaceState;
    history.replaceState = function (...args) {
      settlements++;
      return replaceState.apply(this, args);
    };
    let active = true;
    let previous = performance.now();
    let worstGap = 0;
    const tick = (now: number) => {
      worstGap = Math.max(worstGap, now - previous);
      previous = now;
      if (active) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    try {
      for (let i = 0; i < 40; i++) {
        const event = new WheelEvent("wheel", {
          deltaY: -5,
          clientX: rect.left + rect.width / 2,
          clientY: rect.top + rect.height / 2,
          cancelable: true,
        });
        viewport.dispatchEvent(event);
        if (!event.defaultPrevented) throw new Error("map did not claim plain wheel");
        await new Promise((resolve) => setTimeout(resolve, 4));
      }
      await new Promise((resolve) => setTimeout(resolve, 300));
      return { settlements, worstGap, changed: before !== world.style.transform, settled: !world.hasAttribute("data-gesture") };
    } finally {
      active = false;
      history.replaceState = replaceState;
    }
  });
  expect(result.changed).toBe(true);
  expect(result.settled).toBe(true);
  expect(result.settlements).toBe(1);
  expect(result.worstGap).toBeLessThanOrEqual(MAX_FRAME_GAP_MS);
});
