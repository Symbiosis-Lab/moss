/**
 * `[site].header = "none"`: no header band, a pinned island that carries the
 * controls, and the contents ruler's scope — the assertions that need a real
 * engine, on sites the moss CLI built.
 *
 * Fixture: playwright/header-none.config.ts → tests/e2e/helpers/header-none-site.ts
 * Run via: bash scripts/render-gates.sh header-none
 */
import { test, expect, type Page } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';
import { AFTER_END, BAND_PATH, BAND_SECTIONS, CLOSING_PATH, COLLECTIONS, LONG_TITLE, LONG_TITLE_PATH, BAND_TINT, PICTURES_PATH, UPRIGHT_PATH, PICTURE_SECTIONS, SITE_SUBTITLE } from '../../e2e/helpers/header-none-site';

const BAND_PAGE = `http://localhost:${gatePort('header-none:band')}${BAND_PATH}`;
const ALL = [...COLLECTIONS, AFTER_END];

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

async function open(page: Page, url = '/', width = 1600, height = 900): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(url);
  await page.waitForSelector('.moss-nav-island', { state: 'attached' });
  await page.evaluate(() => document.fonts.ready);
  await frames(page);
}

async function scrollTo(page: Page, y: number): Promise<void> {
  await page.evaluate((target) => window.scrollTo(0, target), y);
  await frames(page);
}

/** Park a section's heading where a fragment jump would. */
async function jumpTo(page: Page, index: number): Promise<void> {
  await page.evaluate((i) => {
    document.querySelectorAll<HTMLElement>('main h2[id]')[i].scrollIntoView({ block: 'start' });
  }, index);
  await frames(page);
}

/** Put the top of `selector`'s element at `fraction` of the window height. */
async function placeTop(page: Page, selector: string, fraction: number): Promise<void> {
  await page.evaluate(
    ([sel, f]) => {
      const el = document.querySelector(sel as string)!;
      window.scrollTo(0, el.getBoundingClientRect().top + window.scrollY - (f as number) * window.innerHeight);
    },
    [selector, fraction] as const,
  );
  await frames(page);
}

const island = (page: Page) => page.locator('.moss-nav-island');
const shown = (page: Page) => island(page).evaluate((el) => (el as HTMLElement).dataset.shown);
const isAway = (page: Page) => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-away'));
const rulerOn = (page: Page) => island(page).evaluate((el) => (el as HTMLElement).dataset.ruler);
/** Widths with the contents ruler on screen beside the text, and without it. */
const WIDE = 1600;
const NARROW = 900;
const nameOf = (page: Page) =>
  page.locator('.moss-nav-island-section-name').evaluate((el) => (el.textContent ?? '').trim());

