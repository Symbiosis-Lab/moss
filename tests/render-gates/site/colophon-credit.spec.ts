// `[site].credit` sits before moss's mark on one line of inline text. The text
// wraps like any other and a string with no break in it breaks anywhere, so no
// credit pushes the page sideways; the mark is set on the text's baseline, not
// centred; every link in the line is at least 24px tall to hit; the middle dot
// has the same space on both sides.
import { test, expect, type Page } from '../../support/browser-test';
import { gatePort } from '../../../playwright/gate-ports';

const longUrl = `http://localhost:${gatePort('colophon-credit:long')}/`;
const shortUrl = `http://localhost:${gatePort('colophon-credit:short')}/`;
const bandUrl = `http://localhost:${gatePort('colophon-credit:band')}/`;
const archivalBandUrl = `http://localhost:${gatePort('colophon-credit:archival-band')}/`;

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

// Reached for, the mark is inked for the ground it stands on: moss's own black
// and green on a plain light page, the footer's ink on a band the theme paints.
for (const [site, url, ink, drop] of [
  ['a plain page', shortUrl, 'rgb(0, 0, 0)', 'rgb(79, 112, 49)'],
  ['a footer band', bandUrl, 'rgb(232, 236, 248)', 'rgb(232, 236, 248)'],
] as const) {
  test(`reached for on ${site}, the mark takes the ink that reads there`, async ({ page }) => {
    await page.goto(url, { waitUntil: 'load' });
    await page.hover('.moss-colophon > a');
    await page.waitForTimeout(400); // past the mark's fade
    expect(await markInks(page)).toEqual({ ink, drop });
  });
}

// An archival copy shows the mark in colour at rest; on a band that colour is
// the footer's ink, the same one the mark takes when reached for.
test('at rest on an archival copy with a footer band, the mark takes the footer ink', async ({ page }) => {
  await page.goto(archivalBandUrl, { waitUntil: 'load' });
  expect(await markInks(page)).toEqual({ ink: 'rgb(232, 236, 248)', drop: 'rgb(232, 236, 248)' });
});

function markInks(page: Page) {
  return page.evaluate(() => {
    const mark = document.querySelector('.moss-colophon > a .moss-mark') as Element;
    return { ink: getComputedStyle(mark).color, drop: getComputedStyle(mark.querySelector('[fill="#4f7031"]') as Element).fill };
  });
}
