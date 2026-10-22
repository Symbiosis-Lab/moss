/**
 * The contents ruler of a page with `contents: pictures`: plain at rest, and
 * magnifying in place around the pointer.
 *
 * What only an engine can answer: where each open row is drawn, that the
 * focused row stays under the pointer as it moves and the list stays put past
 * either end, which row a click follows, how the names fit one line, that the
 * picture keeps its crop, and that nothing on the page is offset.
 *
 * Fixture: playwright/contents-lens.config.ts -> tests/e2e/helpers/contents-lens-site.ts
 * Run via: bash scripts/render-gates.sh contents-lens
 */
import { test, expect, type Page } from '@playwright/test';
import { contrastRatio, relativeLuminance } from '../../e2e/helpers/wcag-contrast';
import { BROKEN_SECTIONS, FOCAL_ROW, FOCUS_POINT, FULL_SECTIONS, LENS_SECTIONS, LENS_WITH_IMAGE, MANY, SHORT_NAME } from '../../e2e/helpers/contents-lens-site';

const LENS = '/essays/lens/';
const PLAIN = '/essays/plain/';
const MANY_PAGE = '/essays/many/';
const FULL_PAGE = '/essays/full/';
const BROKEN_PAGE = '/essays/broken/';
/** contents-lens.ts: rest, open and focused row heights, and the pointer travel per row. */
const REST = 20;
const OPEN_ROW = 26;
const FOCUS_ROW = 74;
const PITCH = 34;
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
  /** The list item: the row's rest box, which never moves. */
  rest: { top: number; centre: number };
  /** The link: the drawn row. */
  box: { top: number; bottom: number; centre: number; height: number };
  dash: { x: number; y: number };
  focus: boolean;
  label: { left: number; right: number; height: number; lineHeight: number; visible: boolean; cut: boolean; ellipsis: string };
  pic: { left: number; shown: boolean; position: string } | null;
}

const rows = (page: Page): Promise<Row[]> =>
  page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>('.moss-contents-ruler a')].map((a) => {
      const li = a.parentElement!.getBoundingClientRect();
      const box = a.getBoundingClientRect();
      const dash = a.querySelector('.moss-contents-ruler-dash')!.getBoundingClientRect();
      const label = a.querySelector<HTMLElement>('.moss-contents-ruler-label')!;
      const lb = label.getBoundingClientRect();
      const ls = getComputedStyle(label);
      const frame = a.querySelector<HTMLElement>('.moss-contents-ruler-pic');
      const fs = frame && getComputedStyle(frame);
      const img = frame?.querySelector('img');
      return {
        href: a.getAttribute('href'),
        rest: { top: li.top, centre: li.top + li.height / 2 },
        box: { top: box.top, bottom: box.bottom, centre: box.top + box.height / 2, height: box.height },
        dash: { x: dash.left, y: dash.top + dash.height / 2 },
        focus: a.hasAttribute('data-focus'),
        label: {
          left: lb.left,
          right: lb.right,
          height: lb.height,
          lineHeight: parseFloat(ls.lineHeight),
          visible: ls.visibility === 'visible' && Number(ls.opacity) > 0,
          cut: label.scrollWidth > label.clientWidth + 0.5,
          ellipsis: ls.textOverflow,
        },
        pic: frame && fs
          ? {
              left: frame.getBoundingClientRect().left,
              shown: fs.display !== 'none' && /^inset\(0(px)?\)$/.test(fs.clipPath),
              position: img ? getComputedStyle(img).objectPosition : '',
            }
          : null,
      };
    }),
  );

const isOpen = (page: Page) => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'));
const focused = async (page: Page): Promise<number> => (await rows(page)).findIndex((r) => r.focus);