test.describe('a one-page site with header = "none"', () => {
  test.beforeEach(async ({ page }) => {
    await open(page);
  });

  test('has no header element, and keeps the skip link', async ({ page }) => {
    expect(await page.locator('header').count()).toBe(0);
    expect(await page.locator('.main-nav').count()).toBe(0);
    await expect(page.locator('.moss-skip-link')).toHaveAttribute('href', '#main-content');
  });

  test('the island names the site, and one theme toggle follows it, with no trail', async ({ page }) => {
    expect(await page.locator('.nav-theme-btn').count()).toBe(1);
    expect(await page.locator('[id="theme-toggle__classic__cutout"]').count(), 'the icon\'s id is unique').toBe(1);
    expect(await page.locator('.moss-nav-island + .nav-icons .nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.moss-nav-island + .nav-icons .nav-search-btn').count(), 'search follows the island too').toBe(1);
    expect(await page.locator('.moss-nav-island + .nav-icons .nav-lang-link').count(), 'and the language links').toBe(1);
    expect(await island(page).locator('.nav-icons, .nav-search-btn, .nav-lang-toggle').count(), 'none of them inside it').toBe(0);
    expect(await page.locator('.moss-nav-island-trail').count()).toBe(0);
    await expect(island(page).locator('.site-name')).toHaveText('Shelf');
    await expect(island(page).locator('.site-name')).toHaveAttribute('href', '/');

    await jumpTo(page, 1);
    const theme = () => page.evaluate(() => document.documentElement.getAttribute('data-theme'));
    const before = await theme();
    await page.locator('.nav-theme-btn').click();
    await expect.poll(theme).not.toBe(before);
    await page.locator('.nav-theme-btn').click();
    await expect.poll(theme).toBe(before);
  });

  test('is hidden at the top, appears at the first section, and stays to the end', async ({ page }) => {
    await open(page, '/', NARROW, 900);
    expect(await page.locator('.moss-contents-ruler').evaluate((el) => (el as HTMLElement).hidden), 'no ruler at this width').toBe(true);
    expect(await shown(page)).toBe('false');
    expect(await island(page).evaluate((el) => (el as HTMLElement).inert)).toBe(true);
    await scrollTo(page, 60);
    expect(await shown(page), 'still on the title area').toBe('false');

    await jumpTo(page, 0);
    expect(await shown(page)).toBe('true');
    await expect.poll(() => island(page).evaluate((el) => getComputedStyle(el).opacity)).toBe('1');

    // Down, then up a little, then to the very bottom: it never leaves.
    for (const k of [2, 4]) {
      await jumpTo(page, k);
      expect(await shown(page), `at section ${k}`).toBe('true');
    }
    await scrollTo(page, 1e6);
    expect(await shown(page), 'at the bottom').toBe('true');
    await page.evaluate(() => window.scrollBy(0, -200));
    await frames(page);
    expect(await shown(page), 'after scrolling up a little').toBe('true');

    // And back above the first section it is put away again.
    await scrollTo(page, 0);
    expect(await shown(page)).toBe('false');
  });

  test('the button is labelled with the current section and opens the panel, pictures aside', async ({ page }) => {
    await open(page, '/', NARROW, 900);
    for (const k of [0, 2, 4]) {
      await jumpTo(page, k);
      expect(await nameOf(page), `section ${k}`).toBe(COLLECTIONS[k]);
    }
    // A section is current once its heading passes 35% of the window.
    await placeTop(page, 'main h2[id="nets"]', 0.3);
    expect(await nameOf(page)).toBe(COLLECTIONS[2]);
    await placeTop(page, 'main h2[id="nets"]', 0.45);
    expect(await nameOf(page)).toBe(COLLECTIONS[1]);
    const button = page.locator('.moss-nav-island-sections');
    await expect(button).toBeVisible();
    await button.click();
    const rows = page.locator('[data-island-menu="sections"] a');
    // Headings after the marker are in the panel as well.
    expect(await rows.allTextContents()).toEqual(ALL);
  });

  test('the home page shows its own title and subtitle, for nothing else names the site', async ({ page }) => {
    const title = page.locator('main h1.moss-folder-title');
    await expect(title).toHaveText('Shelf');
    await expect(title).toBeVisible();
    expect(await title.evaluate((el) => el.classList.contains('visually-hidden'))).toBe(false);
    const box = await title.boundingBox();
    expect(box!.width).toBeGreaterThan(20);
    expect(box!.height).toBeGreaterThan(20);
    const subtitle = page.locator('main .moss-subtitle');
    await expect(subtitle).toHaveText(SITE_SUBTITLE);
    expect((await subtitle.boundingBox())!.y, 'the subtitle follows the title').toBeGreaterThan(box!.y);
  });

  test('the ruler lists the main sections, is up inside them and steps aside past the marker', async ({ page }) => {
    expect(await page.locator('.moss-contents-ruler a').count()).toBe(COLLECTIONS.length);

    await jumpTo(page, 2);
    await expect.poll(() => isAway(page)).toBe(false);

    // The marker passes the middle of the window: the ruler goes, the island stays.
    await placeTop(page, '[data-contents-end]', 0.3);
    await expect.poll(() => isAway(page)).toBe(true);
    expect(await shown(page)).toBe('true');
    // The section after the marker still names itself on the island's button.
    await jumpTo(page, 5);
    expect(await nameOf(page)).toBe(AFTER_END);
    expect(await isAway(page)).toBe(true);

    // Scrolling back above the marker brings the ruler back.
    await jumpTo(page, 3);
    await expect.poll(() => isAway(page)).toBe(false);
  });

  test('the bar is hidden wherever the ruler shows, and back where it does not', async ({ page }) => {
    const bar = () =>
      island(page).evaluate((el) => ({
        visible: getComputedStyle(el).visibility === 'visible' && getComputedStyle(el).opacity !== '0',
        inert: (el as HTMLElement).inert,
      }));
    for (const k of [0, 2, 4]) {
      await jumpTo(page, k);
      await expect.poll(() => rulerOn(page), `section ${k}`).toBe('on');
      await expect.poll(bar, `the ruler is up at section ${k}`).toEqual({ visible: false, inert: true });
    }
    await placeTop(page, '[data-contents-end]', 0.3);
    await expect.poll(() => isAway(page)).toBe(true);
    await expect.poll(bar).toEqual({ visible: true, inert: false });
    await jumpTo(page, 2);
    await expect.poll(bar).toEqual({ visible: false, inert: true });
  });
});

