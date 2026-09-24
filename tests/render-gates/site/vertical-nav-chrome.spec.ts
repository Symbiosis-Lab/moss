/**
 * Nav chrome under vertical writing — geometric claims neither vitest nor a
 * Rust test can see:
 *
 *   1. The font selector's anchor glyph (`.font-trigger`) never moves, in
 *      either writing mode, at any saved font scale, during or after the
 *      open/close morph. Before the font-morph redesign (font-morph.md),
 *      `open()`/`close()` measured and wrote `trigger.style.insetInlineStart` every
 *      time, sliding the anchor itself to the active button's slot whenever
 *      the saved scale wasn't the default — the same displacement bug an
 *      earlier fix (`ae9a43ba8`) repaired on the wrong axis but never
 *      removed. The redesign makes the trigger's position a plain CSS
 *      constant with no runtime write at all; `.font-pill` carries the
 *      computed offset instead, growing from the anchor's fixed point. This
 *      gate samples `.font-trigger`'s `getBoundingClientRect()` at rAF
 *      intervals across the whole open and close animation — not just the
 *      first/last frame, since an intermediate-frame regression is
 *      invisible to an endpoint-only assertion (the SVG-morph case in
 *      `claude-memory/memory/green-at-the-wrong-layer-is-not-evidence.md`).
 *   2. An endpoint check layered on top of (1) (font-morph round 2, defect
 *      A — a gap the sampling above can't see because it only ever compares
 *      the trigger to itself): `.font-trigger`'s cross-axis center lines up
 *      with `.date-line .date`'s cross-axis center — the trigger glyph
 *      continues the date's own text column instead of sitting offset from
 *      it (`.font-anchor`'s `margin-inline-start` regressing to a physical
 *      `margin-left` reproduces a 6px+ delta, vertical-rl only). Font-morph
 *      round 2 found three more physical-property defects in the same
 *      cluster alongside this one — the trigger/pill padding and the
 *      pill's `transform-origin` — folded into the same CSS fix but
 *      without dedicated gates of their own, since nothing here isolates
 *      padding shape or a scale pivot point the way this assertion
 *      isolates the margin. A fourth defect in the same
 *      round, the pill's active button not landing under the trigger's
 *      vacated slot, stays open: the prototyped fix (neutralize the pill's
 *      transform during measurement) genuinely closes the gap in
 *      vertical-rl at the default scale but leaves a residual few-to-tens-
 *      of-px gap in horizontal-tb and at non-default scales — a second,
 *      distinct cause (font-metric timing, not the transform collapse) that
 *      the round-2 design explicitly deferred a fix behind its own
 *      prototype-and-ablate pass. That pass ran here and did not clear, so
 *      no fix and no gate for it landed in this round.
 *   3. The masthead breadcrumb does not fold when the trail actually fits.
 *      `masthead-fold.ts` charged the fold decision against `.nav-left`'s
 *      physical `clientWidth`, which under vertical-rl is the trail's CROSS
 *      axis (small — just wide enough for the widest crumb), not the inline
 *      axis it actually lays its crumbs out along (large — the page's own
 *      column height).
 *
 * The masthead trail's assertions read the `hidden` DOM property rather than
 * Playwright's `toBeVisible()` — investigating a chromium-only zero-height
 * render of `.breadcrumb-segment` in `data-fold-managed` mode found it
 * reproduces identically with the fold's own JS entirely absent (forced via
 * `removeAttribute`/`setAttribute` in a scratch page), and only under
 * `vertical-rl`; webkit renders the same markup at its correct size. That is
 * a chromium intrinsic-sizing quirk for a `flex-shrink: 0; min-inline-size:
 * 0` item whose only child is a clipped block, orthogonal to the font
 * selector's defect here (it is present with or without this branch's fix,
 * and present with or without JS running at all) — not something a per-nav-
 * file change should paper over. `hidden` is the actual thing masthead-
 * fold.ts decides; the pixel box is a chromium rendering detail this gate
 * doesn't own.
 *
 * Harness: playwright/fixtures/vertical-nav-chrome/live-css.html
 * Config:  playwright/vertical-nav-chrome.config.ts
 * Run via: pnpm run test:render-gates vertical-nav-chrome
 */
import { test, expect, type Page } from '@playwright/test';

const URL = '/playwright/fixtures/vertical-nav-chrome/live-css.html';

const rect = (page: Page, sel: string) =>
  page.locator(sel).first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
  });

/** Sample `sel`'s rect on every animation frame for `ms` milliseconds. */
const sampleRectDuringAnimation = (page: Page, sel: string, ms: number) =>
  page.evaluate(
    ([selector, duration]) =>
      new Promise<{ left: number; top: number }[]>((resolve) => {
        const el = document.querySelector(selector as string) as HTMLElement;
        const samples: { left: number; top: number }[] = [];
        const start = performance.now();
        function tick() {
          const r = el.getBoundingClientRect();
          samples.push({ left: r.left, top: r.top });
          if (performance.now() - start < (duration as number)) {
            requestAnimationFrame(tick);
          } else {
            resolve(samples);
          }
        }
        requestAnimationFrame(tick);
      }),
    [sel, ms] as const,
  );

const SCALES = ['small', '', 'large', 'xlarge'];

