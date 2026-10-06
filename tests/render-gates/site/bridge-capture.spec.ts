import { expect, test, type Page } from '@playwright/test';

// The preview bridge answers `moss-capture-request` with a bitmap of its own
// page. The fixture is a flex-column body with a clipped, full-bleed hero (the
// layout a fixed-height body collapses), a data-url @font-face heading and
// two images. In a top-level page window.parent is the window itself, so the
// test plays the host by posting to its own window.

// A pixel comparison needs one device pixel per CSS pixel (the engine device
// presets set their own size and density, Desktop Safari at 2x).
test.use({ viewport: { width: 800, height: 600 }, deviceScaleFactor: 1 });

const FIXTURE = '/playwright/fixtures/bridge-capture/page.html';
const STRETCH_FIXTURE = '/playwright/fixtures/bridge-capture/stretch.html';
// Mean absolute channel difference out of 255 over the whole viewport.
// Measured on this fixture: under 1 in both engines (photo resampling is the
// only source); a collapsed hero or a fallback font is above 10.
const MEAN_DIFF_MAX = 3;
const HEADING_BOX_TOLERANCE_PX = 2;

interface Capture {
  width: number;
  height: number;
  /** Mean diff against the screenshot, out of 255. */
  mean: number;
  /** Ink bounding box of the heading in the capture and in the screenshot. */
  ink: { cap: Box | null; shot: Box | null };
  heroPixel: number[] | null;
  heroLivePixel: number[] | null;
}
type Box = { x: number; y: number; w: number; h: number };

async function load(page: Page, fixture = FIXTURE) {
  await page.addInitScript(() => {
    (window as any).__messages = [];
    window.addEventListener('message', (e) => (window as any).__messages.push(e.data));
  });
  await page.goto(fixture);
  await page.evaluate(() => document.fonts.ready);
  await expect.poll(() => page.evaluate(() => [...document.images].every((i) => i.complete))).toBe(true);
}

/** Ask the page for a bitmap and compare it to what the browser really painted. */
async function captureAndCompare(page: Page): Promise<Capture> {
  const shot = (await page.screenshot()).toString('base64');
  return page.evaluate(async (shotB64) => {
    const response: any = await new Promise((ok) => {
      window.addEventListener('message', function on(e) {
        if (e.data?.type === 'moss-capture-response') {
          window.removeEventListener('message', on);
          ok(e.data);
        }
      });
      window.postMessage({ type: 'moss-capture-request', id: 1 }, '*');
    });
    if (!response.ok) throw new Error(response.error);
    const { width: w, height: h } = response;
    const mk = () => {
      const c = document.createElement('canvas');
      c.width = w;
      c.height = h;
      return c;
    };
    const a = mk(), b = mk();
    const ga = a.getContext('2d', { willReadFrequently: true })!, gb = b.getContext('2d', { willReadFrequently: true })!;
    gb.drawImage(response.bitmap, 0, 0);
    const img = new Image();
    await new Promise((ok) => { img.onload = ok; img.src = 'data:image/png;base64,' + shotB64; });
    ga.drawImage(img, 0, 0);
    const A = ga.getImageData(0, 0, w, h).data, B = gb.getImageData(0, 0, w, h).data;
    let sum = 0;
    for (let i = 0; i < A.length; i += 4) sum += (Math.abs(A[i] - B[i]) + Math.abs(A[i + 1] - B[i + 1]) + Math.abs(A[i + 2] - B[i + 2])) / 3;
    // Ink box: pixels in the heading's live rect that differ from the rect's corner.
    const hr = document.querySelector('h1')?.getBoundingClientRect();
    const ink = (data: Uint8ClampedArray): Box | null => {
      if (!hr) return null;
      const x0 = Math.max(0, Math.floor(hr.x)), y0 = Math.max(0, Math.floor(hr.y));
      const x1 = Math.min(w, Math.ceil(hr.right)), y1 = Math.min(h, Math.ceil(hr.bottom));
      const p0 = (y0 * w + x0) * 4;
      let minx = 1e9, maxx = -1, miny = 1e9, maxy = -1;
      for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
        const p = (y * w + x) * 4;
        if (Math.abs(data[p] - data[p0]) + Math.abs(data[p + 1] - data[p0 + 1]) + Math.abs(data[p + 2] - data[p0 + 2]) > 120) {
          minx = Math.min(minx, x); maxx = Math.max(maxx, x); miny = Math.min(miny, y); maxy = Math.max(maxy, y);
        }
      }
      return maxx < 0 ? null : { x: minx, y: miny, w: maxx - minx + 1, h: maxy - miny + 1 };
    };
    // A pixel in the hero's top-left corner, where only its background shows.
    const hero = document.querySelector('.hero')?.getBoundingClientRect();
    const hp = (data: Uint8ClampedArray) => {
      if (!hero) return null;
      const x = Math.floor(Math.max(0, hero.x)) + 2, y = Math.max(0, Math.floor(hero.y)) + 2, p = (y * w + x) * 4;
      return [data[p], data[p + 1], data[p + 2]];
    };
    return { width: w, height: h, mean: sum / (w * h), ink: { cap: ink(B), shot: ink(A) }, heroPixel: hp(B), heroLivePixel: hp(A) };
  }, shot);
}

test('the bridge advertises that it can capture', async ({ page }) => {
  await load(page);
  const nav = await page.evaluate(() => (window as any).__messages.find((m: any) => m?.type === 'moss-navigation'));
  expect(nav.capture).toBe(true);
});

for (const scrollTop of [0, 300]) {
  test(`a capture at scroll ${scrollTop} matches the live render`, async ({ page }) => {
    await load(page);
    if (scrollTop) await page.evaluate((y) => window.scrollTo(0, y), scrollTop);
    const c = await captureAndCompare(page);

    expect([c.width, c.height]).toEqual([800, 600]);
    // A body pinned to the viewport height collapses the hero and lifts the
    // heading by the height it lost.
    expect(c.heroPixel).toEqual(c.heroLivePixel);
    expect(c.ink.shot).not.toBeNull();
    expect(c.ink.cap).not.toBeNull();
    for (const k of ['x', 'y', 'w', 'h'] as const) {
      expect(Math.abs(c.ink.cap![k] - c.ink.shot![k]), `heading ink ${k}`).toBeLessThanOrEqual(HEADING_BOX_TOLERANCE_PX);
    }
    expect(c.mean).toBeLessThan(MEAN_DIFF_MAX);
  });
}

test('a body sized by its margins is captured at its live height, not the viewport\'s', async ({ page }) => {
  await load(page, STRETCH_FIXTURE);
  const c = await captureAndCompare(page);
  // Pinned to the viewport the body is 200px too tall: a third of the frame in
  // the wrong colour, a mean far above the threshold.
  expect(c.mean).toBeLessThan(MEAN_DIFF_MAX);
});
