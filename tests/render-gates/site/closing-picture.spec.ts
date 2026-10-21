// A footer.md that ends in a `:::hero` closes the page: the picture spans the
// viewport (whatever the body's own padding), nothing paints below it, and its
// words stay on it at every width, in both themes.
import { test, expect, Page } from '@playwright/test';
import { contrastRatio, luminanceOfCss, parseColor, relativeLuminance, worstCaseContrast } from '../../e2e/helpers/wcag-contrast';
import { CREDIT, GROUND, SKY, SKY_NIGHT } from '../../e2e/helpers/closing-picture-site';
import { gatePort } from '../../../playwright/gate-ports';

const lum = ([r, g, b]: readonly number[]) => relativeLuminance(r, g, b);
const contrastWith = (ink: string, ground: readonly number[]) => contrastRatio(luminanceOfCss(ink), lum(ground));

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
      colophonInk: getComputedStyle(document.querySelector('footer .moss-colophon a')!).color,
      credit: box(hero.querySelector('.moss-hero-credit')!),
      creditInk: getComputedStyle(hero.querySelector('.moss-hero-credit')!).color,
      creditPanel: getComputedStyle(hero.querySelector('.moss-hero-credit')!).backgroundColor,
      creditSize: parseFloat(getComputedStyle(hero.querySelector('.moss-hero-credit')!).fontSize),
      wordsSize: parseFloat(getComputedStyle(content.querySelector('p')!).fontSize),
      content: box(content),
      pictureRatio: (() => {
        const img = visible[0].querySelector('img') ?? (visible[0] as HTMLImageElement);
        return Number(img.getAttribute('height')) / Number(img.getAttribute('width'));
      })(),
      scrim: getComputedStyle(hero, '::before').content,
      strayCaption: document.querySelectorAll('footer .moss-hero-caption').length,
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

          // The picture is flat, so its worst case under the words is its sky and
          // under the foot its ground: the words are in ink with no panel, and
          // the ink clears AA against them.
          const [sky, ground] = scheme === 'dark' ? [SKY_NIGHT, [8, 10, 14]] : [SKY, GROUND];
          expect(parseColor(m.panel).a, 'no panel behind the words').toBe(0);
          expect(m.scrim, 'no darkening laid over the picture').toBe('none');
          expect(contrastWith(m.text, sky), `words ${m.text} on the sky`).toBeGreaterThanOrEqual(4.5);

          // The credit: the caption, small, inside the picture at its foot on the end side.
          expect(m.strayCaption, 'the caption is not left below the picture').toBe(0);
          expect(m.credit.bottom, 'credit inside the picture').toBeLessThanOrEqual(m.hero.bottom + 1);
          expect(m.credit.right, 'credit on the end side').toBeLessThanOrEqual(m.hero.right + 1);
          expect(m.credit.left, 'credit on the end side').toBeGreaterThan(m.hero.left + (m.hero.right - m.hero.left) * 0.05);
          expect(m.credit.top, 'credit is below the words').toBeGreaterThan(m.heading.bottom);
          expect(m.credit.bottom, 'credit is above the colophon row').toBeLessThanOrEqual(m.colophon.top + 1);
          expect(m.creditSize, 'credit is smaller than the words').toBeLessThan(m.wordsSize);
          expect(parseColor(m.creditPanel).a, 'ink needs no panel at the foot').toBe(0);
          expect(contrastWith(m.creditInk, ground), `credit ${m.creditInk} on the foot`).toBeGreaterThanOrEqual(4.5);
          expect(contrastWith(m.colophonInk, ground), `colophon ${m.colophonInk} on the foot`).toBeGreaterThanOrEqual(4.5);
        });
      }
    });
  }
}

// The phone keeps the subject: the words take the top, and the bottom 40% of the
// picture's box holds no panel, no darkening and none of the words, with the area
// below the words at least half the picture's own height at that width.
for (const width of [390, 400]) {
  test(`phone ${width}: the area opposite the words is clear and half the picture tall`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/', { waitUntil: 'load' });
    const m = await measure(page);
    const box = m.hero.bottom - m.hero.top;
    expect(m.content.bottom, 'the words end above the bottom 40%').toBeLessThanOrEqual(m.hero.top + box * 0.6);
    expect(m.scrim).toBe('none');
    expect(parseColor(m.panel).a).toBe(0);
    const pictureHeight = m.viewport * m.pictureRatio;
    expect(m.hero.bottom - m.content.bottom, 'room for the picture below the words').toBeGreaterThanOrEqual(pictureHeight * 0.5);
  });
}

test('the credit is the caption, small, and links stay legible', async ({ page }) => {
  await page.goto('/', { waitUntil: 'load' });
  await expect(page.locator('footer .moss-hero-closing .moss-hero-credit')).toHaveText(CREDIT);
});

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