/** Load the harness with a saved font scale and writing mode already set, past the point .font-anchor exists. */
const gotoWithScale = async (page: Page, scale: string, typesetting: 'horizontal' | 'vertical') => {
  await page.addInitScript(
    ([savedScale, mode]) => {
      if (savedScale) localStorage.setItem('moss-font-scale', savedScale as string);
      if (mode === 'horizontal') {
        document.addEventListener('DOMContentLoaded', () => {
          document.body.removeAttribute('data-typesetting');
        });
      }
    },
    [scale, typesetting] as const,
  );
  await page.goto(URL);
  await page.waitForSelector('.font-anchor');
  if (typesetting === 'horizontal') {
    await page.waitForFunction(() => !document.body.hasAttribute('data-typesetting'));
  }
};

/** Cross-axis center of a rect: vertical axis under horizontal-tb, horizontal axis under vertical-rl. */
const crossAxisCenter = (r: { left: number; right: number; top: number; bottom: number }, typesetting: 'horizontal' | 'vertical') =>
  typesetting === 'vertical' ? (r.left + r.right) / 2 : (r.top + r.bottom) / 2;

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
});

for (const typesetting of ['horizontal', 'vertical'] as const) {
  test.describe(`font selector anchor under ${typesetting === 'vertical' ? 'vertical-rl' : 'horizontal-tb'}`, () => {
    for (const scale of SCALES) {
      test(`the trigger never moves, saved scale ${JSON.stringify(scale)}`, async ({ page }) => {
        await gotoWithScale(page, scale, typesetting);

        // Playwright scrolls a target into view before clicking. WebKit's
        // vertical scroll container can move even an already visible target;
        // establish that interaction position before measuring the morph.
        await page.locator('.font-trigger').scrollIntoViewIfNeeded();
        const rest = await rect(page, '.font-trigger');

        // Open: sample every frame across the whole grow animation
        // (280ms) plus slack, not just before/after.
        const [openSamples] = await Promise.all([
          sampleRectDuringAnimation(page, '.font-trigger', 400),
          page.click('.font-trigger'),
        ]);
        await page.waitForFunction(() => document.getElementById('fontPill')?.classList.contains('visible'));
        for (const s of openSamples) {
          expect(Math.abs(s.left - rest.left)).toBeLessThanOrEqual(1);
          expect(Math.abs(s.top - rest.top)).toBeLessThanOrEqual(1);
        }
        const afterOpen = await rect(page, '.font-trigger');
        expect(Math.abs(afterOpen.left - rest.left)).toBeLessThanOrEqual(1);
        expect(Math.abs(afterOpen.top - rest.top)).toBeLessThanOrEqual(1);

        // Close: same sampling across the whole animation.
        const [closeSamples] = await Promise.all([
          sampleRectDuringAnimation(page, '.font-trigger', 400),
          page.mouse.click(5, 5),
        ]);
        await page.waitForFunction(() => !document.getElementById('fontPill')?.classList.contains('visible'));
        for (const s of closeSamples) {
          expect(Math.abs(s.left - rest.left)).toBeLessThanOrEqual(1);
          expect(Math.abs(s.top - rest.top)).toBeLessThanOrEqual(1);
        }
        const afterClose = await rect(page, '.font-trigger');
        expect(Math.abs(afterClose.left - rest.left)).toBeLessThanOrEqual(1);
        expect(Math.abs(afterClose.top - rest.top)).toBeLessThanOrEqual(1);
      });
    }

    for (const scale of ['', 'large']) {
      test(`the trigger continues the date's own column, saved scale ${JSON.stringify(scale)}`, async ({ page }) => {
        await gotoWithScale(page, scale, typesetting);

        const dateRect = await rect(page, '.date-line .date');
        const triggerRect = await rect(page, '.font-trigger');
        expect(Math.abs(crossAxisCenter(triggerRect, typesetting) - crossAxisCenter(dateRect, typesetting))).toBeLessThanOrEqual(1);
      });
    }
  });
}

test.describe('masthead breadcrumb under vertical-rl', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(URL);
    await page.waitForSelector('.font-anchor');
  });

  test('a trail that fits does not fold: the room is measured on the inline axis, not the cross axis', async ({ page }) => {
    // The trail has real room: under vertical-rl the masthead's flex row
    // runs down the page (the inline axis), and the fixture's viewport
    // gives it hundreds of pixels of that axis to work with — far more than
    // three short CJK crumbs need. The bug this pins charged the fold
    // against the trail's physical (cross-axis) width instead, which is
    // only as wide as the widest crumb, and folded every time.
    await page.waitForFunction(() => document.querySelector('.nav-left')?.hasAttribute('data-fold-managed'));

    const geometry = await page.evaluate(() => {
      const navLeft = document.querySelector('.nav-left') as HTMLElement;
      return {
        writingMode: getComputedStyle(navLeft).writingMode,
        clientWidth: navLeft.clientWidth,
        clientHeight: navLeft.clientHeight,
      };
    });
    expect(geometry.writingMode).toBe('vertical-rl');
    // The old bug's own measurement (clientWidth) is small — proving the
    // fixture reproduces the actual failure mode, not a case where both
    // axes happen to agree.
    expect(geometry.clientWidth).toBeLessThan(100);
    expect(geometry.clientHeight).toBeGreaterThan(300);

    await expect(page.locator('.moss-breadcrumb-more')).toBeHidden();
    const hiddenFlags = await page.$$eval(
      '.nav-left .site-name, .nav-left .breadcrumb-segment',
      (nodes) => nodes.map((n) => (n as HTMLElement).hidden),
    );
    expect(hiddenFlags.length).toBeGreaterThan(0);
    expect(hiddenFlags).toEqual(hiddenFlags.map(() => false));
  });
});
