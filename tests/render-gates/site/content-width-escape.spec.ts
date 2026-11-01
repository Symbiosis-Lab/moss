/**
 * Where a `data-width` band lands, and whether the text after it clears it.
 *
 * site.css's content-width escape (`article.container > [data-width]`) widens
 * a block past the reading measure and centres it on the column. It used to
 * centre with `position: relative; left: 50%` plus `translateX(-50%)`: both
 * move only the paint, and only along x. Under vertical-rl x is the block
 * axis, so a figure was painted over the paragraphs that follow it while its
 * slot in the flow stayed put; under `dir="rtl"` the band sat hundreds of
 * pixels off-centre. Nothing in the emitted markup shows either — only a
 * layout does.
 *
 * Site: tests/e2e/helpers/gate-sites.ts → CONTENT_WIDTH_ESCAPE_GATE.
 */
import { test, expect, type Page } from '@playwright/test';

type Box = { left: number; right: number; top: number; bottom: number; width: number; height: number };

const TOKENS = ['body', 'wide', 'page', 'screen'] as const;

const box = (page: Page, sel: string): Promise<Box> =>
  page.locator(sel).first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
  });

/** The article's content box: where the text column actually runs. */
const column = (page: Page): Promise<Box> =>
  page.locator('article.container').evaluate((el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    const left = r.left + parseFloat(s.paddingLeft) + parseFloat(s.borderLeftWidth);
    const right = r.right - parseFloat(s.paddingRight) - parseFloat(s.borderRightWidth);
    const top = r.top + parseFloat(s.paddingTop) + parseFloat(s.borderTopWidth);
    const bottom = r.bottom - parseFloat(s.paddingBottom) - parseFloat(s.borderBottomWidth);
    return { left, right, top, bottom, width: right - left, height: bottom - top };
  });

/** A length resolved inside `main`, where the escape resolves its own. */
const lengthInMain = (page: Page, value: string): Promise<number> =>
  page.evaluate((value) => {
    const probe = document.createElement('div');
    probe.style.inlineSize = value;
    document.querySelector('main')!.appendChild(probe);
    const w = probe.getBoundingClientRect().width;
    probe.remove();
    return w;
  }, value);

const paragraphs = (page: Page): Promise<Box[]> =>
  page.locator('article.container > p').evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    }),
  );

test.describe('vertical-rl', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
  });

  for (const token of TOKENS) {
    test(`|${token}: the text after the figure is laid out past it, not under it`, async ({ page }) => {
      await page.goto(`/v-${token}/`);
      const fig = await box(page, 'article.container > figure');
      const sides = await page.locator('article.container > figure').evaluate((f) => {
        const edge = (el: Element) => el.getBoundingClientRect();
        const before: number[] = [];
        const after: number[] = [];
        for (let n = f.previousElementSibling; n; n = n.previousElementSibling) before.push(edge(n).left);
        for (let n = f.nextElementSibling; n; n = n.nextElementSibling) after.push(edge(n).right);
        return { before, after };
      });
      // Block progression is right to left: everything before the figure (the
      // title) lies wholly to its right, everything after it wholly to its
      // left. A paint-only offset breaks one side or the other.
      expect(sides.before.length).toBeGreaterThan(0);
      expect(sides.after.length).toBe(2);
      for (const left of sides.before) expect(left).toBeGreaterThanOrEqual(fig.right - 1);
      for (const right of sides.after) expect(right).toBeLessThanOrEqual(fig.left + 1);
    });
  }

  for (const token of ['wide', 'page', 'screen'] as const) {
    test(`|${token} is inert: the figure stands in the text band, head-aligned`, async ({ page }) => {
      await page.goto(`/v-${token}/`);
      const fig = await box(page, 'article.container > figure');
      const [p] = await paragraphs(page);
      // The measure is already the whole column; a token must not stretch the
      // plate into the container's inset above and below the text.
      expect(Math.abs(fig.top - p.top)).toBeLessThanOrEqual(1);
      expect(Math.abs(fig.bottom - p.bottom)).toBeLessThanOrEqual(1);
    });
  }
});