test.describe('a page that ends on a closing picture, with no marker', () => {
  test('keeps the ruler until the closing picture comes into view, then the bar', async ({ page }) => {
    await open(page, CLOSING_PATH, WIDE, 900);
    await jumpTo(page, 2);
    await expect.poll(() => isAway(page)).toBe(false);
    await placeTop(page, 'footer .moss-hero-closing', 1.05);
    await expect.poll(() => isAway(page), 'the picture is just below the window').toBe(false);
    await placeTop(page, 'footer .moss-hero-closing', 0.9);
    // The article's end is still below the middle of the window, which alone
    // would have kept the ruler up.
    expect(await page.evaluate(() => document.querySelector('main')!.getBoundingClientRect().bottom > innerHeight / 2)).toBe(true);
    await expect.poll(() => isAway(page), 'its top is in view').toBe(true);
    await expect.poll(() => shown(page)).toBe('true');
  });
});

/** The theme toggle's place and visibility at one scroll position. */
const toggleState = (page: Page) =>
  page.evaluate(() => {
    const visible = (el: Element) => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth && cs.visibility === 'visible' && Number(cs.opacity) > 0 && !el.closest('[inert]');
    };
    const toggles = [...document.querySelectorAll('.nav-theme-btn, .theme-toggle-icon')].filter((el) => el.matches('.nav-theme-btn') || !el.closest('.nav-theme-btn'));
    const t = document.querySelector('.moss-nav-island + .nav-icons')!;
    const r = t.getBoundingClientRect();
    const at = (sel: string) => {
      const b = document.querySelector(sel)!.getBoundingClientRect();
      return { left: b.left, right: b.right, top: b.top };
    };
    const bar = document.querySelector('.moss-nav-island-bar')!.getBoundingClientRect();
    const island = document.querySelector<HTMLElement>('.moss-nav-island')!;
    return {
      visible: toggles.filter(visible).length,
      search: visible(document.querySelector('.nav-search-btn')!),
      lang: visible(document.querySelector('.nav-lang-link')!),
      stack: ['.nav-search-btn', '.nav-lang-toggle', '.nav-theme-btn'].map(at),
      position: getComputedStyle(t).position,
      top: island.hasAttribute('data-top'),
      ruler: island.dataset.ruler,
      lane: island.dataset.ruler === 'lane',
      box: { left: r.left, right: r.right, top: r.top, bottom: r.bottom },
      bar: { right: bar.right, top: bar.top, bottom: bar.bottom },
      scrollY,
    };
  });

test.describe('the controls at the top of a page with no header', () => {
  for (const width of [1360, 390]) {
    test(`at ${width}px their centre is the title's first line and their end is the column's end`, async ({ page }) => {
      await open(page, CLOSING_PATH, width, 800);
      const m = await page.evaluate(() => {
        const icons = document.querySelector('.moss-nav-island + .nav-icons')!.getBoundingClientRect();
        const theme = document.querySelector('.moss-nav-island + .nav-icons .nav-theme-btn')!.getBoundingClientRect();
        const article = document.querySelector('main article')!;
        const title = document.querySelector('main article > h1')!;
        const range = document.createRange();
        range.selectNodeContents(title);
        const line = range.getClientRects()[0];
        const box = article.getBoundingClientRect();
        const style = getComputedStyle(article);
        return {
          scrollY,
          themeCentre: (theme.top + theme.bottom) / 2 + scrollY,
          lineCentre: (line.top + line.bottom) / 2 + scrollY,
          end: Math.max(icons.right, theme.right),
          columnEnd: box.right - parseFloat(style.paddingRight) - parseFloat(style.borderRightWidth),
        };
      });
      expect(m.scrollY).toBe(0);
      expect(Math.abs(m.themeCentre - m.lineCentre), JSON.stringify(m)).toBeLessThanOrEqual(1);
      expect(Math.abs(m.end - m.columnEnd), JSON.stringify(m)).toBeLessThanOrEqual(1);
    });
  }
});