/** Wait until the open list has stopped moving. */
async function settle(page: Page): Promise<void> {
  await expect.poll(() => isOpen(page)).toBe(true);
  let previous = '';
  for (let i = 0; i < 120; i++) {
    await frames(page, 2);
    const now = JSON.stringify((await rows(page)).map((r) => [r.box.top.toFixed(2), r.box.height.toFixed(2), r.pic?.shown]));
    if (now === previous) return;
    previous = now;
  }
  throw new Error('the open list never stopped moving');
}

/** Enter the list over row `k`'s dash at its rest centre plus `offset`, and wait for it to open. */
async function enter(page: Page, k: number, offset = 0): Promise<{ x: number; y: number }> {
  const r = (await rows(page))[k];
  const point = { x: r.dash.x + 5, y: r.rest.centre + offset };
  await page.mouse.move(point.x, point.y);
  await settle(page);
  return point;
}

async function leave(page: Page): Promise<void> {
  await page.mouse.move(1000, 2);
  await expect.poll(() => isOpen(page)).toBe(false);
}

test.describe('contents: pictures', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, LENS);
    await intoPage(page);
  });

  test('at rest it is the plain ruler: same dashes, same place, no names, no pictures, nothing offset', async ({ page }) => {
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
    expect(lens.map((r) => [r.dash.x, r.dash.y, r.box.top, r.box.bottom])).toEqual(plain.map((r) => [r.dash.x, r.dash.y, r.box.top, r.box.bottom]));
    expect(lens[0].dash.x).toBeCloseTo(40, 0);
    expect(lens.every((r) => !r.label.visible), 'no name at rest').toBe(true);
    expect(lens.every((r) => !r.pic?.shown), 'no picture at rest').toBe(true);
    expect(lens.every((r) => Math.abs(r.box.centre - r.rest.centre) < 0.5 && Math.abs(r.box.height - REST) < 0.5), 'every row sits in its rest box').toBe(true);
  });

  test('opening on a row focuses it under the pointer; rows grow on a curve around it, every name on one line', async ({ page }) => {
    const point = await enter(page, 3);
    const r = await rows(page);
    expect(r.findIndex((row) => row.focus)).toBe(3);
    expect(Math.abs(r[3].box.centre - point.y), 'the focused row is centred on the pointer').toBeLessThanOrEqual(1);
    expect(r[3].box.height).toBeCloseTo(FOCUS_ROW, 0);
    for (const k of [2, 4]) {
      expect(r[k].box.height, `row ${k}`).toBeGreaterThan(OPEN_ROW + 4);
      expect(r[k].box.height, `row ${k}`).toBeLessThan(FOCUS_ROW - 4);
    }
    expect(r[0].box.height, 'three rows away is the open height').toBeCloseTo(OPEN_ROW, 0);
    expect(r[6].box.height).toBeCloseTo(OPEN_ROW, 0);
    for (let i = 1; i < N; i++) expect(r[i].box.top, `rows ${i - 1} and ${i} tile`).toBeCloseTo(r[i - 1].box.bottom, 0);
    for (const [i, row] of r.entries()) {
      expect(row.label.visible, `name ${i}`).toBe(true);
      expect(row.label.height, `name ${i} is one line`).toBeLessThan(row.label.lineHeight * 1.5);
    }

    // Only the focused row shows its picture, after its name.
    expect(r.filter((row) => row.pic?.shown).map((row) => row.href)).toEqual([r[3].href]);
    expect(r[3].pic!.left).toBeGreaterThan(r[3].label.right);
    expect(r[6].pic, 'a section with no picture has none').toBeNull();

    // The picture is the page's own image at the narrowest width moss made.
    const src = await page.locator('.moss-contents-ruler a').nth(3).locator('img').getAttribute('src');
    const sources = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLImageElement>('main img')].map((img) => ({
        src: img.getAttribute('src'),
        srcset: img.closest('picture')?.querySelector('source')?.getAttribute('srcset') ?? '',
      })),
    );
    const widths = sources[3].srcset.split(',').map((c) => c.trim().split(/\s+/));
    expect(src).toBe(widths.sort((a, b) => parseInt(a[1]) - parseInt(b[1]))[0][0]);
    expect(src).not.toBe(sources[3].src);
  });

  test('the focused name is a little larger than the rest, and medium weight', async ({ page }) => {
    await enter(page, 3);
    const type = await page.$$eval('.moss-contents-ruler-label', (els) =>
      els.map((el) => ({ size: parseFloat(getComputedStyle(el).fontSize), weight: Number(getComputedStyle(el).fontWeight) })),
    );
    const rest = type[0];
    expect(type[3].size - rest.size, 'grows by about two pixels').toBeCloseTo(2, 0);
    expect(type[3].weight).toBe(500);
    expect(rest.weight).toBeLessThan(500);
    expect(type[2].size, 'a neighbour grows part of the way').toBeGreaterThan(rest.size);
    expect(type[2].size).toBeLessThan(type[3].size);
  });

  test('the focused row stays under the pointer while it moves, one row per pitch of travel', async ({ page }) => {
    const start = await enter(page, 1);
    for (const step of [0.5, 1, 1.5, 2, 3, 4, 2.5, 1]) {
      const y = start.y + step * PITCH;
      await page.mouse.move(start.x, y, { steps: 3 });
      await settle(page);
      const r = await rows(page);
      const k = r.findIndex((row) => row.focus);
      expect(k, `after ${step} rows of travel`).toBe(Math.round(1 + step));
      expect(r[k].box.top, `the focused row holds the pointer after ${step}`).toBeLessThanOrEqual(y + 0.5);
      expect(r[k].box.bottom).toBeGreaterThanOrEqual(y - 0.5);
      if (Number.isInteger(step)) expect(Math.abs(r[k].box.centre - y), `centred after ${step}`).toBeLessThanOrEqual(1);
    }
  });

  test('past either end the focus stops and the list stays put', async ({ page }) => {
    for (const [k, dir] of [[N - 2, 1], [1, -1]] as const) {
      const start = await enter(page, k);
      const end = start.y + dir * PITCH;
      await page.mouse.move(start.x, end, { steps: 3 });
      await settle(page);
      const at = await rows(page);
      expect(at.findIndex((row) => row.focus)).toBe(k + dir);
      for (const beyond of [15, 30]) {
        await page.mouse.move(start.x, end + dir * beyond, { steps: 3 });
        await settle(page);
        const now = await rows(page);
        expect(await isOpen(page), `still open ${beyond}px past the end`).toBe(true);
        expect(now.findIndex((row) => row.focus)).toBe(k + dir);
        now.forEach((row, i) => {
          expect(row.box.top, `row ${i}, ${beyond}px past`).toBeCloseTo(at[i].box.top, 0);
          expect(row.box.height).toBeCloseTo(at[i].box.height, 0);
        });
      }
      await leave(page);
    }
  });

  test('a click on the open list goes to the focused row, wherever it lands', async ({ page }) => {
    const start = await enter(page, 1);
    await page.mouse.move(start.x, start.y + 2 * PITCH, { steps: 3 });
    await settle(page);
    expect(await focused(page)).toBe(3);
    // Over the fade, right of the names: not any row's link.
    const veil = await page.locator('.moss-contents-ruler-veil').boundingBox();
    const x = veil!.x + veil!.width - 12;
    await page.mouse.move(x, start.y + 2 * PITCH);
    await frames(page);
    expect(await isOpen(page)).toBe(true);
    await page.mouse.click(x, start.y + 2 * PITCH);
    await frames(page);
    const ids = await page.evaluate(() => [...document.querySelectorAll('main h2[id]')].map((h) => h.id));
    expect(await page.evaluate(() => decodeURIComponent(location.hash.slice(1)))).toBe(ids[3]);
  });

  test('keyboard focus opens it on the focused row, and Escape closes it', async ({ page }) => {
    // WebKit tabs to links only with Alt held.
    const tab = test.info().project.name === 'webkit' ? 'Alt+Tab' : 'Tab';
    for (let i = 0; i < 120; i++) {
      await page.keyboard.press(tab);
      if (await page.evaluate(() => !!document.activeElement?.closest('.moss-contents-ruler'))) break;
    }
    await settle(page);
    let r = await rows(page);
    expect(r.findIndex((row) => row.focus)).toBe(0);
    expect(Math.abs(r[0].box.centre - r[0].rest.centre), 'opened on its own rest row').toBeLessThanOrEqual(1);
    await page.keyboard.press(tab);
    await settle(page);
    r = await rows(page);
    expect(r.findIndex((row) => row.focus)).toBe(1);
    expect(r[1].box.height).toBeCloseTo(FOCUS_ROW, 0);
    const ring = await page.evaluate(() => getComputedStyle(document.activeElement!).outlineStyle);
    expect(ring, 'the focused row is ringed').toBe('solid');

    await page.keyboard.press('Escape');
    await expect.poll(() => isOpen(page)).toBe(false);
    expect(await page.evaluate(() => document.activeElement?.getAttribute('href')), 'focus stays where it was').toBe(r[1].href);
  });

  test('the focused picture keeps the page image\'s focal point', async ({ page }) => {
    const onPage = await page.evaluate((i) => {
      const heading = document.querySelectorAll('main h2[id]')[i];
      let el = heading.nextElementSibling;
      while (el && !el.querySelector('img')) el = el.nextElementSibling;
      return getComputedStyle(el!.querySelector('img')!).objectPosition;
    }, FOCAL_ROW);
    expect(onPage, 'the fixture really is cropped').toBe(FOCUS_POINT);
    await enter(page, FOCAL_ROW);
    const r = await rows(page);
    expect(r[FOCAL_ROW].pic!.shown).toBe(true);
    expect(r[FOCAL_ROW].pic!.position).toBe(FOCUS_POINT);
    expect(r[0].pic!.position, 'an uncropped picture stays centred').toBe('50% 50%');
  });

  test('behind the names is a fade of the page colour, not a panel', async ({ page }) => {
    await enter(page, 3);
    const veil = await page.evaluate(() => {
      const nav = document.querySelector('.moss-contents-ruler')!;
      const shadowed = [...nav.querySelectorAll('*')].filter((el) => getComputedStyle(el).boxShadow !== 'none').length;
      const el = nav.querySelector('.moss-contents-ruler-veil')!;
      const cs = getComputedStyle(el);
      const b = el.getBoundingClientRect();
      return { shadowed, image: cs.backgroundImage, border: cs.borderTopStyle, top: b.top, bottom: b.bottom, left: b.left };
    });
    expect(veil.shadowed, 'nothing in the list casts a shadow').toBe(0);
    expect(veil.image).toMatch(/linear-gradient/);
    expect(veil.border).toBe('none');
    expect(veil.left).toBeLessThanOrEqual(0.5);
    const r = await rows(page);
    expect(veil.top).toBeLessThan(r[0].box.top);
    expect(veil.bottom).toBeGreaterThan(r[N - 1].box.bottom);
  });

  for (const scheme of ['light', 'dark'] as const) {
    test(`every open name clears 4.5:1 on the fade, in ${scheme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await open(page, LENS);
      await intoPage(page);
      await enter(page, 0);
      // Painted by the engine itself, the ink over the page colour, so any
      // transparency in the ink counts and any colour form resolves to sRGB.
      const pairs = await page.evaluate(() => {
        const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true })!;
        const bg = getComputedStyle(document.body).backgroundColor;
        const paint = (css: string) => {
          ctx.fillStyle = bg;
          ctx.fillRect(0, 0, 1, 1);
          ctx.fillStyle = css;
          ctx.fillRect(0, 0, 1, 1);
          return [...ctx.getImageData(0, 0, 1, 1).data.slice(0, 3)];
        };
        const ground = paint(bg);
        return [...document.querySelectorAll('.moss-contents-ruler-label')].map((l) => ({ ink: paint(getComputedStyle(l).color), ground }));
      });
      const lum = ([r, g, b]: number[]) => relativeLuminance(r, g, b);
      pairs.forEach((p, i) => expect(contrastRatio(lum(p.ink), lum(p.ground)), `name ${i}`).toBeGreaterThanOrEqual(4.5));
    });
  }

  test('a right-to-left page keeps the bar instead, until the list mirrors', async ({ page }) => {
    await page.evaluate(() => document.documentElement.setAttribute('dir', 'rtl'));
    await frames(page, 4);
    expect(await page.locator('.moss-contents-ruler').evaluate((el) => (el as HTMLElement).hidden)).toBe(true);
    expect(await page.locator('.moss-nav-island').evaluate((el) => (el as HTMLElement).dataset.ruler)).toBe('off');
  });

  test('moving off the list closes it', async ({ page }) => {
    await enter(page, 2);
    await leave(page);
    const r = await rows(page);
    expect(r.every((row) => !row.focus && Math.abs(row.box.centre - row.rest.centre) < 0.5)).toBe(true);
  });
});

test.describe('without motion', () => {
  test('rows resize around the focus without animating', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await open(page, LENS);
    await intoPage(page);
    const start = await enter(page, 3);
    const durations = await page.evaluate(() =>
      ['.moss-contents-ruler a', '.moss-contents-ruler-pic', '.moss-contents-ruler-veil'].map(
        (sel) => getComputedStyle(document.querySelector(sel)!).transitionDuration,
      ),
    );
    expect(durations.every((d) => /^0s(, 0s)*$/.test(d)), durations.join(' | ')).toBe(true);
    // One frame after a move the drawing is already where it ends up.
    await page.mouse.move(start.x, start.y + PITCH);
    await frames(page, 1);
    const r = await rows(page);
    expect(r.findIndex((row) => row.focus)).toBe(4);
    expect(r[4].box.height).toBeCloseTo(FOCUS_ROW, 0);
    expect(Math.abs(r[4].box.centre - (start.y + PITCH))).toBeLessThanOrEqual(1);
    expect(r[1].box.height).toBeCloseTo(OPEN_ROW, 0);
  });
});

test.describe('forty sections, open taller than the window', () => {
  test('the list fills the window without a gap and the focused row stays on screen', async ({ page }) => {
    const height = 1100;
    await open(page, MANY_PAGE, 1360, height);
    await intoPage(page);
    expect((await rows(page)).length).toBe(MANY);
    for (const k of [0, 20, MANY - 1]) {
      await enter(page, k);
      const r = await rows(page);
      expect(r.findIndex((row) => row.focus)).toBe(k);
      expect(r[k].box.top, `row ${k} on screen`).toBeGreaterThanOrEqual(16 - 0.5);
      expect(r[k].box.bottom).toBeLessThanOrEqual(height - 16 + 0.5);
      expect(r[0].box.top, 'no gap above').toBeLessThanOrEqual(16 + 0.5);
      expect(r[MANY - 1].box.bottom, 'no gap below').toBeGreaterThanOrEqual(height - 16 - 0.5);
      await leave(page);
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

  test('the ruler is up: the dashes need only their own lane', async ({ page }) => {
    const nav = page.locator('.moss-contents-ruler');
    expect(await nav.evaluate((el) => (el as HTMLElement).hidden || getComputedStyle(el).display === 'none')).toBe(false);
    expect(await page.locator('.moss-contents-ruler a').count()).toBe(FULL_SECTIONS.length);
    const gap = await page.evaluate(() => ({
      list: document.querySelector('.moss-contents-ruler ol')!.getBoundingClientRect().right,
      text: document.querySelector('main h2[id]')!.getBoundingClientRect().left,
    }));
    expect(gap.text - gap.list, 'the dashes clear the text').toBeGreaterThanOrEqual(24);
  });

  test('a long name is one line ended with an ellipsis; the link names the whole title', async ({ page }) => {
    await enter(page, 3);
    const r = await rows(page);
    const names = await page.evaluate(() =>
      [...document.querySelectorAll('.moss-contents-ruler a')].map((a) => ({
        label: a.getAttribute('aria-label'),
        title: a.getAttribute('title'),
        text: a.querySelector('.moss-contents-ruler-label')!.textContent,
      })),
    );
    names.forEach((name, i) => {
      expect(name.label, `row ${i}`).toBe(FULL_SECTIONS[i]);
      expect(name.title).toBe(FULL_SECTIONS[i]);
      expect(name.text).toBe(FULL_SECTIONS[i]);
      expect(r[i].label.height, `row ${i} is one line`).toBeLessThan(r[i].label.lineHeight * 1.5);
    });
    expect(r[3].label.cut, 'ninety characters do not fit').toBe(true);
    expect(r[3].label.ellipsis).toBe('ellipsis');
    expect(r[1].label.cut, 'a short title is whole').toBe(false);
    expect(FULL_SECTIONS[3].length).toBeGreaterThan(40);
  });

  test('at rest the strips stop at the lane: the page beside the dashes is not the ruler', async ({ page }) => {
    const at = await rows(page);
    // The strips end a gutter short of the reserved lane's edge, where the text starts.
    const edge = await page.evaluate(() => {
      const probe = document.createElement('div');
      probe.style.cssText = 'position:absolute;inline-size:calc(var(--moss-contents-lane) - var(--moss-contents-gutter))';
      document.body.append(probe);
      const w = probe.getBoundingClientRect().width;
      probe.remove();
      return w;
    });
    const linkAt = (x: number, y: number) =>
      page.evaluate(([px, py]) => document.elementFromPoint(px, py)?.closest('.moss-contents-ruler')?.tagName ?? null, [x, y]);
    expect(await linkAt(edge - 4, at[2].rest.centre), 'inside the lane').toBe('NAV');
    expect(await linkAt(edge + 12, at[2].rest.centre), 'past it, the page').toBeNull();
  });
});

test.describe('the sections panel at 768px', () => {
  test('a row whose picture does not load shows its whole name, and a short name stands for its heading', async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 900 });
    await page.goto(BROKEN_PAGE);
    await page.evaluate(() => document.fonts.ready);
    await page.evaluate(() => document.querySelectorAll<HTMLElement>('main h2[id]')[0].scrollIntoView({ block: 'start' }));
    await frames(page);
    // The bar comes back on a scroll up.
    await page.evaluate(() => window.scrollBy(0, 120));
    await frames(page);
    await page.evaluate(() => window.scrollBy(0, -80));
    await frames(page);
    await page.waitForFunction(() => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true');
    // The thumbnail is lazy: it fails once the panel first opens. Reopened,
    // the row has given up its picture.
    const button = page.locator('.moss-nav-island-sections');
    await button.click();
    await expect.poll(() => page.locator('[data-island-menu="sections"] a').nth(1).evaluate((a) => a.hasAttribute('data-pic'))).toBe(false);
    await button.click();
    await button.click();
    await expect(page.locator('[data-island-menu="sections"]')).toBeVisible();
    const rows = await page.$$eval('[data-island-menu="sections"] a', (as) =>
      as.map((a) => ({ text: a.textContent ?? '', cut: a.scrollWidth > a.clientWidth })),
    );
    expect(rows).toEqual([
      { text: SHORT_NAME, cut: false },
      { text: BROKEN_SECTIONS[1], cut: false },
      { text: BROKEN_SECTIONS[2], cut: false },
    ]);
    const heading = await page.locator('main h2[id]').first().evaluate((h) => ({ id: h.id, label: (h as HTMLElement).dataset.contentsLabel, text: h.firstChild?.textContent }));
    expect(heading).toEqual({ id: 'hours', label: SHORT_NAME, text: BROKEN_SECTIONS[0] });
  });
});
