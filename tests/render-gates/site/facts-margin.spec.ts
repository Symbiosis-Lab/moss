// The facts list hangs in the right margin only where the page has a margin
// wide enough for it. A theme that widens the content column leaves less
// margin than the list needs: the list then stays in the flow under the
// title, and nothing on the page scrolls sideways. Only a rendered page
// resolves the content width and the list's geometry, so this measures boxes.
import { test, expect } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';

const widenedUrl = `http://localhost:${gatePort('facts-margin:widened')}/court/`;
const defaultUrl = `http://localhost:${gatePort('facts-margin:default')}/court/`;

test.use({ viewport: { width: 1280, height: 800 } });

const measure = (page) =>
  page.evaluate(() => {
    const facts = document.querySelector('dl.moss-facts') as HTMLElement;
    const title = document.querySelector('article h1') as HTMLElement;
    const f = facts.getBoundingClientRect();
    const t = title.getBoundingClientRect();
    const doc = document.documentElement;
    return {
      scrollWidth: doc.scrollWidth,
      clientWidth: doc.clientWidth,
      factsLeft: f.left,
      factsRight: f.right,
      titleRight: t.right,
      titleBottom: t.bottom,
      factsTop: f.top,
    };
  });

test('a widened column keeps the facts in the flow and nothing overflows', async ({ page }) => {
  await page.goto(widenedUrl, { waitUntil: 'load' });
  const m = await measure(page);
  expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
  expect(m.factsRight).toBeLessThanOrEqual(m.clientWidth);
  expect(m.factsTop).toBeGreaterThanOrEqual(m.titleBottom - 1);
});

test('the default column leaves the facts in the right margin', async ({ page }) => {
  await page.goto(defaultUrl, { waitUntil: 'load' });
  const m = await measure(page);
  expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
  expect(m.factsLeft).toBeGreaterThanOrEqual(m.titleRight);
  expect(m.factsRight).toBeLessThanOrEqual(m.clientWidth);
});

test.describe('labels and values at a desktop width', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test('a one-word label stays on one line, and a long value wraps inside its column', async ({ page }) => {
    await page.goto(defaultUrl, { waitUntil: 'load' });
    const m = await page.evaluate(() => {
      const row = (field: string) => document.querySelector(`dl.moss-facts > div[data-field="${field}"]`) as HTMLElement;
      const dt = row('photographer').querySelector('dt') as HTMLElement;
      const dd = row('reference').querySelector('dd') as HTMLElement;
      const dl = document.querySelector('dl.moss-facts') as HTMLElement;
      return {
        line: parseFloat(getComputedStyle(dt).lineHeight),
        labelHeight: dt.getBoundingClientRect().height,
        valueHeight: dd.getBoundingClientRect().height,
        valueRight: dd.getBoundingClientRect().right,
        listRight: dl.getBoundingClientRect().right,
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      };
    });
    expect(m.labelHeight).toBeLessThan(m.line * 1.5);
    expect(m.valueHeight).toBeGreaterThan(m.line * 1.5);
    expect(m.valueRight).toBeLessThanOrEqual(m.listRight + 0.5);
    expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
  });
});
