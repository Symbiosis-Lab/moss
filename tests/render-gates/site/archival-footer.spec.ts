// An archival site's footer: the edition block is not a footer.md column, so
// the footer keeps the article's box and the block fills it; the moss mark
// sits at the end of the edition line, inked, rather than in a band below.
import { test, expect, Page } from '@playwright/test';

const PAGES = ['/', '/prints/pier/'];
const WIDTHS = [
  { name: 'desktop', width: 1440, height: 900 },
  { name: 'phone', width: 390, height: 800 },
];

const measure = (page: Page) =>
  page.evaluate(() => {
    const article = document.querySelector('main article')!;
    const footer = document.querySelector('footer.container')!;
    const contentBox = (el: Element) => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return { left: r.left + parseFloat(cs.paddingLeft), right: r.right - parseFloat(cs.paddingRight) };
    };
    const line = document.querySelector('.moss-edition-line')!;
    const lineBox = line.getBoundingClientRect();
    const firstLineCentre = lineBox.top + parseFloat(getComputedStyle(line).lineHeight) / 2;
    const icon = document.querySelector('.moss-colophon-icon')!;
    const iconBox = icon.getBoundingClientRect();
    const fs = getComputedStyle(footer);
    const dividerBottom = footer.getBoundingClientRect().top + parseFloat(fs.paddingTop)
      + parseFloat(getComputedStyle(footer, '::before').borderTopWidth);
    return {
      footerDisplay: fs.display,
      article: contentBox(article),
      footer: contentBox(footer),
      edition: document.querySelector('.moss-edition')!.getBoundingClientRect().width,
      dividerToLine: lineBox.top - dividerBottom,
      iconRight: iconBox.right,
      iconCentreOffLine: iconBox.top + iconBox.height / 2 - firstLineCentre,
      colophonHeight: document.querySelector('.moss-colophon')!.getBoundingClientRect().height,
      ink: getComputedStyle(icon).color,
      drop: getComputedStyle(icon.querySelector('path[fill="#4f7031"]')!).fill,
    };
  });

for (const { name, width, height } of WIDTHS) {
  test.describe(`archival footer at ${name} width`, () => {
    test.use({ viewport: { width, height } });
    for (const path of PAGES) {
      test(`${path}: the footer keeps the column and the mark closes the edition line`, async ({ page }) => {
        await page.goto(path, { waitUntil: 'load' });
        const m = await measure(page);
        expect(m.footerDisplay, 'a footer columns grid means the edition block matched the footer.md column rule').not.toBe('grid');
        expect(Math.abs(m.footer.left - m.article.left)).toBeLessThanOrEqual(1);
        expect(Math.abs(m.footer.right - m.article.right)).toBeLessThanOrEqual(1);
        expect(m.edition, 'the edition block takes the column, less the mark beside it').toBeGreaterThanOrEqual(m.footer.right - m.footer.left - 48);
        expect(m.dividerToLine).toBeLessThanOrEqual(24);
        expect(Math.abs(m.iconRight - m.footer.right)).toBeLessThanOrEqual(1);
        expect(Math.abs(m.iconCentreOffLine)).toBeLessThanOrEqual(1);
        expect(m.colophonHeight, 'no band of its own').toBeLessThanOrEqual(40);
        expect(m.ink, 'the black half at rest').toBe('rgb(0, 0, 0)');
        expect(m.drop, 'the green tips at rest').toBe('rgb(79, 112, 49)');
      });
    }
  });
}
