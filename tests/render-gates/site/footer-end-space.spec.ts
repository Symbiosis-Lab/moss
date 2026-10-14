// The space at the end of the page belongs to the footer. moss's colophon is
// the footer's last child and, when visible, closes it: the gap above the mark
// equals the footer's end space and a negative end margin cancels the footer's
// end padding. A site that hides the mark with theme CSS must still get that
// end space, so the footer's last text is never left 24px (the container
// padding) from the page end.
//
// Only a rendered cascade resolves `--moss-footer-end` (a different value at
// phone width) and the margin/padding arithmetic, so this measures boxes.
import { test, expect } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';

const hiddenUrl = `http://localhost:${gatePort('footer-end-space:hidden')}/`;
const defaultUrl = `http://localhost:${gatePort('footer-end-space:default')}/`;

const WIDTHS = [
  { name: 'desktop', width: 1280, height: 800 },
  { name: 'phone', width: 390, height: 800 },
];

// Resolves the custom property to px on the footer itself, where it is set.
const measure = (page) =>
  page.evaluate(() => {
    const footer = document.querySelector('footer.container') as HTMLElement;
    const probe = document.createElement('div');
    probe.style.blockSize = getComputedStyle(footer).getPropertyValue('--moss-footer-end');
    footer.appendChild(probe);
    const end = probe.getBoundingClientRect().height;
    probe.remove();
    const colophon = footer.querySelector(':scope > .moss-colophon') as HTMLElement | null;
    const last = footer.querySelector(':scope > p:last-of-type') as HTMLElement;
    return {
      end,
      footerBottom: footer.getBoundingClientRect().bottom,
      lastBottom: last.getBoundingClientRect().bottom,
      colophonTop: colophon?.getBoundingClientRect().top ?? null,
      colophonBottom: colophon?.getBoundingClientRect().bottom ?? null,
      colophonDisplay: colophon ? getComputedStyle(colophon).display : null,
    };
  });

for (const { name, width, height } of WIDTHS) {
  test.describe(`footer end space at ${name} width`, () => {
    test.use({ viewport: { width, height } });

    test('a colophon hidden by theme CSS leaves the footer its end space', async ({ page }) => {
      await page.goto(hiddenUrl, { waitUntil: 'load' });
      const m = await measure(page);
      expect(m.colophonDisplay).toBe('none');
      expect(m.end).toBeGreaterThan(24);
      expect(m.footerBottom - m.lastBottom).toBeCloseTo(m.end, 1);
    });

    test('a visible colophon closes the footer, with the end space above it', async ({ page }) => {
      await page.goto(defaultUrl, { waitUntil: 'load' });
      const m = await measure(page);
      expect(m.colophonBottom).not.toBeNull();
      expect(m.footerBottom - m.colophonBottom!).toBeCloseTo(0, 1);
      expect(m.colophonTop! - m.lastBottom).toBeCloseTo(m.end, 1);
    });
  });
}
