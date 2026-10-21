/**
 * The contents ruler of a page with `contents: pictures`: plain at rest, and
 * opening like a wheel around the pointer.
 *
 * What only an engine can answer: that the strips under the pointer never
 * move, that the row drawn under the pointer is the strip's own row, how big
 * each row's picture is, and that nothing on the page is offset.
 *
 * Fixture: playwright/contents-lens.config.ts -> tests/e2e/helpers/contents-lens-site.ts
 * Run via: bash scripts/render-gates.sh contents-lens
 */
import { test, expect, type Page } from '@playwright/test';
import { FULL_SECTIONS, LENS_SECTIONS, LENS_WITH_IMAGE, MANY } from '../../e2e/helpers/contents-lens-site';

const LENS = '/essays/lens/';
const PLAIN = '/essays/plain/';
const MANY_PAGE = '/essays/many/';
const FULL_PAGE = '/essays/full/';
const PIC = 40;
const REST = 20;
const N = LENS_SECTIONS.length;

async function frames(page: Page, n = 3): Promise<void> {
  await page.evaluate(
    (count) =>
      new Promise<void>((resolve) => {
        let left = count;
        const step = () => (--left <= 0 ? resolve() : requestAnimationFrame(step));
        requestAnimationFrame(step);
      }),
    n,
  );
}

async function open(page: Page, path: string, width = 1360, height = 800): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForSelector('.moss-contents-ruler', { state: 'attached' });
  await page.evaluate(() => document.fonts.ready);
  await frames(page);
}

/** Put the first section where the ruler belongs on screen, and wait for it to be up. */
async function intoPage(page: Page): Promise<void> {
  await page.evaluate(() => document.querySelectorAll<HTMLElement>('main h2[id]')[1].scrollIntoView({ block: 'start' }));
  await frames(page);
  await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-away'))).toBe(false);
}

interface Row {
  href: string | null;
  strip: { top: number; bottom: number; x: number };
  dash: { y: number; x: number };
  /** Drawn centre, and the drawn height implied by the row's weight. */
  centre: number;
  pic: { width: number; centre: number; visible: boolean; src: string | null } | null;
  labelOpacity: number;
  labelVisible: boolean;
}

const rows = (page: Page): Promise<Row[]> =>
  page.evaluate(
    ({ pic }) =>
      [...document.querySelectorAll<HTMLElement>('.moss-contents-ruler a')].map((a) => {
        const box = a.getBoundingClientRect();
        const dash = a.querySelector('.moss-contents-ruler-dash')!.getBoundingClientRect();
        const label = a.querySelector<HTMLElement>('.moss-contents-ruler-label')!;
        const frame = a.querySelector<HTMLElement>('.moss-contents-ruler-pic');
        const r = frame?.getBoundingClientRect();
        const labelBox = label.getBoundingClientRect();
        const cs = getComputedStyle(label);
        return {
          href: a.getAttribute('href'),
          strip: { top: box.top, bottom: box.bottom, x: box.left },
          dash: { y: dash.top + dash.height / 2, x: dash.left },
          centre: labelBox.top + labelBox.height / 2,
          pic: frame && r
            ? {
                width: r.width,
                centre: r.top + r.height / 2,
                visible: getComputedStyle(frame).visibility === 'visible' && r.width > 0.5,
                src: frame.querySelector('img')?.getAttribute('src') ?? null,
              }
            : null,
          labelOpacity: Number(cs.opacity),
          labelVisible: cs.visibility === 'visible',
        };
      }),
    { pic: PIC },
  );

const stripCentre = (r: Row): number => (r.strip.top + r.strip.bottom) / 2;

/** Hover a point and wait until the ruler is open and the drawing has stopped moving. */
async function settle(page: Page): Promise<void> {
  await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(true);
  let previous = '';
  for (let i = 0; i < 120; i++) {
    await frames(page, 2);
    const now = JSON.stringify((await rows(page)).map((r) => [r.centre.toFixed(2), r.pic?.width.toFixed(2)]));
    if (now === previous) return;
    previous = now;
  }
  throw new Error('the open ruler never stopped moving');
}

async function hoverStrip(page: Page, k: number, offset = 0): Promise<{ x: number; y: number }> {
  const r = (await rows(page))[k];
  const point = { x: r.dash.x + 5, y: stripCentre(r) + offset };
  await page.mouse.move(point.x, point.y);
  await settle(page);
  return point;
}

