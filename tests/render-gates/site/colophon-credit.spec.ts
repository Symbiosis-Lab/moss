// `[site].credit` sits before moss's mark on one line of inline text. The text
// wraps like any other and a string with no break in it breaks anywhere, so no
// credit pushes the page sideways; the mark is set on the text's baseline, not
// centred; every link in the line is at least 24px tall to hit.
import { test, expect } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';

const longUrl = `http://localhost:${gatePort('colophon-credit:long')}/`;
const shortUrl = `http://localhost:${gatePort('colophon-credit:short')}/`;

for (const width of [1280, 390, 360, 320]) {
  test.describe(`credit at ${width}px`, () => {
    test.use({ viewport: { width, height: 800 } });

    test('an unbreakable credit wraps and never scrolls the page sideways', async ({ page }) => {
      await page.goto(longUrl, { waitUntil: 'load' });
      const m = await page.evaluate(() => ({
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      }));
      expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
    });

    test('the mark stands on the text baseline and links are 24px tall', async ({ page }) => {
      await page.goto(shortUrl, { waitUntil: 'load' });
      const m = await page.evaluate(() => {
        const credit = document.querySelector('.moss-colophon-credit') as HTMLElement;
        const mark = document.querySelector('.moss-colophon > a svg') as SVGElement;
        const probe = document.createElement('span');
        probe.style.cssText = 'display:inline-block;width:0;height:0;vertical-align:baseline';
        credit.appendChild(probe);
        const baseline = probe.getBoundingClientRect().bottom;
        probe.remove();
        const markRect = mark.getBoundingClientRect();
        const hit = (a: Element) => {
          return a.getBoundingClientRect().height; // an inline box's rect already includes its padding
        };
        return {
          markBottom: markRect.bottom,
          baseline,
          markSize: markRect.width,
          dotInLink: !!document.querySelector('.moss-colophon > a > .moss-colophon-dot'),
          label: getComputedStyle(document.querySelector('.moss-colophon-label') as Element).display,
          hits: [...document.querySelectorAll('.moss-colophon a')].map(hit),
        };
      });
      expect(m.markSize).toBe(16);
      expect(m.dotInLink).toBe(true);
      expect(m.label).toBe('none');
      // Same line only when the credit and the mark share it; at 320px a wrapped mark sits below.
      if (width >= 360) expect(Math.abs(m.markBottom - m.baseline)).toBeLessThanOrEqual(2);
      for (const h of m.hits) expect(h).toBeGreaterThanOrEqual(24);
    });
  });
}
