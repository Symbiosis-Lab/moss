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
import { AFTER_END, BAND_PATH, BAND_SECTIONS, COLLECTIONS, LONG_TITLE, SITE_SUBTITLE } from '../../e2e/helpers/header-none-site';

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

  test('the island carries the theme toggle, with no trail and no site name', async ({ page }) => {
    expect(await page.locator('.moss-nav-island .nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.nav-theme-btn').count()).toBe(1);
    expect(await page.locator('.moss-nav-island-trail').count()).toBe(0);
    expect(await island(page).locator('.site-name').count()).toBe(0);

    await jumpTo(page, 1);
    const theme = () => page.evaluate(() => document.documentElement.getAttribute('data-theme'));
    const before = await theme();
    await island(page).locator('.nav-theme-btn').click();
    await expect.poll(theme).not.toBe(before);
    await island(page).locator('.nav-theme-btn').click();
    await expect.poll(theme).toBe(before);
  });

  test('is hidden at the top, appears at the first section, and stays to the end', async ({ page }) => {
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
    for (const k of [0, 2, 4]) {
      await jumpTo(page, k);
      expect(await nameOf(page), `section ${k}`).toBe(COLLECTIONS[k]);
    }
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

  test('the sections button stays on the island while the ruler is up', async ({ page }) => {
    await jumpTo(page, 2);
    await expect.poll(() => isAway(page)).toBe(false);
    expect(await page.locator('.moss-nav-island-sections').evaluate((el) => getComputedStyle(el).display)).not.toBe('none');
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
    await expect(island(page).locator('.nav-theme-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island-sections')).toBeHidden();
    expect(await page.locator('.moss-contents-ruler').count()).toBe(0);
  });
});

test.describe('a headerless page without JavaScript', () => {
  test.use({ javaScriptEnabled: false });

  test('still shows the island with its controls, at the top of the page', async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 800 });
    await page.goto('/');
    await expect(page.locator('.moss-nav-island .nav-theme-btn')).toBeVisible();
    await expect(page.locator('.moss-nav-island-sections')).toBeHidden();
    const box = await page.locator('.moss-nav-island').boundingBox();
    expect(box!.y).toBeLessThan(40);
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

test.describe('the ruler steps aside from a full-width block', () => {
  test.beforeEach(async ({ page }) => {
    await open(page, BAND_PAGE);
  });

  test('while the block overlaps it, and returns once the block has passed', async ({ page }) => {
    expect(await page.locator('main [data-width="screen"]').count()).toBe(1);
    await jumpTo(page, 1);
    await expect.poll(() => isAway(page)).toBe(false);

    // The band crosses the middle of the window, where the ruler sits.
    await placeTop(page, '[data-width="screen"]', 0.4);
    await expect.poll(() => isAway(page)).toBe(true);

    // Below the band again (the band is above the window's middle): back.
    await page.evaluate(() => {
      const band = document.querySelector('[data-width="screen"]')!;
      const ol = document.querySelector('.moss-contents-ruler ol')!.getBoundingClientRect();
      window.scrollTo(0, band.getBoundingClientRect().bottom + window.scrollY - ol.top + 40);
    });
    await frames(page);
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
      return { text, room: name.getBoundingClientRect().width, need, ellipsis: text.includes('…') };
    });

  test('at 1360px the whole title shows, the bar having hundreds of pixels to spare', async ({ page }) => {
    await open(page, '/', 1360, 800);
    await jumpTo(page, COLLECTIONS.indexOf(LONG_TITLE));
    await expect.poll(() => nameOf(page)).toBe(LONG_TITLE);
    const l = await label(page);
    expect(l.ellipsis).toBe(false);
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
    if (l.text === LONG_TITLE) {
      expect(l.need).toBeLessThanOrEqual(l.room);
    } else {
      expect(l.text).toContain('…');
      // Whatever is shown fills the room: one more character would not fit.
      expect(l.need).toBeLessThanOrEqual(l.room);
      expect(l.room - l.need, 'the cut label fills the box').toBeLessThan(30);
    }
    const bar = await page.locator('.moss-nav-island-bar').boundingBox();
    expect(bar!.x + bar!.width).toBeLessThanOrEqual(390);

    // The controls keep their size and their place at the bar's end.
    const cluster = async () =>
      page.evaluate(() => {
        const bar = document.querySelector('.moss-nav-island-bar')!.getBoundingClientRect();
        const icons = document.querySelector('.moss-nav-island .nav-icons')!.getBoundingClientRect();
        const button = document.querySelector('.moss-nav-island-sections')!.getBoundingClientRect();
        return { w: icons.width, h: icons.height, fromEnd: bar.right - icons.right, gap: icons.left - button.right };
      });
    const narrow = await cluster();
    await open(page, '/', 1360, 800);
    await jumpTo(page, COLLECTIONS.indexOf(LONG_TITLE));
    const wide = await cluster();
    expect(narrow.w, 'not squeezed').toBeGreaterThanOrEqual(28);
    expect(narrow.w).toBeCloseTo(wide.w, 0);
    expect(narrow.h).toBeCloseTo(wide.h, 0);
    expect(narrow.fromEnd).toBeCloseTo(wide.fromEnd, 0);
    expect(narrow.gap, 'the section button does not run into the controls').toBeGreaterThanOrEqual(0);
  });
});