test.describe('one theme toggle', () => {
  for (const [width, path] of [[WIDE, '/'], [NARROW, '/'], [WIDE, CLOSING_PATH], [390, '/'], [1360, PICTURES_PATH]] as const) {
    test(`at ${width}px on ${path}, exactly one is visible at every scroll position`, async ({ page }) => {
      await open(page, path, width, 800);
      const height = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
      const stops = [0, 10, 20, 21, 22, 24, 40, 48, 60, 120, 300];
      for (let y = 400; y < height; y += 250) stops.push(y);
      stops.push(height);
      const places = new Set<string>();
      for (const y of stops) {
        await scrollTo(page, y);
        await frames(page);
        const s = await toggleState(page);
        expect(s.visible, `at ${y}px: ${JSON.stringify(s)}`).toBe(1);
        expect([s.search, s.lang], `search and language at ${y}px`).toEqual([true, true]);
        for (const c of s.stack) {
          expect(c.left, `a control starts inside the window at ${y}px`).toBeGreaterThanOrEqual(0);
          expect(c.right, `and ends inside it`).toBeLessThanOrEqual(width);
        }
        places.add(s.top ? 'top' : s.ruler === 'on' || s.lane ? 'list' : 'bar');
      }
      const want = path === PICTURES_PATH ? ['list', 'top'] : width === WIDE ? ['bar', 'list', 'top'] : ['bar', 'top'];
      expect([...places].sort(), 'it has been in each of its places').toEqual(want);
      await expect(page.getByRole('button', { name: 'Toggle theme' })).toHaveCount(1);
    });
  }

  test('in each place it is where the design puts it', async ({ page }) => {
    // The island fades in; without motion its box is final at once.
    await page.emulateMedia({ reducedMotion: 'reduce' });
    // At the top: the end of the bar's place, scrolling with the page.
    await open(page, '/', NARROW, 800);
    let s = await toggleState(page);
    expect(s.top).toBe(true);
    expect(s.position).toBe('absolute');
    await scrollTo(page, 20);
    const scrolled = await toggleState(page);
    expect(scrolled.box.top, 'it scrolls away with the page').toBeCloseTo(s.box.top - 20, 0);
    // In the bar: the slot the bar keeps free at its end, centred on its height.
    await jumpTo(page, 2);
    s = await toggleState(page);
    expect(s.position).toBe('fixed');
    expect(s.bar.right - s.box.right, 'its padding and border from the bar\'s end').toBeCloseTo(9, 0);
    expect(s.stack[0].top, 'in a row').toBeCloseTo(s.stack[2].top, 0);
    expect((s.box.top + s.box.bottom) / 2).toBeCloseTo((s.bar.top + s.bar.bottom) / 2, 0);
    const name = await page.locator('.moss-nav-island-sections').boundingBox();
    expect(name!.x + name!.width, 'the bar\'s contents stop before it').toBeLessThanOrEqual(s.box.left);
    // Under the ruler: the foot of the window, on the dashes' column.
    await open(page, '/', WIDE, 800);
    await jumpTo(page, 2);
    await expect.poll(() => rulerOn(page)).toBe('on');
    s = await toggleState(page);
    const dash = await page.locator('.moss-contents-ruler-dash').first().boundingBox();
    for (const c of s.stack) expect(c.left, 'stacked from where the dashes start').toBeCloseTo(dash!.x, 0);
    expect(s.stack[0].top).toBeLessThan(s.stack[1].top);
    expect(s.stack[1].top).toBeLessThan(s.stack[2].top);
    expect(s.box.bottom).toBeGreaterThan(800 - 40);
    expect(s.box.bottom).toBeLessThanOrEqual(800);
  });

  test('its name and its place in the focus order are right in each place', async ({ page }) => {
    const tab = test.info().project.name === 'webkit' ? 'Alt+Tab' : 'Tab';
    /** The accessible names of the first `n` stops after the skip link. */
    const order = async (n: number): Promise<string[]> => {
      await page.locator('.moss-skip-link').focus();
      const names: string[] = [];
      for (let i = 0; i < n; i++) {
        await page.keyboard.press(tab);
        names.push(await page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          if (el.matches('.nav-theme-btn')) return `toggle:${el.getAttribute('aria-label')}`;
          if (el.matches('.nav-search-btn')) return 'search';
          if (el.matches('.nav-lang-link')) return 'lang';
          return el.closest('.moss-nav-island') ? `island:${(el.textContent ?? '').trim()}` : 'page';
        }));
      }
      return names;
    };
    await open(page, '/', NARROW, 800);
    const controls = ['search', 'lang', 'toggle:Toggle theme'];
    expect(await order(4), 'at the top: first, as a header\'s controls would be').toEqual([...controls, 'page']);
    await jumpTo(page, 2);
    const inBar = await order(5);
    expect(inBar[0]).toBe('island:Shelf');
    expect(inBar[1]).toMatch(/^island:/);
    expect(inBar.slice(2), 'in the bar: after the bar\'s own controls').toEqual(controls);
    await open(page, '/', WIDE, 800);
    await jumpTo(page, 2);
    await expect.poll(() => rulerOn(page)).toBe('on');
    expect(await order(4), 'under the ruler: the hidden bar is skipped').toEqual([...controls, 'page']);
  });
});

