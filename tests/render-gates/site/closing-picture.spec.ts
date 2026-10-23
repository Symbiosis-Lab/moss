// A footer.md that ends in a `:::hero` closes the page: the picture spans the
// viewport (whatever the body's own padding), nothing paints below it, and its
// words stay on it at every width, in both themes.
import { test, expect, Page } from '@playwright/test';
import { contrastRatio, luminanceOfCss, parseColor, relativeLuminance, worstCaseContrast } from '../../e2e/helpers/wcag-contrast';
import { CREDIT, GROUND, SKY, SKY_NIGHT, capAt } from '../../e2e/helpers/closing-picture-site';
import { gatePort } from '../../../playwright/gate-ports';
import { readFileSync } from 'node:fs';

// What the stylesheet's real boxes must sit inside: hero_ink's type model for these words.
const MODEL: Record<string, Record<'heading' | 'link', { left: number; right: number; top: number; bottom: number }>> = JSON.parse(
  readFileSync(new URL('./closing-picture-model.json', import.meta.url), 'utf8'),
);

const lum = ([r, g, b]: readonly number[]) => relativeLuminance(r, g, b);

/** A computed colour as 0-255 channels and alpha, whether the engine writes `rgb()` or `color(srgb …)`. */
const channels = (css: string) => {
  const nums = (css.match(/[\d.]+/g) ?? []).map(Number);
  const scale = css.startsWith('color(') ? 255 : 1;
  return { rgb: nums.slice(0, 3).map((n) => n * scale), a: nums[3] ?? 1 };
};
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
    const box = (el: Element | Range) => {
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
      // The glyph line boxes of the heading (a Range's client rects), not its element box, which is the whole measure.
      headingText: (() => {
        const range = document.createRange();
        range.selectNodeContents(content.querySelector('h2')!);
        const rects = [...range.getClientRects()];
        return {
          left: Math.min(...rects.map((r) => r.left)),
          right: Math.max(...rects.map((r) => r.right)),
          top: Math.min(...rects.map((r) => r.top)),
          bottom: Math.max(...rects.map((r) => r.bottom)),
        };
      })(),
      headingLeading: parseFloat(getComputedStyle(content.querySelector('h2')!).lineHeight) / parseFloat(getComputedStyle(content.querySelector('h2')!).fontSize),
      link: box(content.querySelector('a')!),
      linkStyle: (() => {
        const cs = getComputedStyle(content.querySelector('a')!);
        return { weight: cs.fontWeight, fill: cs.backgroundColor, border: cs.borderTopColor, ink: cs.color, text: getComputedStyle(document.body).color, page: getComputedStyle(document.body).backgroundColor };
      })(),
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
          // They start clamp(8px, 2vw, 24px) below the picture's top edge, and the display heading sets at 1.08.
          expect(m.heading.top - m.hero.top, 'the words start close to the top').toBeCloseTo(Math.min(24, Math.max(8, 0.02 * m.viewport)), 0);
          expect(m.headingLeading, 'the heading line height').toBeCloseTo(1.08, 2);

          // The call to action: weight 500, in the text colour with its border, on the page colour at 80%; it reads
          // at 4.5:1 over pure white and pure black whatever the picture behind it is.
          expect(m.linkStyle.weight).toBe('500');
          expect(m.linkStyle.ink, 'the text colour').toBe(m.linkStyle.text);
          expect(m.linkStyle.border, 'the border is the text colour').toBe(m.linkStyle.text);
          const fill = channels(m.linkStyle.fill);
          expect(fill.a, 'the link fill is 80% of the page colour').toBeCloseTo(0.8, 2);
          channels(m.linkStyle.page).rgb.forEach((c, i) => expect(fill.rgb[i]).toBeCloseTo(c, 0));
          const { overBlack, overWhite } = worstCaseContrast(`rgba(${fill.rgb.join(',')},${fill.a})`, m.linkStyle.ink);
          expect(Math.min(overBlack, overWhite), 'the link text on its fill').toBeGreaterThanOrEqual(4.5);

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

// 64rem is half-open: the narrow desktop's tighter padding round the words
// stops at 1023px, and a page exactly 1024px wide takes the wide one.
for (const [width, narrow] of [[1023, true], [1024, false]] as const) {
  test(`at ${width}px the closing picture's words take the ${narrow ? 'narrow desktop' : 'wide'} padding`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/', { waitUntil: 'load' });
    const pad = await page.evaluate(() =>
      getComputedStyle(document.querySelector('footer .moss-hero-closing')!).getPropertyValue('--moss-words-pad').trim(),
    );
    expect(pad === '5%', `--moss-words-pad is ${pad}`).toBe(!narrow);
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
// sky keeps the panel on a default-width page (the words reach 61% across at
// 2560px) and spares it on a `content_width: full` one (53%).
for (const [path, panel] of [['', true], ['full/', false]] as const) {
  test(`a dark patch over the right half of the sky ${panel ? 'keeps' : 'spares'} the panel on ${panel ? 'a default-width' : 'a full-width'} page at 1360px`, async ({ page }) => {
    await page.setViewportSize({ width: 1360, height: 800 });
    await page.goto(`${SKY_PAGE}${path}`, { waitUntil: 'load' });
    const bg = await page.evaluate(() => getComputedStyle(document.querySelector('footer .moss-hero-closing .moss-hero-content')!).backgroundColor);
    if (panel) expect(parseColor(bg).a).toBeGreaterThan(0.5);
    else expect(parseColor(bg).a).toBe(0);
  });
}

// `fade=top`: the picture's top fades into the page colour, and the credit stays
// at the foot. Contrast is read off the rendered pixels behind the words and the
// credit, with both hidden, so it checks what a reader sees.
const FADE = `http://localhost:${gatePort('closing-picture:fade')}/`;

const openFaded = async (page: Page, theme: string | null = null, url = FADE) => {
  await page.goto(url, { waitUntil: 'load' });
  if (theme) await page.evaluate((t) => document.documentElement.setAttribute('data-theme', t), theme);
  await page.evaluate(() => scrollTo(0, 1e6));
  await page.waitForFunction(() =>
    [...document.querySelectorAll('footer .moss-hero-closing img')]
      .filter((i) => getComputedStyle(i).display !== 'none')
      .every((i) => (i as HTMLImageElement).complete && (i as HTMLImageElement).naturalWidth > 0),
  );
};

/** The theme's page text colour and page colour, as the engine computes them now. */
const themeInks = (page: Page) =>
  page.evaluate(() => {
    const read = (token: string) => {
      const probe = document.createElement('i');
      probe.style.color = `var(${token})`;
      document.body.append(probe);
      const colour = getComputedStyle(probe).color;
      probe.remove();
      return colour;
    };
    return { text: read('--moss-color-text'), page: read('--moss-color-bg') };
  });

const fadedBoxes = (page: Page) =>
  page.evaluate(() => {
    const box = (el: Element) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
    };
    const hero = document.querySelector('footer .moss-hero-closing')!;
    const content = hero.querySelector('.moss-hero-content')!;
    const credit = hero.querySelector('.moss-hero-credit')!;
    const img = [...hero.querySelectorAll('img')].find((i) => getComputedStyle(i).display !== 'none')!;
    const style = getComputedStyle(img);
    return {
      hero: box(hero),
      content: box(content),
      heading: box(content.querySelector('h2')!),
      // The credit's glyphs: where its text ends and how many lines it sets.
      creditText: (() => {
        const range = document.createRange();
        range.selectNodeContents(credit);
        const rects = [...range.getClientRects()];
        return { left: Math.min(...rects.map((r) => r.left)), right: Math.max(...rects.map((r) => r.right)), lines: new Set(rects.map((r) => Math.round(r.top))).size };
      })(),
      // The glyph line boxes of the heading (a Range's client rects), not its element box, which is the whole measure.
      headingText: (() => {
        const range = document.createRange();
        range.selectNodeContents(content.querySelector('h2')!);
        const rects = [...range.getClientRects()];
        return {
          left: Math.min(...rects.map((r) => r.left)),
          right: Math.max(...rects.map((r) => r.right)),
          top: Math.min(...rects.map((r) => r.top)),
          bottom: Math.max(...rects.map((r) => r.bottom)),
        };
      })(),
      link: box(content.querySelector('a')!),
      credit: box(credit),
      colophon: box(document.querySelector('footer .moss-colophon')!),
      viewport: document.documentElement.clientWidth,
      docBottom: document.documentElement.scrollHeight - scrollY,
      mask: style.maskImage && style.maskImage !== 'none' ? style.maskImage : style.webkitMaskImage,
      words: { ink: getComputedStyle(content).color, panel: getComputedStyle(content).backgroundColor },
      creditStyle: { ink: getComputedStyle(credit).color, panel: getComputedStyle(credit).backgroundColor, size: parseFloat(getComputedStyle(credit).fontSize), pad: parseFloat(getComputedStyle(credit).paddingInlineStart) },
      page: getComputedStyle(document.body).backgroundColor,
      shown: img.getAttribute('data-hero-variant'),
    };
  });

/** The pixels in `box` (viewport px), with the words and credit hidden. */
const backdrop = async (page: Page, box: { left: number; right: number; top: number; bottom: number }) => {
  await page.addStyleTag({ content: 'footer .moss-hero-closing :is(.moss-hero-content, .moss-hero-credit) { visibility: hidden !important; }' });
  const shot = await page.screenshot({ clip: { x: box.left, y: box.top, width: box.right - box.left, height: box.bottom - box.top } });
  return page.evaluate(async (b64) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const canvas = document.createElement('canvas');
    [canvas.width, canvas.height] = [img.width, img.height];
    const ctx = canvas.getContext('2d')!;
    ctx.drawImage(img, 0, 0);
    const px = ctx.getImageData(0, 0, img.width, img.height).data;
    const out: number[][] = [];
    for (let i = 0; i < px.length; i += 4) out.push([px[i], px[i + 1], px[i + 2]]);
    return out;
  }, shot.toString('base64'));
};

const worstContrast = (ink: string, pixels: number[][]) => pixels.reduce((worst, p) => Math.min(worst, contrastWith(ink, p)), Infinity);

const THEMES = [
  { name: 'light', system: 'light', toggle: null, dark: false },
  { name: 'dark by system setting', system: 'dark', toggle: null, dark: true },
  { name: 'dark by toggle', system: 'light', toggle: 'dark', dark: true },
] as const;

for (const theme of THEMES) {
  for (const width of [2560, 1360, 768, 390]) {
    test(`faded closing picture, ${theme.name}, ${width}px: the top fades into the page, the words and credit read on it`, async ({ browser }) => {
      const context = await browser.newContext({ colorScheme: theme.system, viewport: { width, height: width > 2000 ? 1300 : 800 } });
      const page = await context.newPage();
      await openFaded(page, theme.toggle);
      const m = await fadedBoxes(page);
      expect(m.shown, 'the theme picks the picture').toBe(theme.dark ? 'dark' : 'light');
      expect(m.mask, 'the picture is masked').toContain('linear-gradient');

      // The glyphs sit inside the rows and columns the ink measurement reads (hero_ink's type model).
      const rel = (b: { left: number; right: number; top: number; bottom: number }) => ({ left: b.left - m.hero.left, right: b.right - m.hero.left, top: b.top - m.hero.top, bottom: b.bottom - m.hero.top });
      for (const [part, real] of [['heading', rel(m.headingText)], ['link', rel(m.link)]] as const) {
        const want = MODEL[String(width)][part];
        const where = `${part} at ${width}px: real ${JSON.stringify(real)}, modelled ${JSON.stringify(want)}`;
        // A line's client rect is its font's content area, which at the heading's 1.08 leading runs several px above its line box, by an amount that depends on the face's metrics and so cannot be modelled.
        expect(real.top, where).toBeGreaterThanOrEqual(want.top - 10);
        expect(real.bottom, where).toBeLessThanOrEqual(want.bottom + 2);
        expect(real.left, where).toBeGreaterThanOrEqual(want.left - 2);
        expect(real.right, where).toBeLessThanOrEqual(want.right + 2);
      }

      // Nothing follows: the picture is the page's last box and the colophon sits on its foot.
      expect(m.hero.left).toBeCloseTo(0, 0);
      expect(m.hero.right).toBeCloseTo(m.viewport, 0);
      expect(m.hero.bottom).toBeCloseTo(m.docBottom, 0);
      expect(m.colophon.bottom).toBeLessThanOrEqual(m.hero.bottom + 1);
      expect(m.colophon.top).toBeGreaterThanOrEqual(m.credit.bottom - 1);

      // The credit stays at the foot, on the inline end, last before the colophon.
      expect(m.credit.top, 'below the words').toBeGreaterThan(m.content.bottom);
      expect(Math.abs(m.credit.bottom - m.colophon.top), 'on the colophon row').toBeLessThanOrEqual(2);
      // Its text ends where the page's text column ends (the words start at the other end of it), on one line where it fits.
      const column = MODEL[String(width)].heading.left;
      expect(Math.abs(m.creditText.right - (m.hero.right - column)), 'right-aligned to the end of the column').toBeLessThanOrEqual(1);
      if (width >= 1360) expect(m.creditText.lines, 'on one line').toBe(1);
      expect(m.creditStyle.size).toBeLessThan(16);

      // In ink, no panel, and AA against every pixel behind them.
      expect(parseColor(m.words.panel).a, 'no panel behind the words').toBe(0);
      expect(parseColor(m.creditStyle.panel).a, 'no panel behind the credit').toBe(0);
      const behindWords = await backdrop(page, m.heading);
      expect(worstContrast(m.words.ink, behindWords), `words ${m.words.ink}`).toBeGreaterThanOrEqual(4.5);
      const behindCredit = await backdrop(page, m.credit);
      expect(worstContrast(m.creditStyle.ink, behindCredit), `credit ${m.creditStyle.ink}`).toBeGreaterThanOrEqual(4.5);

      // The top row of the picture is the page colour; halfway down it is the picture.
      const top = await backdrop(page, { left: m.hero.left + 2, right: m.hero.right - 2, top: m.hero.top, bottom: m.hero.top + 1 });
      const pageLum = luminanceOfCss(m.page);
      expect(top.reduce((most, p) => Math.max(most, Math.abs(lum(p) - pageLum)), 0), 'the picture starts in the page colour').toBeLessThan(0.02);
      await context.close();
    });
  }
}

test('a faded closing picture in a right-to-left page: the credit sits at the foot, at the inline end', async ({ page }) => {
  await page.setViewportSize({ width: 1360, height: 800 });
  await openFaded(page);
  await page.evaluate(() => { document.documentElement.dir = 'rtl'; });
  const m = await fadedBoxes(page);
  expect(Math.abs(m.creditText.left - (m.hero.left + MODEL['1360'].heading.left)), 'at the end of the column, which is the inline end').toBeLessThanOrEqual(1);
  expect(m.credit.top, 'below the words').toBeGreaterThan(m.content.bottom);
  expect(Math.abs(m.credit.bottom - m.colophon.top), 'on the colophon row').toBeLessThanOrEqual(2);
});

test('the credit is in the UI face, and --moss-hero-credit-font gives it another', async ({ page }) => {
  await page.setViewportSize({ width: 1360, height: 800 });
  await openFaded(page);
  const family = () => page.evaluate(() => [getComputedStyle(document.querySelector('footer .moss-hero-credit')!).fontFamily, getComputedStyle(document.body).fontFamily]);
  const [credit, body] = await family();
  expect(credit, 'the UI face by default').toBe(body);
  await page.evaluate(() => document.documentElement.style.setProperty('--moss-hero-credit-font', 'monospace'));
  expect((await family())[0]).toBe('monospace');
});

test('a faded closing picture is still capped by --moss-hero-max-height, and the crop keeps the foot', async ({ page }) => {
  await page.setViewportSize({ width: 1360, height: 800 });
  await openFaded(page);
  await page.evaluate(() => document.documentElement.style.setProperty('--moss-hero-max-height', '400px'));
  const b = await boxOf(page);
  expect(Math.abs(b.height - 400)).toBeLessThanOrEqual(1);
  expect(b.position.split(' ')[1]).toBe('100%');
});

test('without fade=top nothing fades and the credit stays at the foot', async ({ page }) => {
  await page.setViewportSize({ width: 1360, height: 800 });
  await page.goto('/', { waitUntil: 'load' });
  const m = await page.evaluate(() => {
    const hero = document.querySelector('footer .moss-hero-closing')!;
    const img = [...hero.querySelectorAll('img')].find((i) => getComputedStyle(i).display !== 'none')!;
    const s = getComputedStyle(img);
    return { fade: hero.hasAttribute('data-fade'), mask: s.maskImage || s.webkitMaskImage };
  });
  expect(m).toEqual({ fade: false, mask: 'none' });
});

// Busy under the words and along the foot, with the fade: the words keep their
// panel (on a phone ending in its fade), and the credit stays at the foot on a
// small panel of its own.
const BUSY_FADE = `http://localhost:${gatePort('closing-picture:busy-fade')}/`;
for (const scheme of SCHEMES) {
  for (const width of [390, 900]) {
    test(`busy faded picture, ${scheme}, ${width}px: the words keep their panel, the credit its own at the foot`, async ({ browser }) => {
      const context = await browser.newContext({ colorScheme: scheme, viewport: { width, height: 800 } });
      const page = await context.newPage();
      await page.goto(BUSY_FADE, { waitUntil: 'load' });
      const m = await page.evaluate(() => {
        const hero = document.querySelector('footer .moss-hero-closing')!;
        const content = hero.querySelector('.moss-hero-content')!;
        const credit = hero.querySelector('.moss-hero-credit')!;
        const [h, c, k] = [hero.getBoundingClientRect(), content.getBoundingClientRect(), credit.getBoundingClientRect()];
        return {
          hero: { bottom: h.bottom },
          content: { bottom: c.bottom },
          credit: { top: k.top, bottom: k.bottom },
          panels: [getComputedStyle(content).backgroundColor, getComputedStyle(credit).backgroundColor],
          ink: getComputedStyle(credit).color,
          tail: getComputedStyle(content, '::after').content,
        };
      });
      expect(parseColor(m.panels[0]).a, 'the words keep their panel').toBeGreaterThan(0.5);
      expect(parseColor(m.panels[1]).a, 'the credit keeps a panel').toBeGreaterThan(0.5);
      expect(m.credit.top, 'the credit is at the foot, below the words').toBeGreaterThan(m.content.bottom);
      expect(m.credit.bottom).toBeLessThanOrEqual(m.hero.bottom + 1);
      const { overBlack, overWhite } = worstCaseContrast(m.panels[1], m.ink);
      expect(Math.min(overBlack, overWhite), `credit ${m.ink} on ${m.panels[1]}`).toBeGreaterThanOrEqual(4.5);
      if (width < 768) expect(m.tail, "the words' panel ends in its fade").toBe('""');
      await context.close();
    });
  }
}

// A faded top shows the page, so the words in ink take the theme's own colours,
// not moss's fixed near-black and white.
for (const theme of THEMES) {
  test(`a faded closing picture's words are the page's text colour, ${theme.name}`, async ({ browser }) => {
    const context = await browser.newContext({ colorScheme: theme.system, viewport: { width: 1360, height: 800 } });
    const page = await context.newPage();
    await openFaded(page, theme.toggle);
    const m = await fadedBoxes(page);
    expect(parseColor(m.words.panel).a, 'in ink, no panel').toBe(0);
    expect(m.words.ink).toBe((await themeInks(page)).text);
    await context.close();
  });
}

// `panel=none` is the author's call: plain ink over the same hatching, with nothing
// behind the words or the credit, the better ink of the two in each theme.
const PLAIN = `http://localhost:${gatePort('closing-picture:plain')}/`;
for (const scheme of SCHEMES) {
  for (const width of [390, 1360]) {
    test(`panel=none over a busy picture, ${scheme}, ${width}px: plain ink, no panel behind the words or the credit`, async ({ browser }) => {
      const context = await browser.newContext({ colorScheme: scheme, viewport: { width, height: 800 } });
      const page = await context.newPage();
      await page.goto(PLAIN, { waitUntil: 'load' });
      const m = await page.evaluate(() => {
        const hero = document.querySelector('footer .moss-hero-closing')!;
        const content = hero.querySelector('.moss-hero-content')!;
        const credit = hero.querySelector('.moss-hero-credit')!;
        return {
          panels: [getComputedStyle(content).backgroundColor, getComputedStyle(credit).backgroundColor],
          ink: [getComputedStyle(content.querySelector('h2')!).color, getComputedStyle(credit).color],
          tail: getComputedStyle(content, '::after').content,
          shadows: [content, content.querySelector('h2')!, content.querySelector('a')!].map((e) => getComputedStyle(e).textShadow),
        };
      });
      expect(m.shadows, 'panel=none draws no shadow').toEqual(['none', 'none', 'none']);
      expect(parseColor(m.panels[0]).a, 'no panel behind the words').toBe(0);
      expect(parseColor(m.panels[1]).a, 'no panel behind the credit').toBe(0);
      // One of the theme's two inks, its text or its page colour, whichever reads better over the hatching at this layout.
      const inks = await themeInks(page);
      expect([inks.text, inks.page], `words ${m.ink[0]}`).toContain(m.ink[0]);
      await context.close();
    });
  }
}

// `max-height=` and a position on the picture are build data: the build measures
// the part of the picture the capped box shows, and a phone gives the faded
// picture a floor so its words sit on the faded top. The credit stays at the
// foot. Contrast is read off the rendered pixels.
const CAPPED = `http://localhost:${gatePort('closing-picture:capped')}/`;
for (const theme of THEMES) {
  for (const width of [1360, 1100, 900, 768, 390]) {
    test(`capped faded closing picture, ${theme.name}, ${width}px: measured as shown, words and credit in ink on the picture`, async ({ browser }) => {
      const context = await browser.newContext({ colorScheme: theme.system, viewport: { width, height: 800 } });
      const page = await context.newPage();
      await openFaded(page, theme.toggle, CAPPED);
      const m = await fadedBoxes(page);
      const extra = await page.evaluate(() => {
        const hero = document.querySelector('footer .moss-hero-closing')!;
        const img = [...hero.querySelectorAll('img')].find((i) => getComputedStyle(i).display !== 'none')!;
        const after = [...document.querySelectorAll('footer ~ *, footer .moss-hero-closing ~ :not(.moss-colophon)')].filter((e) => {
          const s = getComputedStyle(e);
          return s.display !== 'none' && s.position !== 'fixed' && e.getBoundingClientRect().height > 0;
        });
        return { position: getComputedStyle(img).objectPosition, cap: getComputedStyle(hero).getPropertyValue('--moss-hero-max-height').trim(), after: after.length };
      });
      expect(m.shown).toBe(theme.dark ? 'dark' : 'light');
      expect(extra.position, 'the position the image names').toBe('50% 72%');
      expect(extra.cap).toBe('clamp(440px, 50vw, 680px)');

      // The box: the cap from 48rem; on a phone the faded floor, which is taller.
      const height = m.hero.bottom - m.hero.top;
      if (width >= 768) expect(Math.abs(height - capAt(width)), `box ${height}`).toBeLessThanOrEqual(1);
      else expect(height).toBeGreaterThanOrEqual(608 - 1);

      // Nothing follows the picture, and nothing of it is outside it.
      expect(m.hero.left).toBeCloseTo(0, 0);
      expect(m.hero.right).toBeCloseTo(m.viewport, 0);
      expect(m.hero.bottom).toBeCloseTo(m.docBottom, 0);
      expect(extra.after, 'no box after the picture').toBe(0);
      for (const [part, r] of [['words', m.content], ['credit', m.credit], ['colophon', m.colophon]] as const) {
        expect(r.top, `${part} top`).toBeGreaterThanOrEqual(m.hero.top - 1);
        expect(r.bottom, `${part} bottom`).toBeLessThanOrEqual(m.hero.bottom + 1);
        expect(r.left, `${part} left`).toBeGreaterThanOrEqual(m.hero.left - 1);
        expect(r.right, `${part} right`).toBeLessThanOrEqual(m.hero.right + 1);
      }
      // The words sit inside the rows the build measured (on a phone the top
      // 60% of the box); the credit is at the foot, last before the colophon.
      if (width < 768) expect(m.content.bottom).toBeLessThanOrEqual(m.hero.top + height * 0.6);
      expect(m.credit.top, 'the credit below the words').toBeGreaterThan(m.content.bottom);
      expect(Math.abs(m.credit.bottom - m.colophon.top), 'on the colophon row').toBeLessThanOrEqual(2);

      expect(parseColor(m.words.panel).a, 'no panel behind the words').toBe(0);
      expect(parseColor(m.creditStyle.panel).a, 'no panel behind the credit').toBe(0);
      const behindWords = await backdrop(page, m.heading);
      expect(worstContrast(m.words.ink, behindWords), `words ${m.words.ink}`).toBeGreaterThanOrEqual(4.5);
      const behindCredit = await backdrop(page, m.credit);
      expect(worstContrast(m.creditStyle.ink, behindCredit), `credit ${m.creditStyle.ink}`).toBeGreaterThanOrEqual(4.5);
      await context.close();
    });
  }
}
// A picture that is pale where the words sit and dark below them: the build
// measures the glyphs' own rows and the credit's own box, so both are in ink
// with no panel, at a phone's width and a wide one, in each theme.
const LANDSCAPE = `http://localhost:${gatePort('closing-picture:landscape')}/`;
for (const theme of THEMES) {
  for (const width of [1360, 768, 390]) {
    test(`pale-sky closing picture, ${theme.name}, ${width}px: words and credit in ink, no panel`, async ({ browser }) => {
      const context = await browser.newContext({ colorScheme: theme.system, viewport: { width, height: 800 } });
      const page = await context.newPage();
      await openFaded(page, theme.toggle, LANDSCAPE);
      const m = await fadedBoxes(page);
      expect(parseColor(m.words.panel).a, 'no panel behind the words').toBe(0);
      expect(parseColor(m.creditStyle.panel).a, 'no panel behind the credit').toBe(0);
      // Dark ink on the pale sky in light, light ink on the dark sky in dark; the
      // credit is the page colour over the dark foot in light and the pale foot in dark.
      // The words take the theme's page text colour (a faded top shows the page), whichever theme.
      expect(m.words.ink, 'words in the theme\'s text colour').toBe((await themeInks(page)).text);
      const creditLum = luminanceOfCss(m.creditStyle.ink);
      if (theme.dark) expect(creditLum, `credit ${m.creditStyle.ink}`).toBeLessThan(0.05);
      else expect(creditLum, `credit ${m.creditStyle.ink}`).toBeGreaterThan(0.8);
      expect(m.creditStyle.size).toBeLessThan(16);
      expect(worstContrast(m.words.ink, await backdrop(page, m.heading)), 'heading').toBeGreaterThanOrEqual(4.5);
      expect(worstContrast(m.words.ink, await backdrop(page, m.link)), 'link').toBeGreaterThanOrEqual(4.5);
      expect(worstContrast(m.creditStyle.ink, await backdrop(page, m.credit)), 'credit').toBeGreaterThanOrEqual(4.5);
      await context.close();
    });
  }
}

// The archival edition on a closing picture: the words, the credit (when the
// picture has one), the edition and the colophon row stack at the foot in that
// order, never overlapping one another, inside the picture, with nothing below it.
const ARCHIVAL_SITES = [
  { name: 'no credit', port: 'closing-picture:archival', credit: false },
  { name: 'with a credit', port: 'closing-picture:archival-caption', credit: true },
];
const ARCHIVAL_WIDTHS = [
  { name: 'desktop', width: 1360, height: 800 },
  { name: 'narrow desktop', width: 900, height: 800 },
  { name: 'phone 390', width: 390, height: 800 },
];
for (const site of ARCHIVAL_SITES) {
  for (const scheme of SCHEMES) {
    for (const { name: wname, width, height } of ARCHIVAL_WIDTHS) {
      test.describe(`archival closing picture, ${site.name}, ${scheme}, ${wname}`, () => {
        test.use({ viewport: { width, height }, colorScheme: scheme });

        test('the foot stacks in reading order, nothing overlaps, nothing below the picture', async ({ page }) => {
          await page.goto(`http://localhost:${gatePort(site.port)}/`, { waitUntil: 'load' });
          const m = await page.evaluate(() => {
            const box = (el: Element | null) => {
              if (!el) return null;
              const r = el.getBoundingClientRect();
              return { left: r.left, right: r.right, top: r.top + scrollY, bottom: r.bottom + scrollY };
            };
            const hero = document.querySelector('footer .moss-hero-closing')!;
            const edition = hero.querySelector('.moss-edition')!;
            return {
              docHeight: document.documentElement.scrollHeight,
              hero: box(hero),
              words: box(hero.querySelector('.moss-hero-content')),
              credit: box(hero.querySelector('.moss-hero-credit')),
              edition: box(edition),
              colophon: box(document.querySelector('footer .moss-colophon')),
              ink: getComputedStyle(edition).color,
              panel: getComputedStyle(edition).backgroundColor,
              afterFooter: [...document.querySelectorAll('footer ~ *')].filter((e) => {
                const s = getComputedStyle(e);
                return s.display !== 'none' && s.position !== 'fixed' && e.getBoundingClientRect().height > 0;
              }).length,
            };
          });
          type B = { left: number; right: number; top: number; bottom: number };
          const parts: [string, B | null][] = [
            ['words', m.words],
            ['credit', m.credit],
            ['edition', m.edition],
            ['colophon', m.colophon],
          ];
          const present = parts.filter((p): p is [string, B] => p[1] !== null);
          expect(present.map((p) => p[0]), 'the parts present').toEqual(
            site.credit ? ['words', 'credit', 'edition', 'colophon'] : ['words', 'edition', 'colophon'],
          );
          for (const [name, r] of present) {
            expect(r.top, `${name} inside the picture (top)`).toBeGreaterThanOrEqual(m.hero!.top - 1);
            expect(r.bottom, `${name} inside the picture (bottom)`).toBeLessThanOrEqual(m.hero!.bottom + 1);
            expect(r.left, `${name} inside the picture (left)`).toBeGreaterThanOrEqual(m.hero!.left - 1);
            expect(r.right, `${name} inside the picture (right)`).toBeLessThanOrEqual(m.hero!.right + 1);
          }
          for (let i = 0; i < present.length; i++) {
            for (let j = i + 1; j < present.length; j++) {
              const [an, a] = present[i];
              const [bn, b] = present[j];
              const overlap = a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
              expect(overlap, `${an} and ${bn} do not intersect`).toBe(false);
            }
          }
          // Reading order: each part starts at or below the one before it.
          for (let i = 1; i < present.length; i++) {
            expect(present[i][1].top, `${present[i][0]} follows ${present[i - 1][0]}`).toBeGreaterThanOrEqual(present[i - 1][1].top - 1);
          }
          expect(m.hero!.bottom, 'nothing is below the picture').toBeCloseTo(m.docHeight, 0);
          expect(m.afterFooter, 'no box follows the footer').toBe(0);

          if (parseColor(m.panel).a > 0) {
            // No ink reads over the whole band the block can sit in, so it keeps its panel: the panel must carry the ink.
            const { overBlack, overWhite } = worstCaseContrast(m.panel, m.ink);
            expect(Math.min(overBlack, overWhite), `edition ${m.ink} on its panel ${m.panel}`).toBeGreaterThanOrEqual(4.5);
          }
          // The picture the box sits on, measured with the edition hidden: the
          // worst pixel under the box must clear AA in the ink the page chose.
          const area = await page.evaluate(() => {
            const edition = document.querySelector<HTMLElement>('footer .moss-hero-closing .moss-edition')!;
            const r = edition.getBoundingClientRect();
            edition.style.visibility = 'hidden';
            return { x: r.left + scrollX, y: r.top + scrollY, width: r.width, height: r.height };
          });
          const shot = await page.screenshot({ fullPage: true, clip: area });
          const rgb = await page.evaluate(async (b64) => {
            const img = new Image();
            img.src = `data:image/png;base64,${b64}`;
            await img.decode();
            const canvas = document.createElement('canvas');
            canvas.width = img.width;
            canvas.height = img.height;
            const ctx = canvas.getContext('2d')!;
            ctx.drawImage(img, 0, 0);
            const px = ctx.getImageData(0, 0, img.width, img.height).data;
            const out: number[] = [];
            for (let i = 0; i < px.length; i += 4) out.push(px[i], px[i + 1], px[i + 2]);
            return out;
          }, shot.toString('base64'));
          const ink = luminanceOfCss(m.ink);
          let worst = Infinity;
          let worstAt = '';
          for (let i = 0; i < rgb.length; i += 3) {
            const ratio = contrastRatio(ink, relativeLuminance(rgb[i], rgb[i + 1], rgb[i + 2]));
            if (ratio < worst) {
              worst = ratio;
              worstAt = `${rgb[i]},${rgb[i + 1]},${rgb[i + 2]}`;
            }
          }
          if (parseColor(m.panel).a === 0) {
            expect(worst, `edition ${m.ink} on the picture under it (worst pixel ${worstAt}, box ${JSON.stringify(area)})`).toBeGreaterThanOrEqual(4.5);
          }
        });
      });
    }
  }
}