test.describe('horizontal', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
  });

  const expectedWidth: Record<(typeof TOKENS)[number], string> = {
    body: '100%',
    wide: 'var(--moss-width-wide)',
    page: 'var(--moss-site-max-width)',
    screen: '100%',
  };

  for (const token of TOKENS) {
    test(`|${token}: the band has its width, is centred on the column, and the text follows it`, async ({ page }) => {
      await page.goto(`/h-${token}/`);
      const fig = await box(page, 'article.container > figure');
      const col = await column(page);
      const want = token === 'body' ? col.width : await lengthInMain(page, expectedWidth[token]);
      expect(Math.abs(fig.width - want)).toBeLessThanOrEqual(1);
      expect(Math.abs((fig.left + fig.right) / 2 - (col.left + col.right) / 2)).toBeLessThanOrEqual(0.5);
      for (const p of await paragraphs(page)) expect(p.top).toBeGreaterThanOrEqual(fig.bottom);
    });
  }

  test('a fenced-div band spans the viewport and keeps its text at the reading width, centred', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/band/');
    const band = await box(page, 'article.container > div.band');
    const p = await box(page, 'article.container > div.band > p');
    const readingWidth = await lengthInMain(page, 'var(--moss-content-width)');
    expect(Math.abs(band.left)).toBeLessThanOrEqual(1);
    expect(Math.abs(band.right - 1280)).toBeLessThanOrEqual(1);
    expect(p.width).toBeLessThanOrEqual(readingWidth + 1);
    expect(p.width).toBeGreaterThan(readingWidth / 2);
    expect(Math.abs((p.left + p.right) / 2 - (band.left + band.right) / 2)).toBeLessThanOrEqual(1);
  });

  for (const width of [390, 1024]) {
    test(`a screen grid and a screen fenced div both span the ${width}px viewport; cell text keeps the page inset (phone) / fills the band (desktop)`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/bands/');
      await page.addStyleTag({ content: 'article.container > [data-width="screen"] { background: #cde; }' });
      const fenced = await box(page, 'article.container > div[data-width="screen"]:not(.moss-grid)');
      const grid = await box(page, 'article.container > .moss-grid[data-width="screen"]');
      for (const b of [fenced, grid]) {
        expect(Math.abs(b.left)).toBeLessThanOrEqual(1);
        expect(Math.abs(b.width - width)).toBeLessThanOrEqual(1);
      }
      const inset = await lengthInMain(page, 'var(--moss-container-padding)');
      const cell = await page.locator('.moss-grid[data-width="screen"] > *').first().evaluate((el) => {
        const range = document.createRange();
        range.selectNodeContents(el);
        return range.getBoundingClientRect().left;
      });
      if (width < 768) expect(Math.abs(cell - inset)).toBeLessThanOrEqual(1);
      else expect(Math.abs(cell - grid.left)).toBeLessThanOrEqual(1); // unchanged above 48rem: cells fill the band
    });
  }

  test('|wide under dir="rtl" is centred on the column too, with no sideways scroll', async ({ page }) => {
    await page.goto('/h-wide/');
    await page.evaluate(() => { document.documentElement.dir = 'rtl'; });
    const fig = await box(page, 'article.container > figure');
    const col = await column(page);
    expect(Math.abs((fig.left + fig.right) / 2 - (col.left + col.right) / 2)).toBeLessThanOrEqual(0.5);
    const overflow = await page.evaluate(() => document.scrollingElement!.scrollWidth - window.innerWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  for (const side of ['left', 'right'] as const) {
    test(`a |wide|align-${side} figure keeps the band width and is centred`, async ({ page }) => {
      // A block reaches the band from its negative margins alone; a float
      // shrinks to fit, so only the band's own inline size keeps it wide.
      // The old paint offset pushed an align-right band off the left edge
      // of the page.
      await page.goto(`/float-${side}/`);
      const fig = await box(page, 'article.container > figure');
      const col = await column(page);
      expect(Math.abs(fig.width - (await lengthInMain(page, 'var(--moss-width-wide)')))).toBeLessThanOrEqual(1);
      expect(Math.abs((fig.left + fig.right) / 2 - (col.left + col.right) / 2)).toBeLessThanOrEqual(0.5);
    });
  }

  test('a percent wins over a width token: `|wide|40%` stays inside the column', async ({ page }) => {
    await page.goto('/sized/');
    const fig = await box(page, 'article.container > figure');
    const col = await column(page);
    expect(Math.abs(fig.width - 0.4 * col.width)).toBeLessThanOrEqual(1);
    expect(fig.left).toBeGreaterThanOrEqual(col.left - 1);
    expect(fig.right).toBeLessThanOrEqual(col.right + 1);
  });
});

test.describe('horizontal, phone width', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
  });

  for (const token of TOKENS) {
    test(`|${token} collapses to the column`, async ({ page }) => {
      await page.goto(`/h-${token}/`);
      const fig = await box(page, 'article.container > figure');
      const col = await column(page);
      expect(Math.abs(fig.left - col.left)).toBeLessThanOrEqual(1);
      expect(Math.abs(fig.right - col.right)).toBeLessThanOrEqual(1);
    });
  }

  test('a fenced-div band causes no sideways scroll', async ({ page }) => {
    await page.goto('/band/');
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  test('a fenced-div screen band spans the viewport and its text keeps the page gutter', async ({ page }) => {
    await page.goto('/band/');
    const band = await box(page, 'article.container > div.band');
    const p = await box(page, 'article.container > div.band > p');
    const col = await column(page);
    expect(Math.abs(band.left)).toBeLessThanOrEqual(1);
    expect(Math.abs(band.right - 390)).toBeLessThanOrEqual(1);
    expect(Math.abs(p.left - col.left)).toBeLessThanOrEqual(1);
    expect(Math.abs(p.right - col.right)).toBeLessThanOrEqual(1);
  });

  test('a hand-written band narrowed by an inline width stays centred', async ({ page }) => {
    await page.goto('/raw/');
    const fig = await box(page, 'article.container > figure');
    const col = await column(page);
    expect(Math.abs(fig.width - 0.4 * col.width)).toBeLessThanOrEqual(1);
    expect(Math.abs((fig.left + fig.right) / 2 - (col.left + col.right) / 2)).toBeLessThanOrEqual(0.5);
  });
});