// An ordinary hero keeps its panel unless it asks to be measured; one that asks
// loses the panel over a quiet sky and keeps it over a hatched one.
for (const width of [1360, 390]) {
  test.describe(`ordinary heroes at ${width}px`, () => {
    test.use({ viewport: { width, height: 800 }, colorScheme: 'light' });
    const words = (page: Page) =>
      page.evaluate(() => {
        const c = document.querySelector('.moss-hero .moss-hero-content')!;
        const cs = getComputedStyle(c);
        return { text: cs.color, panel: cs.backgroundColor, scrim: getComputedStyle(c.parentElement!, '::before').content };
      });

    test('panel=auto over a quiet sky: ink, no panel', async ({ page }) => {
      await page.goto('/hero/quiet/', { waitUntil: 'load' });
      const m = await words(page);
      expect(parseColor(m.panel).a).toBe(0);
      expect(contrastWith(m.text, SKY)).toBeGreaterThanOrEqual(4.5);
    });
    test('panel=auto over a hatched sky keeps the panel', async ({ page }) => {
      await page.goto('/hero/busy/', { waitUntil: 'load' });
      expect(parseColor((await words(page)).panel).a).toBeGreaterThan(0.5);
    });
    test('a hero that does not ask is unchanged', async ({ page }) => {
      await page.goto('/hero/plain/', { waitUntil: 'load' });
      const m = await words(page);
      expect(parseColor(m.panel).a).toBeGreaterThan(0.5);
      expect(await page.locator('body > .moss-hero[data-words], body > .moss-hero[data-words-night]').count(), 'nothing measured').toBe(0);
    });
  });
}

// The words are measured where their box sits at each layout. A dark patch over the
// far end of the words' rows spares the wide layout (a box of about 47% at the start)
// but not the narrower ones, where the box reaches it or runs edge to edge.
for (const [width, panel] of [[1360, false], [1000, true], [390, true]] as const) {
  test(`a dark patch at the far end of the words' rows ${panel ? 'keeps' : 'spares'} the panel at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/hero/patch/', { waitUntil: 'load' });
    const bg = await page.evaluate(() => getComputedStyle(document.querySelector('.moss-hero .moss-hero-content')!).backgroundColor);
    if (panel) expect(parseColor(bg).a).toBeGreaterThan(0.5);
    else expect(parseColor(bg).a).toBe(0);
  });
}

// A busy foot: the credit takes a small panel of its own and stays legible in
// both themes, whatever tone the picture is.
const BUSY = `http://localhost:${gatePort('closing-picture:busy')}/`;
for (const scheme of SCHEMES) {
  for (const width of [1360, 390]) {
    test(`busy foot, ${scheme}, ${width}px: the credit is legible on its own panel`, async ({ browser }) => {
      const context = await browser.newContext({ colorScheme: scheme, viewport: { width, height: 800 } });
      const page = await context.newPage();
      await page.goto(BUSY, { waitUntil: 'load' });
      const credit = await page.evaluate(() => {
        const el = document.querySelector('footer .moss-hero-closing .moss-hero-credit')!;
        const cs = getComputedStyle(el);
        const hero = el.parentElement!.getBoundingClientRect();
        const r = el.getBoundingClientRect();
        return { ink: cs.color, panel: cs.backgroundColor, inside: r.bottom <= hero.bottom + 1 && r.right <= hero.right + 1 };
      });
      expect(credit.inside).toBe(true);
      expect(parseColor(credit.panel).a, 'a panel behind the credit').toBeGreaterThan(0.5);
      const { overBlack, overWhite } = worstCaseContrast(credit.panel, credit.ink);
      expect(Math.min(overBlack, overWhite), `credit ${credit.ink} on ${credit.panel}`).toBeGreaterThanOrEqual(4.5);
      await context.close();
    });
  }
}

// The picture is the point: from 48rem the box takes the picture's own shape at
// the window's width, so the whole picture shows, even taller than the window.
// An author's `--moss-hero-max-height` caps it, keeping the side away from the words.
const SKY_PAGE = `http://localhost:${gatePort('closing-picture:sky')}/`;
const boxOf = (page: Page) =>
  page.evaluate(() => {
    const hero = document.querySelector('footer .moss-hero-closing')!;
    const r = hero.getBoundingClientRect();
    const img = [...hero.querySelectorAll('img')].find((i) => getComputedStyle(i).display !== 'none')!;
    return {
      width: r.width,
      height: r.height,
      ratio: Number(img.getAttribute('height')) / Number(img.getAttribute('width')),
      position: getComputedStyle(img).objectPosition,
    };
  });

for (const width of [1360, 900]) {
  test(`desktop ${width}px: the box is the picture's shape, so the whole picture shows`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/', { waitUntil: 'load' });
    const b = await boxOf(page);
    expect(Math.abs(b.height - b.width * b.ratio), `box ${b.height} for a ${b.width}px wide picture of ratio ${b.ratio}`).toBeLessThanOrEqual(1);
  });
}

test.describe('a picture taller than the window', () => {
  test('1360x800: the box is the whole picture, taller than the window', async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 800 });
    await page.goto(SKY_PAGE, { waitUntil: 'load' });
    const b = await boxOf(page);
    expect(b.width * b.ratio, 'the picture is taller than the window').toBeGreaterThan(800);
    expect(Math.abs(b.height - b.width * b.ratio)).toBeLessThanOrEqual(1);
  });

  test('--moss-hero-max-height caps it and the crop keeps the foot', async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 800 });
    await page.goto(SKY_PAGE, { waitUntil: 'load' });
    await page.evaluate(() => document.documentElement.style.setProperty('--moss-hero-max-height', '500px'));
    const b = await boxOf(page);
    expect(Math.abs(b.height - 500)).toBeLessThanOrEqual(1);
    expect(b.position.split(' ')[1], 'the crop keeps the bottom').toBe('100%');
  });
});