test.describe('a contents: pictures page reserves a lane for its list', () => {
  /** Where the list, the bar and the text are at one scroll position. */
  const state = (page: Page) =>
    page.evaluate(() => {
      const nav = document.querySelector<HTMLElement>('.moss-contents-ruler')!;
      const island = document.querySelector<HTMLElement>('.moss-nav-island')!;
      const ist = getComputedStyle(island);
      return {
        list: !nav.hidden && !nav.hasAttribute('data-away'),
        bar: island.dataset.shown === 'true' || (ist.visibility === 'visible' && ist.opacity !== '0' && !island.inert),
        lane: island.dataset.ruler === 'on' || island.dataset.ruler === 'lane',
        textLeft: document.querySelector('main h2[id]')!.getBoundingClientRect().left,
        listRight: nav.querySelector('ol')!.getBoundingClientRect().right,
      };
    });

  for (const width of [1100, 1360]) {
    test(`at ${width}px the list is up and the bar never shows, from the first section to the end`, async ({ page }) => {
      await open(page, PICTURES_PATH, width, 800);
      await jumpTo(page, 0);
      await expect.poll(async () => (await state(page)).list).toBe(true);
      const s = await state(page);
      expect(s.lane).toBe(true);
      expect(s.textLeft - s.listRight, 'the text starts a gutter past the list').toBeGreaterThanOrEqual(24 - 0.5);
      expect(s.textLeft, 'the column narrowed to keep the lane').toBeLessThan(92 + 48);

      const start = await page.evaluate(() => scrollY);
      const end = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
      for (let y = start; y <= end + 199; y += 200) {
        await scrollTo(page, Math.min(y, end));
        const now = await state(page);
        expect(now.bar, `no bar at ${Math.min(y, end)}px`).toBe(false);
      }
      // Over the tinted band the list stays: its words keep the text column.
      await placeTop(page, '.tinted', 0.2);
      expect((await state(page)).list, 'up over a band whose words keep the column').toBe(true);
      expect((await state(page)).bar).toBe(false);
      // A picture as wide as the window covers the lane: the list steps aside, and no bar comes.
      await placeTop(page, 'main figure[data-width="screen"]', 0.1);
      await expect.poll(async () => (await state(page)).list).toBe(false);
      expect((await state(page)).bar).toBe(false);
      // And at the closing picture.
      await placeTop(page, 'footer .moss-hero-closing', 0.3);
      await expect.poll(async () => (await state(page)).list).toBe(false);
      expect((await state(page)).bar).toBe(false);
    });

    test(`at ${width}px the controls in the lane stay above the closing picture, never over it`, async ({ page }) => {
      await open(page, PICTURES_PATH, width, 800);
      const gap = () =>
        page.evaluate(() => {
          const icons = document.querySelector('.nav-icons')!.getBoundingClientRect();
          return { bottom: icons.bottom, picture: document.querySelector('footer .moss-hero-closing')!.getBoundingClientRect().top, height: innerHeight };
        });
      await placeTop(page, 'footer .moss-hero-closing', 1.5);
      await expect.poll(async () => (await gap()).height - (await gap()).bottom, 'at rest they stand a space above the window foot').toBeLessThan(40);
      for (const at of [0.9, 0.6, 0.3]) {
        await placeTop(page, 'footer .moss-hero-closing', at);
        await expect.poll(async () => { const g = await gap(); return g.bottom <= g.picture + 0.5; }, `the picture's top is at ${at} of the window`).toBe(true);
      }
    });
  }

  test('the fade behind the open names takes the colour of the band under the list', async ({ page }) => {
    await open(page, PICTURES_PATH, 1360, 800);
    await page.evaluate(() => {
      const band = document.querySelector('.tinted')!.getBoundingClientRect();
      window.scrollTo(0, band.top + band.height / 2 + window.scrollY - innerHeight / 2);
    });
    await frames(page);
    await expect.poll(async () => (await state(page)).list).toBe(true);
    const dash = (await page.locator('.moss-contents-ruler-dash').nth(2).boundingBox())!;
    await page.mouse.move(dash.x + 4, dash.y + 1);
    await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(true);
    const veil = await page.locator('.moss-contents-ruler-veil').evaluate((el) => getComputedStyle(el).backgroundImage);
    expect(veil).toContain(BAND_TINT);
    // Moved off and opened again over the page, it is the page's colour.
    await page.mouse.move(1000, 400);
    await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(false);
    await jumpTo(page, 0);
    await page.mouse.move(dash.x + 4, dash.y + 1);
    await expect.poll(() => page.locator('.moss-contents-ruler').evaluate((el) => el.hasAttribute('data-lens'))).toBe(true);
    const page_ = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    expect(await page.locator('.moss-contents-ruler-veil').evaluate((el) => getComputedStyle(el).backgroundImage)).toContain(page_);
  });

  test('a page without contents: pictures keeps its column', async ({ page }) => {
    await open(page, PICTURES_PATH, 1100, 800);
    const pictures = await page.evaluate(() => ({ lane: getComputedStyle(document.body).getPropertyValue('--moss-contents-lane').trim(), left: document.querySelector('main h2[id]')!.getBoundingClientRect().left }));
    expect(pictures.lane).not.toBe('');
    await page.evaluate(() => document.body.removeAttribute('data-contents'));
    const without = await page.evaluate(() => ({ lane: getComputedStyle(document.body).getPropertyValue('--moss-contents-lane').trim(), left: document.querySelector('main h2[id]')!.getBoundingClientRect().left }));
    expect(without.lane, 'no lane without the field').toBe('');
    expect(without.left, 'the full-width column reaches the page gutter again').toBeLessThan(pictures.left - 40);
  });

  test('a right-to-left or vertical page reserves nothing and keeps the bar', async ({ page }) => {
    for (const [path, set] of [[PICTURES_PATH, "document.documentElement.setAttribute('dir', 'rtl')"], [UPRIGHT_PATH, '']]) {
      await open(page, path, 1360, 800);
      if (set) await page.evaluate(set);
      await frames(page, 4);
      const s = await page.evaluate(() => ({
        lane: getComputedStyle(document.body).getPropertyValue('--moss-contents-lane').trim(),
        marked: document.querySelector<HTMLElement>('.moss-nav-island')!.dataset.ruler !== 'off',
        ruler: document.querySelector<HTMLElement>('.moss-contents-ruler')!.hidden,
      }));
      expect(s, `${path} ${set}`).toEqual({ lane: '', marked: false, ruler: true });
    }
  });
});

