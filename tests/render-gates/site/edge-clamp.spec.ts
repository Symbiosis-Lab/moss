/**
 * Edge-clamp gate — every floating surface stays inside the visible band.
 *
 * One assertion, applied to three surfaces at two widths in two engines: the
 * box the reader is meant to read is fully on screen. Nothing here asserts a
 * *position*; that would pin an implementation detail and break on any spacing
 * change. It asserts containment, which is the promise.
 *
 * Why an engine is required. Each surface is placed from measurements — the
 * anchor's `getBoundingClientRect()`, the surface's own width, the width of the
 * visible band — and jsdom returns 0 for all three. A unit test can check the
 * arithmetic (`crates/moss-build/src/js-src/site/__tests__/viewport.test.ts`)
 * but cannot see that a
 * `display: none` box measured 0 and centred itself on nothing, which is the
 * shape of the bug this gate exists for: a selection popover in the left margin
 * of a phone put half its buttons off-screen, under a fully green suite.
 *
 * The gutter is 8px (12 for hints), so a surface may legitimately sit close to
 * an edge. These assertions use 0 as the bound — being *inside* is the promise;
 * the gutter is a comfort margin the unit tests pin exactly.
 */
import { test, expect, type Page } from '@playwright/test';

/** Phone and laptop. The failures are width-dependent, so both are load-bearing. */
const WIDTHS = [
  { label: 'phone', size: { width: 390, height: 844 } },
  { label: 'laptop', size: { width: 1280, height: 800 } },
] as const;

interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
  width: number;
}

/** Assert a surface is fully within the band the reader can see. */
function expectOnScreen(box: Box | null, band: { width: number; height: number }, what: string) {
  expect(box, `${what} should be rendered`).not.toBeNull();
  const b = box!;
  expect(b.width, `${what} should have a real width`).toBeGreaterThan(0);
  expect(b.left, `${what} runs off the left edge`).toBeGreaterThanOrEqual(0);
  expect(b.right, `${what} runs off the right edge`).toBeLessThanOrEqual(band.width);
  expect(b.top, `${what} runs off the top edge`).toBeGreaterThanOrEqual(0);
  expect(b.bottom, `${what} runs off the bottom edge`).toBeLessThanOrEqual(band.height);
}

/** The visible band, measured the way the modules under test measure it. */
async function band(page: Page): Promise<{ width: number; height: number }> {
  return page.evaluate(() => ({
    width: document.documentElement.clientWidth,
    height: document.documentElement.clientHeight,
  }));
}

async function rectOf(page: Page, selector: string): Promise<Box | null> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width };
  }, selector);
}

/**
 * Select the text inside `#id` and tell the page a mouse was released, which is
 * what the desktop popover listens for. A synthetic Range is the only way to
 * select a specific node reliably across both engines — a drag would depend on
 * where the text happened to wrap.
 */
async function selectAndRelease(page: Page, id: string): Promise<void> {
  await page.evaluate((elementId) => {
    const node = document.getElementById(elementId)!;
    const range = document.createRange();
    range.selectNodeContents(node);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
  }, id);
  await page.waitForFunction(() => !!document.querySelector('.sel-popover.visible'));
}

for (const { label, size } of WIDTHS) {
  test.describe(`${label} — ${size.width}px`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize(size);
      await page.goto('/playwright/fixtures/edge-clamp/live-css.html');
      await page.waitForFunction(() => (window as any).__harnessReady === true);
    });

    test('the selection popover stays on screen in the left margin', async ({ page }) => {
      // The reported bug, reproduced: 全文 is the first thing on its line, hard
      // against the left edge, and the popover is centred on it.
      await selectAndRelease(page, 'sel-left');
      expectOnScreen(await rectOf(page, '.sel-popover'), await band(page), 'selection popover');
    });

    test('the selection popover stays on screen at the right edge', async ({ page }) => {
      await selectAndRelease(page, 'sel-right');
      expectOnScreen(await rectOf(page, '.sel-popover'), await band(page), 'selection popover');
    });

    test('the selection popover does not cover the words it is about', async ({ page }) => {
      // Containment alone would be satisfied by pinning the popover to the top
      // gutter — over the selection. The flip is what makes it useful.
      await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
      await selectAndRelease(page, 'sel-top');

      const popover = (await rectOf(page, '.sel-popover'))!;
      const anchor = (await rectOf(page, '#sel-top'))!;
      expectOnScreen(popover, await band(page), 'selection popover');
      const overlaps = popover.bottom > anchor.top && popover.top < anchor.bottom;
      expect(overlaps, 'popover overlaps the selected text').toBe(false);
    });

    test('the link preview card stays on screen at either edge', async ({ page }) => {
      for (const anchor of ['a.wikilink.at-left', 'a.wikilink.at-right']) {
        await page.hover(anchor);
        await page.waitForFunction(() => !!document.querySelector('.moss-preview-popup.visible'));
        expectOnScreen(
          await rectOf(page, '.moss-preview-popup'),
          await band(page),
          `link preview from ${anchor}`,
        );
        await page.mouse.move(0, 0);
      }
    });

    test('the hover hint stays on screen at either edge', async ({ page }) => {
      for (const anchor of ['button.at-right', 'button.at-left']) {
        await page.hover(anchor);
        // The pill is a `::after` pseudo-element, so it has no node to measure.
        // Its box comes from the host's own box plus the offset the module set.
        const box = await page.evaluate((sel) => {
          const host = document.querySelector(sel) as HTMLElement;
          const pill = getComputedStyle(host, '::after');
          const px = (v: string) => parseFloat(v) || 0;
          const width =
            px(pill.width) +
            px(pill.paddingLeft) +
            px(pill.paddingRight) +
            px(pill.borderLeftWidth) +
            px(pill.borderRightWidth);
          const hostRect = host.getBoundingClientRect();
          const dx = px(getComputedStyle(host).getPropertyValue('--moss-hint-x'));
          const left = hostRect.left + dx;
          // Vertical containment is not this module's job — the pill sits just
          // below its host and the hosts here are near the top — so the
          // reported top/bottom are the host's own, which are on screen by
          // construction.
          return {
            left,
            right: left + width,
            top: hostRect.top,
            bottom: hostRect.bottom,
            width,
          };
        }, anchor);
        expectOnScreen(box, await band(page), `hover hint on ${anchor}`);
        await page.mouse.move(0, 0);
      }
    });
  });
}