test.describe('contents: pictures', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, LENS);
    await intoPage(page);
  });

  test('at rest it is the plain ruler: same dashes, same place, no pictures, nothing offset', async ({ page }) => {
    const lens = await rows(page);
    expect(await page.locator('.moss-contents-ruler').getAttribute('data-contents')).toBe('pictures');
    const textLeft = await page.evaluate(() => document.querySelector('main h2[id]')!.getBoundingClientRect().left);
    const body = await page.evaluate(() => ({
      padding: getComputedStyle(document.body).paddingLeft,
      style: document.body.getAttribute('style'),
      scroll: document.documentElement.scrollWidth <= window.innerWidth,
    }));
    expect(body).toEqual({ padding: '0px', style: null, scroll: true });

    await open(page, PLAIN);
    await intoPage(page);
    const plain = await rows(page);
    expect(await page.evaluate(() => document.querySelector('main h2[id]')!.getBoundingClientRect().left), 'the text starts where it does without the field').toBeCloseTo(textLeft, 0);
    expect(lens.map((r) => [r.dash.x, r.dash.y, r.strip.top, r.strip.bottom])).toEqual(
      plain.map((r) => [r.dash.x, r.dash.y, r.strip.top, r.strip.bottom]),
    );
    expect(lens[0].dash.x).toBeCloseTo(40, 0);

    await open(page, LENS);
    await intoPage(page);
    const rest = await rows(page);
    expect(rest.every((r) => !r.labelVisible), 'no title at rest').toBe(true);
    expect(rest.every((r) => !r.pic?.visible), 'no picture at rest').toBe(true);
    expect(rest.every((r) => Math.abs(r.centre - stripCentre(r)) < 1), 'every row sits in its strip').toBe(true);
  });

  test('hovering a row opens it to the picture, its neighbours less, the far rows not at all', async ({ page }) => {
    await hoverStrip(page, 3);
    const r = await rows(page);
    expect(r[3].pic!.width).toBeGreaterThanOrEqual(PIC - 1);
    expect(r[3].pic!.width).toBeLessThanOrEqual(PIC + 0.5);
    for (const k of [2, 4]) {
      expect(r[k].pic!.width, `row ${k}`).toBeGreaterThan(8);
      expect(r[k].pic!.width, `row ${k}`).toBeLessThan(r[3].pic!.width - 5);
    }
    expect(r[0].pic!.visible, 'row 0 is three rows away').toBe(false);
    expect(r[6].pic, 'a section with no picture has none').toBeNull();
    expect(r.every((row) => row.labelVisible && row.labelOpacity >= 0.5), 'every title stays legible').toBe(true);
    expect(r[3].labelOpacity).toBeCloseTo(1, 1);

    // The picture is the page's own image at the narrowest width moss made.
    const sources = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLImageElement>('main img')].map((img) => ({
        src: img.getAttribute('src'),
        srcset: img.closest('picture')?.querySelector('source')?.getAttribute('srcset') ?? '',
      })),
    );
    const widths = sources[3].srcset.split(',').map((c) => c.trim().split(/\s+/));
    const narrowest = widths.sort((a, b) => parseInt(a[1]) - parseInt(b[1]))[0][0];
    expect(r[3].pic!.src).toBe(narrowest);
    expect(r[3].pic!.src).not.toBe(sources[3].src);
  });

  test('the opened row stays centred under the pointer, wherever it is aimed', async ({ page }) => {
    for (const k of [0, 3, N - 1]) {
      const point = await hoverStrip(page, k);
      const r = (await rows(page))[k];
      const centre = r.pic ? r.pic.centre : r.centre;
      expect(Math.abs(centre - point.y), `row ${k}`).toBeLessThanOrEqual(2);
      await page.mouse.move(2, 2);
      await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(false);
    }
  });

  test('the strip under the pointer is the drawn row under it, and the strips never move', async ({ page }) => {
    const at = await rows(page);
    await hoverStrip(page, 0);
    for (let k = 0; k < N; k++) {
      // Over the dash, and over the lane well to its right (the `::after`).
      for (const [dx, offset] of [[5, -8], [5, 0], [5, 8], [90, 0], [90, 7]] as const) {
        const x = at[k].dash.x + dx;
        const y = stripCentre(at[k]) + offset;
        await page.mouse.move(x, y);
        // Straight away, mid-motion: the link under the pointer is the strip's.
        const immediate = await page.evaluate(
          ([px, py]) => document.elementFromPoint(px, py)?.closest('.moss-contents-ruler a')?.getAttribute('href'),
          [x, y],
        );
        expect(immediate, `strip ${k} at ${dx},${offset} before the drawing catches up`).toBe(at[k].href);
        await settle(page);
        const now = await rows(page);
        expect(now.map((r) => [r.strip.top, r.strip.bottom]), 'the strips have not moved').toEqual(at.map((r) => [r.strip.top, r.strip.bottom]));
        // Each drawn row spans its centre +- half its height; they tile, and the pointer is in row k.
        const drawn = now.map((r, j) => {
          const m = r.pic ? Math.min(1, r.pic.width / PIC) : r.labelOpacity > 0 ? 1 : 0;
          const grown = r.pic ? REST + (48 - REST) * m : REST + 4;
          return { j, top: r.centre - grown / 2, bottom: r.centre + grown / 2 };
        });
        const containing = drawn.filter((d) => y >= d.top - 0.75 && y <= d.bottom + 0.75).map((d) => d.j);
        expect(containing, `the pointer at strip ${k} (${offset}) is over drawn row ${k}`).toContain(k);
      }
    }
  });

  test('clicking goes to the section under the pointer', async ({ page }) => {
    await hoverStrip(page, 3, 6);
    const r = (await rows(page))[3];
    await page.mouse.click(r.dash.x + 5, stripCentre(r) + 6);
    await frames(page);
    const ids = await page.evaluate(() => [...document.querySelectorAll('main h2[id]')].map((h) => h.id));
    expect(await page.evaluate(() => decodeURIComponent(location.hash.slice(1)))).toBe(ids[3]);
  });

  test('moving onto a title does not close it: the strip reaches the end of the titles', async ({ page }) => {
    await hoverStrip(page, 2);
    const label = await page.locator('.moss-contents-ruler a').nth(2).locator('.moss-contents-ruler-label').boundingBox();
    const r = (await rows(page))[2];
    await page.mouse.move(label!.x + Math.min(label!.width, 120), stripCentre(r));
    await frames(page, 6);
    expect(await page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(true);
  });

  test('keyboard focus opens it at the focused row', async ({ page }) => {
    // WebKit tabs to links only with Alt held.
    const tab = test.info().project.name === 'webkit' ? 'Alt+Tab' : 'Tab';
    for (let i = 0; i < 120; i++) {
      await page.keyboard.press(tab);
      if (await page.evaluate(() => !!document.activeElement?.closest('.moss-contents-ruler'))) break;
    }
    await settle(page);
    let r = await rows(page);
    expect(r[0].pic!.width).toBeGreaterThanOrEqual(PIC - 1);
    expect(r[1].pic!.width).toBeLessThan(r[0].pic!.width - 5);
    await page.keyboard.press(tab);
    await settle(page);
    r = await rows(page);
    expect(r[1].pic!.width).toBeGreaterThanOrEqual(PIC - 1);
    expect(r[0].pic!.width).toBeLessThan(r[1].pic!.width - 5);
  });

  test('keyboard focus rings the drawn row, not the 20px link', async ({ page }) => {
    const tab = test.info().project.name === 'webkit' ? 'Alt+Tab' : 'Tab';
    for (let i = 0; i < 120; i++) {
      await page.keyboard.press(tab);
      if (await page.evaluate(() => !!document.activeElement?.closest('.moss-contents-ruler'))) break;
    }
    await settle(page);
    const ring = await page.evaluate(() => {
      const a = document.activeElement as HTMLElement;
      const pic = a.querySelector('.moss-contents-ruler-pic')!;
      const label = a.querySelector('.moss-contents-ruler-label')!;
      const accent = (el: Element) => getComputedStyle(el).outlineColor;
      return {
        link: { style: getComputedStyle(a).outlineStyle, color: accent(a) },
        pic: { style: getComputedStyle(pic).outlineStyle, width: getComputedStyle(pic).outlineWidth, color: accent(pic) },
        label: { style: getComputedStyle(label).outlineStyle, color: accent(label) },
      };
    });
    expect(ring.link.style, 'the link keeps an outline').toBe('solid');
    expect(ring.link.color, 'but not a visible one').toMatch(/rgba\(0, 0, 0, 0\)|transparent/);
    expect(ring.pic.style).toBe('solid');
    expect(ring.pic.width).toBe('2px');
    expect(ring.label.style).toBe('solid');
    expect(ring.pic.color, 'the ring colour is the same as the plain ruler\'s').toBe(ring.label.color);
    expect(ring.pic.color).not.toMatch(/rgba\(0, 0, 0, 0\)/);
  });

  test('the open list stays inside the window at both ends', async ({ page }) => {
    const height = page.viewportSize()!.height;
    for (const k of [0, N - 1]) {
      await hoverStrip(page, k);
      for (const r of await rows(page)) {
        expect(r.centre - 12, `row ${r.href} top`).toBeGreaterThanOrEqual(0);
        expect(r.centre + 12, `row ${r.href} bottom`).toBeLessThanOrEqual(height);
      }
    }
  });
});

