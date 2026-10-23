// One alignment grid: a full-width band's text and a closing picture's words
// start where the page's text column starts, and stay within its width, at
// desktop and phone width, with the default content width and `full`.
import { test, expect, type Page } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';
import { BAND_TEXT, BODY_TEXT } from '../../e2e/helpers/content-grid-site';

const measure = (page: Page) =>
  page.evaluate(
    ([band, body]) => {
      const edges = (el: Element) => {
        const r = el.getBoundingClientRect();
        return { left: r.left, right: r.right };
      };
      const para = (scope: string, text: string) =>
        edges([...document.querySelectorAll(`${scope} p`)].find((e) => e.textContent!.includes(text))!);
      const hero = document.querySelector('footer .moss-hero-closing')!;
      return {
        body: para('article > ', body),
        band: para('article [data-width] >', band),
        heading: edges(hero.querySelector('h2')!),
        link: edges(hero.querySelector('.moss-hero-content a')!),
      };
    },
    [BAND_TEXT, BODY_TEXT],
  );

for (const key of ['default', 'full'] as const) {
  for (const width of [1360, 900, 390]) {
    test(`${key} width, ${width}px: band text and closing words share the text column's start`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(`http://localhost:${gatePort(`content-grid:${key}`)}/`, { waitUntil: 'load' });
      const m = await measure(page);
      expect(m.band.left, 'band text').toBeCloseTo(m.body.left, 0);
      expect(m.heading.left, 'closing heading').toBeCloseTo(m.body.left, 0);
      expect(m.link.left, 'closing link').toBeCloseTo(m.body.left, 0);
      expect(m.band.right, 'band text stops at the column').toBeLessThanOrEqual(m.body.right + 1);
      expect(m.heading.right, 'closing heading stops at the column').toBeLessThanOrEqual(m.body.right + 1);
    });
  }
}

// With sidenotes the page gives up room on the right (`--moss-sidenote-inset`), and the
// text column re-centres in what is left: the band's text and the words follow it.
for (const key of ['default', 'full'] as const) {
  test(`${key} width with sidenotes, 1250px: still one left edge`, async ({ page }) => {
    await page.setViewportSize({ width: 1250, height: 800 });
    await page.goto(`http://localhost:${gatePort(`content-grid:${key}`)}/notes/`, { waitUntil: 'load' });
    const inset = await page.evaluate(() => parseFloat(getComputedStyle(document.body).paddingRight));
    if (key === 'default') expect(inset, 'the page reserves sidenote room here').toBeGreaterThan(0);
    const m = await measure(page);
    expect(m.band.left, 'band text').toBeCloseTo(m.body.left, 0);
    expect(m.heading.left, 'closing heading').toBeCloseTo(m.body.left, 0);
  });
}

// `align=end` puts the words at the column's end edge; `align=center` centres the box.
for (const width of [1360, 900]) {
  test(`align=end at ${width}px: the words end at the text column's end`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto(`http://localhost:${gatePort('content-grid:end')}/`, { waitUntil: 'load' });
    const m = await measure(page);
    const box = await page.evaluate(() => document.querySelector('footer .moss-hero-closing .moss-hero-content p')!.getBoundingClientRect().right);
    expect(box, 'link row ends at the column').toBeLessThanOrEqual(m.body.right + 1);
    expect(m.heading.right, 'heading reaches the column end').toBeCloseTo(m.body.right, 0);
  });
  test(`align=center at ${width}px: the words box is centred`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.goto(`http://localhost:${gatePort('content-grid:center')}/`, { waitUntil: 'load' });
    const c = await page.evaluate(() => {
      const r = document.querySelector('footer .moss-hero-closing .moss-hero-content')!.getBoundingClientRect();
      return { mid: (r.left + r.right) / 2, vw: document.documentElement.clientWidth };
    });
    expect(c.mid).toBeCloseTo(c.vw / 2, 0);
  });
}

// The ink measure (`hero_ink.rs`) samples, for a closing picture, columns from the start to
// REACH of the width and rows to the box's real height; the words' box must lie inside.
// REACH is the measure's reach at the layout's narrowest viewport; the words box
// only moves inward from there. The 22rem is the measure's WORDS_PX.
const REACH = (width: number, key: 'default' | 'full') => (width < 768 ? 1 : width < 1280 ? 627 / 768 : key === 'full' ? 608 / 1280 : 829 / 1280);
for (const key of ['default', 'full'] as const) {
  for (const width of [390, 768, 800, 900, 1000, 1279, 1280, 1360]) {
    test(`${key} width, ${width}px: the words box lies inside what the ink measure samples`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(`http://localhost:${gatePort(`content-grid:${key}`)}/`, { waitUntil: 'load' });
      const m = await page.evaluate(() => {
        const hero = document.querySelector('footer .moss-hero-closing')!;
        const b = hero.querySelector('.moss-hero-content')!.getBoundingClientRect();
        const h = hero.getBoundingClientRect();
        return { left: b.left, right: b.right, height: b.height, heroHeight: h.height, vw: document.documentElement.clientWidth, rem: parseFloat(getComputedStyle(document.documentElement).fontSize) };
      });
      expect(m.right, 'right edge within the sampled columns').toBeLessThanOrEqual(m.vw * REACH(width, key) + 1);
      if (width >= 768) {
        expect(m.height, 'words box no taller than 22rem').toBeLessThanOrEqual(22 * m.rem);
        expect(m.height, 'and under 60% of the box').toBeLessThanOrEqual(m.heroHeight * 0.6);
      }
    });
  }
}