// On a phone a kept panel fades out toward the picture's subject instead of
// ending in a seam. The words' own box stays solid (their contrast is measured
// there); the fade hangs below it, inside the room the picture keeps clear.
for (const scheme of SCHEMES) {
  test(`phone, ${scheme}: a kept panel fades into the picture, below the words`, async ({ browser }) => {
    const context = await browser.newContext({ colorScheme: scheme, viewport: { width: 390, height: 800 } });
    const page = await context.newPage();
    await page.goto(SKY_PAGE, { waitUntil: 'load' });
    await page.evaluate(() => scrollTo(0, 1e6));
    const m = await page.evaluate(() => {
      const hero = document.querySelector('footer .moss-hero-closing')!;
      const content = hero.querySelector('.moss-hero-content')!;
      const cs = getComputedStyle(content);
      const tail = getComputedStyle(content, '::after');
      const h = hero.getBoundingClientRect();
      const c = content.getBoundingClientRect();
      return {
        panel: cs.backgroundColor,
        ink: cs.color,
        tail: { content: tail.content, bg: tail.backgroundColor, height: parseFloat(tail.height), mask: tail.maskImage || tail.webkitMaskImage, top: tail.top },
        hero: { top: h.top, bottom: h.bottom, left: h.left, width: h.width },
        words: { bottom: c.bottom },
      };
    });
    expect(parseColor(m.panel).a, 'the panel is kept').toBeGreaterThan(0.5);
    const { overBlack, overWhite } = worstCaseContrast(m.panel, m.ink);
    expect(Math.min(overBlack, overWhite), `words ${m.ink} on ${m.panel}`).toBeGreaterThanOrEqual(4.5);
    expect(m.tail.content).not.toBe('none');
    expect(m.tail.bg, 'the tail is the panel faded').toBe(m.panel);
    expect(m.tail.mask).toContain('linear-gradient');
    expect(m.tail.height).toBeGreaterThan(24);
    expect(m.tail.height * 4, 'the fade takes at most a quarter of the room below the words').toBeLessThanOrEqual(m.hero.bottom - m.words.bottom);

    // No seam: the column of pixels crossing the end of the panel changes gently.
    const shot = await page.screenshot({ clip: { x: m.hero.left + m.hero.width * 0.2, y: m.words.bottom - 8, width: 1, height: m.tail.height + 16 } });
    const column = await page.evaluate(async (b64) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const canvas = document.createElement('canvas');
      canvas.width = img.width;
      canvas.height = img.height;
      const ctx = canvas.getContext('2d')!;
      ctx.drawImage(img, 0, 0);
      const px = ctx.getImageData(0, 0, img.width, img.height).data;
      return Array.from({ length: img.height }, (_, y) => 0.2126 * px[y * 4] + 0.7152 * px[y * 4 + 1] + 0.0722 * px[y * 4 + 2]);
    }, shot.toString('base64'));
    const steps = column.slice(1).map((v, i) => Math.abs(v - column[i]));
    expect(Math.max(...steps), `rows across the panel's end: ${column.map((v) => Math.round(v)).join(' ')}`).toBeLessThan(14);
    await context.close();
  });
}

// The words start at the text column, so the stretch of picture they cover
// depends on the page's content width: a dark patch over the right half of the
// sky keeps the panel on a default-width page (the words reach 65% across) and
// spares it on a `content_width: full` one (48%).
for (const [path, panel] of [['', true], ['full/', false]] as const) {
  test(`a dark patch over the right half of the sky ${panel ? 'keeps' : 'spares'} the panel on ${panel ? 'a default-width' : 'a full-width'} page at 1360px`, async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 800 });
    await page.goto(`${SKY_PAGE}${path}`, { waitUntil: 'load' });
    const bg = await page.evaluate(() => getComputedStyle(document.querySelector('footer .moss-hero-closing .moss-hero-content')!).backgroundColor);
    if (panel) expect(parseColor(bg).a).toBeGreaterThan(0.5);
    else expect(parseColor(bg).a).toBe(0);
  });
}
