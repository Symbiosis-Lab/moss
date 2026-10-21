// A footer.md that ends in a `:::hero` closes the page: the picture spans the
// viewport (whatever the body's own padding), nothing paints below it, and its
// words stay on it at every width, in both themes.
import { test, expect, Page } from '@playwright/test';
import { worstCaseContrast } from '../../e2e/helpers/wcag-contrast';

const PAGES = [
  { name: 'a short page', path: '/' },
  { name: 'a page with pictures in its contents', path: '/essays/long/' },
];
const WIDTHS = [
  { name: 'desktop', width: 1360, height: 800 },
  { name: 'phone', width: 400, height: 800 },
  { name: 'narrow desktop', width: 900, height: 800 },
];
const SCHEMES = ['light', 'dark'] as const;

const measure = (page: Page) =>
  page.evaluate(() => {
    const box = (el: Element) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top + scrollY, bottom: r.bottom + scrollY };
    };
    const hero = document.querySelector('footer .moss-hero-closing')!;
    const content = hero.querySelector('.moss-hero-content')!;
    const visible = [...hero.querySelectorAll('[data-hero-variant]')].filter((v) => getComputedStyle(v).display !== 'none');
    const cs = getComputedStyle(content);
    return {
      viewport: document.documentElement.clientWidth,
      docHeight: document.documentElement.scrollHeight,
      hero: box(hero),
      footer: box(document.querySelector('footer')!),
      heading: box(content.querySelector('h2')!),
      link: box(content.querySelector('a')!),
      colophon: box(document.querySelector('footer .moss-colophon')!),
      visibleSrc: visible.map((v) => (v.querySelector('img') ?? v).getAttribute('src') ?? ''),
      panel: cs.backgroundColor,
      text: cs.color,
      afterFooter: [...document.querySelectorAll('footer ~ *')].filter((e) => {
        const s = getComputedStyle(e);
        return s.display !== 'none' && s.position !== 'fixed' && e.getBoundingClientRect().height > 0;
      }).length,
    };
  });

for (const scheme of SCHEMES) {
  for (const { name: wname, width, height } of WIDTHS) {
    test.describe(`closing picture, ${scheme}, ${wname}`, () => {
      test.use({ viewport: { width, height }, colorScheme: scheme });

      for (const { name, path } of PAGES) {
        test(`on ${name}: edge to edge, last on the page, words on the picture`, async ({ page }) => {
          await page.goto(path, { waitUntil: 'load' });
          const m = await measure(page);

          expect(m.hero.left, 'picture starts at the viewport edge').toBeCloseTo(0, 0);
          expect(m.hero.right, 'picture reaches the other edge').toBeCloseTo(m.viewport, 0);
          expect(m.hero.bottom, 'nothing is below the picture').toBeCloseTo(m.docHeight, 0);
          expect(m.footer.bottom).toBeCloseTo(m.hero.bottom, 0);
          expect(m.afterFooter, 'no box follows the footer').toBe(0);

          for (const part of ['heading', 'link', 'colophon'] as const) {
            const r = m[part];
            expect(r.left, `${part} left`).toBeGreaterThanOrEqual(m.hero.left - 1);
            expect(r.right, `${part} right`).toBeLessThanOrEqual(m.hero.right + 1);
            expect(r.top, `${part} top`).toBeGreaterThanOrEqual(m.hero.top - 1);
            expect(r.bottom, `${part} bottom`).toBeLessThanOrEqual(m.hero.bottom + 1);
          }
          // `align=start`: the words are at the top, the colophon at the foot.
          expect(m.heading.top - m.hero.top).toBeLessThan(m.hero.bottom - m.heading.bottom);

          expect(m.visibleSrc.length, 'one picture of the pair is shown').toBe(1);
          expect(m.visibleSrc[0].includes('dark'), `the ${scheme} picture is the one shown`).toBe(scheme === 'dark');

          const { overBlack, overWhite } = worstCaseContrast(m.panel, m.text);
          expect(Math.min(overBlack, overWhite), `words ${m.text} on panel ${m.panel}`).toBeGreaterThanOrEqual(4.5);
        });
      }
    });
  }
}

test('the colophon does not sit on the words', async ({ page }) => {
  await page.setViewportSize({ width: 400, height: 800 });
  await page.goto('/', { waitUntil: 'load' });
  const m = await measure(page);
  expect(m.link.bottom).toBeLessThanOrEqual(m.colophon.top + 1);
});

const shownPictures = (page: Page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('footer .moss-hero-closing [data-hero-variant]')]
      .filter((v) => getComputedStyle(v).display !== 'none')
      .map((v) => v.getAttribute('data-hero-variant')),
  );

// The theme toggle sets `data-theme` on <html> whatever the system says, so the
// picture follows the attribute, not the media query.
for (const [system, chosen] of [['light', 'dark'], ['dark', 'light']] as const) {
  test(`the toggle wins over the system: system ${system}, theme ${chosen}`, async ({ browser }) => {
    const context = await browser.newContext({ colorScheme: system, viewport: { width: 1360, height: 800 } });
    const page = await context.newPage();
    await page.goto('/', { waitUntil: 'load' });
    expect(await shownPictures(page)).toEqual([system]);
    await page.evaluate((t) => document.documentElement.setAttribute('data-theme', t), chosen);
    expect(await shownPictures(page)).toEqual([chosen]);
    await context.close();
  });
}

// In a right-to-left page the picture still spans the viewport.
for (const { name, width } of [{ name: 'phone', width: 400 }, { name: 'narrow desktop', width: 900 }]) {
  test(`right-to-left page at ${name} width: the picture still spans the viewport`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/essays/long/', { waitUntil: 'load' });
    await page.evaluate(() => { document.documentElement.dir = 'rtl'; });
    const m = await measure(page);
    expect(m.hero.left).toBeCloseTo(0, 0);
    expect(m.hero.right).toBeCloseTo(m.viewport, 0);
    expect(m.hero.bottom).toBeCloseTo(m.docHeight, 0);
  });
}