test.describe('a contents: pictures page under a coarse pointer', () => {
  test.use({ hasTouch: true, isMobile: true });

  test('reserves no lane, so the bar is the contents list', async ({ page }) => {
    await open(page, PICTURES_PATH, 1360, 800);
    await jumpTo(page, 1);
    const s = await page.evaluate(() => ({
      lane: getComputedStyle(document.body).getPropertyValue('--moss-contents-lane').trim(),
      ruler: document.querySelector<HTMLElement>('.moss-contents-ruler')!.hidden,
      shown: document.querySelector<HTMLElement>('.moss-nav-island')!.dataset.shown,
    }));
    expect(s).toEqual({ lane: '', ruler: true, shown: 'true' });
  });
});

test.describe('at the top, the controls stand at the end of the title line', () => {
  for (const width of [390, 768, 1360]) {
    test(`at ${width}px they touch neither the title nor the subtitle`, async ({ page }) => {
      for (const path of ['/', CLOSING_PATH, LONG_TITLE_PATH, UPRIGHT_PATH]) {
        await open(page, path, width, 800);
        const s = await page.evaluate(() => {
          const lines = (el: Element | null) => {
            if (!el) return [];
            const range = document.createRange();
            range.selectNodeContents(el);
            return [...range.getClientRects()].filter((r) => r.width > 0);
          };
          const controls = [...document.querySelectorAll('.moss-nav-island + .nav-icons > *')].map((el) => el.getBoundingClientRect());
          const text = [...lines(document.querySelector('main > article > h1')), ...lines(document.querySelector('main .moss-subtitle'))];
          const hits = controls.flatMap((c) =>
            text.filter((t) => c.left < t.right && t.left < c.right && c.top < t.bottom && t.top < c.bottom).map((t) => [c.left, c.top, t.left, t.right, t.top]),
          );
          const title = document.querySelector('main > article > h1')!.getBoundingClientRect();
          return { top: document.querySelector('.moss-nav-island')!.hasAttribute('data-top'), hits, controls: controls.length, beside: controls.every((c) => c.top < title.bottom) };
        });
        expect(s.top, `${path}: at the top`).toBe(true);
        expect(s.controls).toBe(3);
        expect(s.hits, `${path} at ${width}px`).toEqual([]);
        if (path !== UPRIGHT_PATH) expect(s.beside, `${path}: on the title's line, not above or below it`).toBe(true);
      }
    });
  }
});

test.describe('a vertical title keeps no room for the controls', () => {
  for (const width of [390, 1360]) {
    test(`at ${width}px its first column runs the column's full length`, async ({ page }) => {
      await open(page, UPRIGHT_PATH, width, 800);
      const s = await page.evaluate(() => {
        const h = document.querySelector<HTMLElement>('main > article > h1')!;
        const cs = getComputedStyle(h);
        const range = document.createRange();
        range.selectNodeContents(h);
        const lines = [...range.getClientRects()].filter((r) => r.width > 0);
        const first = lines.reduce((a, b) => (b.right > a.right ? b : a));
        const end = h.getBoundingClientRect().bottom - parseFloat(cs.paddingBottom) - parseFloat(cs.borderBottomWidth);
        return { mode: cs.writingMode, columns: new Set(lines.map((r) => Math.round(r.right))).size, gap: end - first.bottom, glyph: parseFloat(cs.fontSize) };
      });
      expect(s.mode).toBe('vertical-rl');
      expect(s.columns, 'the title is longer than one column').toBeGreaterThan(1);
      expect(s.gap, 'no notch at the foot of the first column').toBeLessThan(s.glyph * 1.5);
    });
  }
});