test.describe('without motion', () => {
  test('hovering shows every row open at once, statically', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await open(page, LENS);
    await intoPage(page);
    const at = await rows(page);
    await page.mouse.move(at[3].dash.x + 5, stripCentre(at[3]));
    await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(true);
    const r = await rows(page);
    r.forEach((row, i) => {
      expect([row.labelVisible, row.labelOpacity], `title ${i}`).toEqual([true, 1]);
      if (LENS_WITH_IMAGE[i]) expect(row.pic!.width, `picture ${i}`).toBeGreaterThanOrEqual(PIC - 1);
    });
    const before = JSON.stringify(r.map((row) => row.centre));
    await page.mouse.move(at[4].dash.x + 5, stripCentre(at[4]));
    await frames(page, 4);
    expect(JSON.stringify((await rows(page)).map((row) => row.centre)), 'the pointer moving changes nothing').toBe(before);
  });
});

test.describe('forty sections', () => {
  test('open the same way, and stay inside the window', async ({ page }) => {
    await open(page, MANY_PAGE, 1360, 1100);
    await intoPage(page);
    const count = (await rows(page)).length;
    expect(count).toBe(MANY);
    for (const k of [0, 20, MANY - 1]) {
      const point = await hoverStrip(page, k);
      const r = await rows(page);
      expect(Math.abs((r[k].pic?.centre ?? r[k].centre) - point.y), `row ${k}`).toBeLessThanOrEqual(2);
      for (const row of r) {
        expect(row.centre - 12).toBeGreaterThanOrEqual(0);
        expect(row.centre + 12).toBeLessThanOrEqual(1100);
      }
      await page.mouse.move(2, 2);
      await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(false);
    }
  });
});

