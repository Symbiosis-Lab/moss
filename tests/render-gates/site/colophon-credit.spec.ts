// `[site].credit` sits before moss's mark on one line of inline text. The text
// wraps like any other and a string with no break in it breaks anywhere, so no
// credit pushes the page sideways; the mark is set on the text's baseline, not
// centred; every link in the line is at least 24px tall to hit; the middle dot
// has the same space on both sides.
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
          markW: markRect.width,
          markH: markRect.height,
          fontSize: parseFloat(getComputedStyle(credit).fontSize),
          dotInLink: !!document.querySelector('.moss-colophon > a > .moss-colophon-dot'),
          label: getComputedStyle(document.querySelector('.moss-colophon-label') as Element).display,
          hits: [...document.querySelectorAll('.moss-colophon a')].map(hit),
        };
      });
      // The mark scales with the credit's own size, so its sprigs match the capitals.
      expect(Math.abs(m.markH - m.fontSize)).toBeLessThanOrEqual(0.5);
      expect(Math.abs(m.markW - m.fontSize)).toBeLessThanOrEqual(0.5);
      expect(m.dotInLink).toBe(true);
      expect(m.label).toBe('none');
      // Same line only when the credit and the mark share it; at 320px a wrapped mark sits below.
      if (width >= 360) expect(Math.abs(m.markBottom - m.baseline)).toBeLessThanOrEqual(2);
      for (const h of m.hits) expect(h).toBeGreaterThanOrEqual(24);
    });

    test('the dot has equal space before it and after it', async ({ page }) => {
      await page.goto(shortUrl, { waitUntil: 'load' });
      const m = await page.evaluate(() => {
        const credit = document.querySelector('.moss-colophon-credit') as HTMLElement;
        const dot = document.querySelector('.moss-colophon-dot') as HTMLElement;
        const mark = document.querySelector('.moss-colophon > a svg') as SVGElement;
        const range = document.createRange();
        range.selectNodeContents(credit);
        const creditRects = [...range.getClientRects()];
        const creditEnd = creditRects[creditRects.length - 1];
        const d = dot.getBoundingClientRect();
        const k = mark.getBoundingClientRect();
        return { before: d.left - creditEnd.right, after: k.left - d.right, sameLine: Math.abs(d.top - creditEnd.top) < 12 && Math.abs(k.bottom - d.bottom) < 12 };
      });
      // A wrapped dot has no neighbour on its line to measure against.
      if (m.sameLine) expect(Math.abs(m.before - m.after)).toBeLessThan(1);
    });
  });
}