test.describe('without scripting, the title still keeps room for three controls', () => {
  test.use({ javaScriptEnabled: false });

  test('at 390px a long title wraps before them', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 800 });
    await page.goto(LONG_TITLE_PATH);
    const s = await page.evaluate(() => {
      const range = document.createRange();
      range.selectNodeContents(document.querySelector('main > article > h1')!);
      const text = [...range.getClientRects()].filter((r) => r.width > 0);
      const controls = [...document.querySelectorAll('.moss-nav-island + .nav-icons > *')].map((el) => el.getBoundingClientRect());
      return {
        controls: controls.length,
        hits: controls.flatMap((c) => text.filter((t) => c.left < t.right && t.left < c.right && c.top < t.bottom && t.top < c.bottom).map((t) => [c.left, t.right])),
      };
    });
    expect(s.controls).toBe(3);
    expect(s.hits).toEqual([]);
  });
});

test.describe('a headerless page gives its first element the header\'s top space', () => {
  for (const width of [1360, 390]) {
    test(`at ${width}px the home title clears the top edge by a spacing step`, async ({ page }) => {
      await open(page, '/', width, 800);
      const lg = await page.evaluate(() => {
        const probe = document.createElement('div');
        probe.style.blockSize = 'var(--moss-space-lg)';
        document.body.append(probe);
        const h = probe.getBoundingClientRect().height;
        probe.remove();
        return h;
      });
      expect(lg).toBeGreaterThan(16);
      const top = (await page.locator('main h1.moss-folder-title').boundingBox())!.y;
      expect(top, 'the title sits below the top edge').toBeGreaterThanOrEqual(lg - 1);
    });
  }

  test('a page that keeps its header gets no extra space on main', async ({ page }) => {
    await open(page, BAND_PAGE);
    expect(await page.locator('main').evaluate((el) => getComputedStyle(el).paddingBlockStart)).toBe('0px');
  });
});

test.describe('a headerless page with fewer than two sections', () => {
  test('shows the controls from the start, without a sections button', async ({ page }) => {
    await open(page, '/short/');
    expect(await shown(page)).toBe('true');
    await expect(page.locator('.nav-theme-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island-sections')).toBeHidden();
    expect(await page.locator('.moss-contents-ruler').count()).toBe(0);
  });
});

test.describe('a headerless page without JavaScript', () => {
  test.use({ javaScriptEnabled: false });

  test('still shows the island with its controls, at the top of the page', async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 800 });
    await page.goto('/');
    await expect(page.locator('.moss-nav-island + .nav-icons .nav-theme-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island + .nav-icons .nav-search-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island-sections')).toBeHidden();
    const box = await page.locator('.moss-nav-island').boundingBox();
    expect(box!.y).toBeLessThan(40);
    const toggle = await page.locator('.nav-theme-btn').boundingBox();
    expect(toggle!.y, 'the toggle sits on the bar').toBeLessThan(box!.y + box!.height);
  });
});

test.describe('a site that keeps its header', () => {
  test('has the header, the controls in it, and an island that is not pinned', async ({ page }) => {
    await open(page, BAND_PAGE);
    expect(await page.locator('header .main-nav').count()).toBe(1);
    expect(await page.locator('header .nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.moss-nav-island .nav-theme-btn').count()).toBe(0);
    expect(await page.locator('.moss-nav-island-trail').count()).toBe(1);
    expect(await island(page).getAttribute('data-header')).toBeNull();

    // Scrolling down hides it, as before.
    await jumpTo(page, 2);
    await page.evaluate(() => window.scrollBy(0, 400));
    await frames(page);
    expect(await shown(page)).toBe('false');
  });
});