test.describe('at 400px', () => {
  test.use({ viewport: { width: 400, height: 800 } });

  test('nothing changes: no ruler, nothing offset', async ({ page }) => {
    await open(page, LENS, 400, 800);
    expect(await page.locator('.moss-contents-ruler').evaluate((el) => (el as HTMLElement).hidden || getComputedStyle(el).display === 'none')).toBe(true);
    expect(await page.evaluate(() => ({
      padding: getComputedStyle(document.body).paddingLeft,
      style: document.body.getAttribute('style'),
      scroll: document.documentElement.scrollWidth <= window.innerWidth,
    }))).toEqual({ padding: '0px', style: null, scroll: true });
  });
});

test.describe('a full-width page, the narrowest margin a page has', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, FULL_PAGE);
    await intoPage(page);
  });

  interface Title {
    text: string;
    lines: number;
    clipped: boolean;
    box: { left: number; right: number; top: number; bottom: number };
    surface: { background: string; shadow: string; events: string };
    under: string | null;
  }

  const titles = (page: Page): Promise<Title[]> =>
    page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('.moss-contents-ruler-label')].map((label) => {
        const box = label.getBoundingClientRect();
        const cs = getComputedStyle(label);
        const under = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
        return {
          text: label.textContent ?? '',
          lines: Math.round((box.height - 4) / parseFloat(cs.lineHeight)),
          clipped: label.scrollWidth > label.clientWidth,
          box: { left: box.left, right: box.right, top: box.top, bottom: box.bottom },
          surface: { background: cs.backgroundColor, shadow: cs.boxShadow, events: cs.pointerEvents },
          under: under?.closest('.moss-contents-ruler a')?.getAttribute('href') ?? under?.tagName ?? null,
        };
      }),
    );

  test('the ruler is up: the dashes need only their own lane', async ({ page }) => {
    const nav = page.locator('.moss-contents-ruler');
    expect(await nav.evaluate((el) => (el as HTMLElement).hidden || getComputedStyle(el).display === 'none')).toBe(false);
    expect(await nav.evaluate((el) => el.hasAttribute('data-away'))).toBe(false);
    expect(await page.locator('.moss-contents-ruler a').count()).toBe(FULL_SECTIONS.length);
    const gap = await page.evaluate(() => ({
      list: document.querySelector('.moss-contents-ruler ol')!.getBoundingClientRect().right,
      text: document.querySelector('main h2[id]')!.getBoundingClientRect().left,
    }));
    expect(gap.text - gap.list, 'the dashes clear the text').toBeGreaterThanOrEqual(24);
    const rest = await titles(page);
    expect(rest.every((t) => t.under === null || !t.under.startsWith('#')), 'nothing of the ruler is under a hidden title').toBe(true);
  });

  test('an open title is the whole heading on its own surface, wrapped, never cut', async ({ page }) => {
    const textLeft = await page.evaluate(() => document.querySelector('main h2[id]')!.getBoundingClientRect().left);
    let reachedOverText = false;
    for (const k of [0, 2, FULL_SECTIONS.length - 1]) {
      await hoverStrip(page, k);
      const t = await titles(page);
      t.forEach((title, i) => {
        expect(title.text, `title ${i} while row ${k} is focused`).toBe(FULL_SECTIONS[i]);
        expect(title.text).not.toContain('…');
        expect(title.clipped, `title ${i} is clipped`).toBe(false);
        expect(title.surface.background, `title ${i} has a surface`).not.toMatch(/rgba\(0, 0, 0, 0\)|transparent/);
        expect(title.surface.shadow).not.toBe('none');
        expect(title.surface.events, 'the surface takes no pointer').toBe('none');
        reachedOverText ||= title.box.right > textLeft;
      });
      expect(t[0].lines, 'a long title wraps').toBe(2);
      expect(t[3].lines, 'a title of ninety characters is wrapped whole, not cut').toBeGreaterThanOrEqual(3);
      expect(t[1].lines, 'a short one stays on a line').toBe(1);
      // No title box overlaps another.
      const sorted = [...t].sort((a, b) => a.box.top - b.box.top);
      for (let i = 1; i < sorted.length; i++) {
        expect(sorted[i].box.top, `titles ${i - 1} and ${i} overlap`).toBeGreaterThanOrEqual(sorted[i - 1].box.bottom - 0.5);
      }
      await page.mouse.move(2, 2);
      await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(false);
    }
    expect(reachedOverText, 'a title may reach over the page').toBe(true);
  });

  const linkAt = (page: Page, x: number, y: number) =>
    page.evaluate(([px, py]) => document.elementFromPoint(px, py)?.closest('.moss-contents-ruler a')?.getAttribute('href') ?? null, [x, y]);

  test('while open the strips reach every title: the pointer moves onto one and stays, and a click follows it', async ({ page }) => {
    const at = await rows(page);
    for (let k = 0; k < FULL_SECTIONS.length; k++) {
      await hoverStrip(page, k);
      const box = (await titles(page))[k].box;
      const x = (box.left + box.right) / 2;
      const y = stripCentre(at[k]);
      expect(y, `the strip's height is within title ${k}`).toBeGreaterThan(box.top);
      expect(y).toBeLessThan(box.bottom);
      await page.mouse.move(x, y);
      await frames(page, 4);
      expect(await linkAt(page, x, y), `the centre of title ${k} is its row's strip`).toBe(at[k].href);
      expect(await page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens')), `still open on title ${k}`).toBe(true);
      expect((await rows(page)).map((r) => [r.strip.top, r.strip.bottom]), 'the strips have not moved').toEqual(at.map((r) => [r.strip.top, r.strip.bottom]));
      if (k === 3) {
        await page.mouse.click(x, y);
        await frames(page);
        const ids = await page.evaluate(() => [...document.querySelectorAll('main h2[id]')].map((h) => h.id));
        expect(await page.evaluate(() => decodeURIComponent(location.hash.slice(1)))).toBe(ids[3]);
      }
      await page.mouse.move(2, 2);
      await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(false);
    }
  });

  test('at rest the strips stop at the lane: the page beside the dashes is not the ruler', async ({ page }) => {
    const at = await rows(page);
    const lane = await page.locator('.moss-contents-ruler').evaluate((el) => parseFloat(el.style.getPropertyValue('--moss-contents-lane')));
    const x = at[0].dash.x - 0 + lane + 12 - 0;
    const y = stripCentre(at[2]);
    expect(x, 'the probe is still in the margin').toBeLessThan(await page.evaluate(() => document.querySelector('main h2[id]')!.getBoundingClientRect().left));
    expect(await linkAt(page, x, y), 'page content, not the ruler').toBeNull();
    // Open, the same point is the row's strip; closed again, it is the page.
    await hoverStrip(page, 2);
    await page.mouse.move(x, y);
    await frames(page, 4);
    expect(await linkAt(page, x, y)).toBe(at[2].href);
    await page.mouse.move(x + 600, y);
    await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(false);
    expect(await linkAt(page, x, y)).toBeNull();
  });

  test('at rest the titles are not drawn', async ({ page }) => {
    const rest = await rows(page);
    expect(rest.every((r) => !r.labelVisible)).toBe(true);
    expect(rest.every((r) => Math.abs(r.centre - stripCentre(r)) < 1)).toBe(true);
  });
});