test.describe('the plain ruler and a full-width band', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, BAND_PAGE);
  });

  test('stays over a band whose words keep the column, and steps aside past the end marker', async ({ page }) => {
    expect(await page.locator('main div[data-width="screen"]').count()).toBe(1);
    await jumpTo(page, 1);
    await expect.poll(() => isAway(page)).toBe(false);

    // The band crosses the middle of the window, where the ruler sits. Its
    // words keep the text column, so it covers nothing and the ruler stays.
    await placeTop(page, 'main div[data-width="screen"]', 0.4);
    await frames(page);
    expect(await isAway(page)).toBe(false);

    // A picture as wide as the window reaches the margin through its own
    // child: the ruler steps aside while it overlaps, and returns after.
    await placeTop(page, 'main figure[data-width="screen"]', 0.3);
    await expect.poll(() => isAway(page)).toBe(true);
    await jumpTo(page, 0);
    await expect.poll(() => isAway(page)).toBe(false);

    // A heading can carry the end marker too: the ruler lists the sections
    // before it, and steps aside once it passes the middle of the window.
    const marker = page.locator('main h2[data-contents-end]');
    expect(await marker.evaluate((el) => ({ id: el.id, text: (el.textContent ?? '').replace('#', '').trim() }))).toEqual({
      id: 'afterword',
      text: 'Afterword',
    });
    expect(await page.locator('.moss-contents-ruler a').count()).toBe(BAND_SECTIONS.length - 1);
    await placeTop(page, 'main h2[data-contents-end]', 0.3);
    await expect.poll(() => isAway(page)).toBe(true);

    // And before it, above the ruler, too.
    await jumpTo(page, 0);
    await expect.poll(() => isAway(page)).toBe(false);
    expect(BAND_SECTIONS.length).toBeGreaterThan(3);
  });
});

test.describe('the island names a long section in the room the bar has', () => {
  /** The label's text, whether it is clipped, and the bar's room left of the buttons. */
  const label = (page: Page) =>
    page.evaluate(() => {
      const name = document.querySelector<HTMLElement>('.moss-nav-island-section-name')!;
      const text = (name.textContent ?? '').trim();
      const probe = document.createElement('span');
      probe.style.cssText = 'position:absolute;visibility:hidden;white-space:nowrap';
      const cs = getComputedStyle(name);
      probe.style.font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
      probe.textContent = text;
      document.body.append(probe);
      const need = probe.getBoundingClientRect().width;
      probe.remove();
      return { text, room: name.getBoundingClientRect().width, need, cut: name.scrollWidth > name.clientWidth, ellipsis: cs.textOverflow };
    });

  test('at 1360px the whole title shows, the bar having hundreds of pixels to spare', async ({ page }) => {
    await open(page, '/', 1360, 800);
    await jumpTo(page, COLLECTIONS.indexOf(LONG_TITLE));
    await expect.poll(() => nameOf(page)).toBe(LONG_TITLE);
    const l = await label(page);
    expect(l.cut).toBe(false);
    expect(l.room, 'the label box is wider than the old 16rem button').toBeGreaterThan(300);
    expect(l.need).toBeLessThanOrEqual(l.room);
    const bar = await page.locator('.moss-nav-island-bar').boundingBox();
    expect(bar!.width).toBeGreaterThan(l.room);
  });

  test('at 390px it is cut only because it no longer fits', async ({ page }) => {
    await open(page, '/', 390, 800);
    await jumpTo(page, COLLECTIONS.indexOf(LONG_TITLE));
    await expect.poll(() => nameOf(page)).not.toBe('');
    const l = await label(page);
    expect(l.text, 'the whole title, cut at its end by CSS').toBe(LONG_TITLE);
    expect(l.cut).toBe(l.need > l.room);
    expect(l.ellipsis).toBe('ellipsis');
    const site = await island(page).locator('.site-name').evaluate((el) => ({ text: el.textContent, cut: el.scrollWidth > el.clientWidth }));
    expect(site, 'the site name stays whole').toEqual({ text: 'Shelf', cut: false });
    const bar = await page.locator('.moss-nav-island-bar').boundingBox();
    expect(bar!.x + bar!.width).toBeLessThanOrEqual(390);

    // The controls keep their size and their place at the bar's end.
    const cluster = async () =>
      page.evaluate(() => {
        const bar = document.querySelector('.moss-nav-island-bar')!.getBoundingClientRect();
        const icons = document.querySelector('.moss-nav-island + .nav-icons')!.getBoundingClientRect();
        const button = document.querySelector('.moss-nav-island-sections')!.getBoundingClientRect();
        return { w: icons.width, h: icons.height, fromEnd: bar.right - icons.right, gap: icons.left - button.right };
      });
    const narrow = await cluster();
    await open(page, '/', NARROW, 800);
    await jumpTo(page, COLLECTIONS.indexOf(LONG_TITLE));
    const wide = await cluster();
    expect(narrow.w, 'not squeezed').toBeGreaterThanOrEqual(28);
    expect(narrow.w).toBeCloseTo(wide.w, 0);
    expect(narrow.h).toBeCloseTo(wide.h, 0);
    expect(narrow.fromEnd).toBeCloseTo(wide.fromEnd, 0);
    expect(narrow.gap, 'the section button does not run into the controls').toBeGreaterThanOrEqual(0);
  });
});
