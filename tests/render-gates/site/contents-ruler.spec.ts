/**
 * The contents ruler and the section name on the island's button — the
 * assertions that need a real engine, on a site the moss CLI built.
 *
 * The ruler is a column of 20px rows in the left margin, so almost every claim
 * about it is geometry: which link is under the pointer, how far a label may
 * reach, whether a truncated title still fits its box, which of two controls
 * is on screen. jsdom (nav-island's vitest files) covers the wiring; this
 * covers what only layout can answer, in chromium and webkit.
 *
 * Fixture: playwright/contents-ruler.config.ts → tests/e2e/helpers/contents-ruler-site.ts
 * Run via: bash scripts/render-gates.sh contents-ruler
 */
import { test, expect, type Page } from '@playwright/test';
import { SECTIONS, SHORT_SECTIONS, LONG_CJK_TITLE, LONG_LATIN_TITLE } from '../../e2e/helpers/contents-ruler-site';

const ARTICLE = '/essays/reading/long/';
const SHORT = '/essays/reading/short/';

/** Two frames: scroll and resize events are delivered per frame, and the ruler
 *  coalesces its own work into one more. */
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

async function open(page: Page, width: number, height: number, path = ARTICLE): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(path);
  await page.waitForSelector('.moss-contents-ruler', { state: 'attached' });
  await page.evaluate(() => document.fonts.ready);
  await frames(page);
}

async function resize(page: Page, width: number, height: number): Promise<void> {
  await page.setViewportSize({ width, height });
  await frames(page);
}

/** Park a section's heading where a fragment jump would, and let everything settle. */
async function jumpTo(page: Page, index: number): Promise<void> {
  await page.evaluate((i) => {
    // `scrollIntoView` honours the root's scroll-padding, as a fragment jump
    // does, and (unlike setting the same hash twice) always moves.
    document.querySelectorAll<HTMLElement>('main h2[id]')[i].scrollIntoView({ block: 'start' });
  }, index);
  await frames(page);
}

async function scrollTo(page: Page, y: number): Promise<void> {
  await page.evaluate((target) => window.scrollTo(0, target), y);
  await frames(page);
}

/** Mid-article, where the ruler belongs on screen. */
async function intoArticle(page: Page): Promise<void> {
  await jumpTo(page, 5);
  await expect.poll(() => isAway(page)).toBe(false);
}

const ruler = (page: Page) => page.locator('.moss-contents-ruler');

/** Whether the ruler has faded — an attribute presence, which `toHaveAttribute` cannot say. */
const isAway = (page: Page) => ruler(page).evaluate((el) => el.hasAttribute('data-away'));

/** The facts about the first heading and the ruler's rows that most claims need. */
async function geometry(page: Page) {
  return page.evaluate(() => {
    const rect = (el: Element) => el.getBoundingClientRect();
    const headings = [...document.querySelectorAll<HTMLElement>('main h2[id]')];
    const links = [...document.querySelectorAll<HTMLElement>('.moss-contents-ruler a')];
    return {
      textLeft: rect(headings[0]).left,
      ids: headings.map((h) => h.id),
      hrefs: links.map((a) => a.getAttribute('href')),
      dashes: links.map((a) => {
        const r = rect(a.querySelector('.moss-contents-ruler-dash')!);
        return { x: r.left, y: r.top + r.height / 2, left: r.left };
      }),
      rows: links.map((a) => ({ top: rect(a).top, height: rect(a).height })),
      labels: links.map((a) => {
        const r = rect(a.querySelector('.moss-contents-ruler-label')!);
        return { left: r.left, right: r.right, x: r.left + 24, y: r.top + r.height / 2 };
      }),
      list: rect(document.querySelector('.moss-contents-ruler ol')!),
      innerHeight: window.innerHeight,
    };
  });
}

/** The link under a point, as its href — what a click there would follow. */
function linkAt(page: Page, x: number, y: number): Promise<string | null> {
  return page.evaluate(
    ([px, py]) =>
      document.elementFromPoint(px, py)?.closest('.moss-contents-ruler a')?.getAttribute('href') ?? null,
    [x, y],
  );
}

for (const [width, height] of [
  [1600, 900],
  [1280, 720],
] as const) {
  test.describe(`at ${width}px`, () => {
    test.beforeEach(async ({ page }) => {
      await open(page, width, height);
      await intoArticle(page);
    });

    test('it sits 40px from the left edge, centred on the window, with no background', async ({ page }) => {
      const g = await geometry(page);
      expect(g.dashes[0].left).toBeCloseTo(40, 0);
      const middle = (g.list.top + g.list.bottom) / 2;
      expect(Math.abs(middle - g.innerHeight / 2)).toBeLessThanOrEqual(1);

      const background = () =>
        page.evaluate(() =>
          ['.moss-contents-ruler', '.moss-contents-ruler ol'].map(
            (s) => getComputedStyle(document.querySelector(s)!).backgroundColor,
          ),
        );
      expect(await background()).toEqual(['rgba(0, 0, 0, 0)', 'rgba(0, 0, 0, 0)']);
      await page.mouse.move(g.dashes[3].x + 4, g.dashes[3].y);
      expect(await background()).toEqual(['rgba(0, 0, 0, 0)', 'rgba(0, 0, 0, 0)']);
    });

    test('the link under the pointer is the section it points at, on the dash and on the label', async ({ page }) => {
      const g = await geometry(page);
      const n = g.ids.length;
      // Labels are hidden at rest, so only the dash column is hit-tested.
      const hiddenAtRest = await page.evaluate(
        () => getComputedStyle(document.querySelector('.moss-contents-ruler-label')!).visibility,
      );
      expect(hiddenAtRest).toBe('hidden');

      for (const k of [0, Math.floor(n / 2), n - 1]) {
        const want = `#${g.ids[k]}`;
        await page.mouse.move(g.dashes[k].x + 4, g.dashes[k].y);
        await frames(page, 2);
        expect(await linkAt(page, g.dashes[k].x + 4, g.dashes[k].y), `dash ${k}`).toBe(want);

        // Rows do not re-space when the labels open: the row under the pointer
        // must still be where it was.
        const open = await geometry(page);
        expect(open.rows, `rows at rest vs open, pointer on ${k}`).toEqual(g.rows);

        const label = open.labels[k];
        await page.mouse.move(label.x, label.y, { steps: 6 });
        await frames(page, 2);
        expect(await linkAt(page, label.x, label.y), `label ${k}`).toBe(want);
        await page.mouse.move(width - 10, height / 2);
        await frames(page, 2);
      }
    });

    test('labels open to the right of the dashes and stay clear of the text', async ({ page }) => {
      const g = await geometry(page);
      await page.mouse.move(g.dashes[2].x + 4, g.dashes[2].y);
      await frames(page, 2);
      const open = await geometry(page);
      for (const label of open.labels) {
        expect(label.left).toBeGreaterThan(open.dashes[0].left);
        expect(label.right, 'at least one gutter short of the text').toBeLessThanOrEqual(
          open.textLeft - 24 + 0.5,
        );
      }
    });

    test('no label is clipped, and a cut title keeps its start and its end', async ({ page }) => {
      const labels = await page.$$eval('.moss-contents-ruler-label', (els) =>
        els.map((el) => ({
          text: el.textContent ?? '',
          scrollWidth: el.scrollWidth,
          clientWidth: el.clientWidth,
        })),
      );
      expect(labels).toHaveLength(SECTIONS.length);
      labels.forEach((label, i) => {
        expect(label.scrollWidth, `label ${i} overflows its box`).toBeLessThanOrEqual(label.clientWidth);
        if (label.text === SECTIONS[i]) return;
        const parts = label.text.split('…');
        expect(parts, `label ${i} has exactly one ellipsis`).toHaveLength(2);
        expect(parts[0].length).toBeGreaterThan(0);
        expect(parts[1].length).toBeGreaterThan(0);
        expect(SECTIONS[i].startsWith(parts[0])).toBe(true);
        expect(SECTIONS[i].endsWith(parts[1])).toBe(true);
      });
      if (width === 1280) {
        expect(labels[SECTIONS.indexOf(LONG_CJK_TITLE)].text).toContain('…');
      }
    });

    test('the ruler and the island panel agree on the current section', async ({ page }) => {
      for (const k of [0, 5, SECTIONS.length - 1]) {
        await jumpTo(page, k);
        const current = await page.evaluate(() => {
          // The panel is marked when it opens; the button is hidden while the
          // ruler is up, so open it the way its click handler would.
          const button = document.querySelector<HTMLButtonElement>('.moss-nav-island-sections')!;
          if (button.getAttribute('aria-expanded') !== 'true') button.click();
          const index = (sel: string) =>
            [...document.querySelectorAll(sel)].findIndex((a) => a.getAttribute('aria-current') === 'true');
          return {
            ruler: index('.moss-contents-ruler a'),
            panel: index('[data-island-menu="sections"] a'),
          };
        });
        expect(current, `after jumping to section ${k}`).toEqual({ ruler: k, panel: k });
      }
    });

    test('the island sections button is hidden while the ruler is up and back when it fades', async ({ page }) => {
      const display = () =>
        page.evaluate(() => getComputedStyle(document.querySelector('.moss-nav-island-sections')!).display);
      expect(await display()).toBe('none');

      await scrollTo(page, 0);
      await expect.poll(() => isAway(page)).toBe(true);
      expect(await display()).not.toBe('none');

      await intoArticle(page);
      expect(await display()).toBe('none');
    });
  });
}

test.describe('when it shows', () => {
  test('hidden below the width that leaves a label 96px, and the smallest width that shows it is the one that does', async ({ page }) => {
    await open(page, 1280, 720);
    await intoArticle(page);
    const shown = () => ruler(page).evaluate((el) => !(el as HTMLElement).hidden);
    expect(await shown()).toBe(true);

    let smallest = 1280;
    for (let w = 1270; w >= 900; w -= 10) {
      await resize(page, w, 720);
      if (!(await shown())) break;
      smallest = w;
    }
    // About 1110px on a default page; anywhere near it is the claim.
    expect(smallest).toBeGreaterThan(1024);
    expect(smallest).toBeLessThanOrEqual(1200);

    // At the smallest width, the room between the label column and the text
    // is exactly what the 96px rule is about.
    await resize(page, smallest, 720);
    const g = await geometry(page);
    expect(g.textLeft - 24 - g.labels[0].left, 'the room is at least 96px').toBeGreaterThanOrEqual(96);

    await resize(page, 1024, 720);
    await expect(ruler(page)).toBeHidden();
  });

  test('faded at the top over the cover, shown mid-article, faded once past the article', async ({ page }) => {
    await open(page, 1600, 900);
    await expect.poll(() => isAway(page)).toBe(true);
    const opacity = () => ruler(page).evaluate((el) => getComputedStyle(el).opacity);
    await expect.poll(opacity).toBe('0');

    await intoArticle(page);
    await expect.poll(opacity).toBe('1');

    await scrollTo(page, await page.evaluate(() => document.documentElement.scrollHeight));
    const past = await page.evaluate(
      () => (document.querySelector('main article') ?? document.querySelector('main'))!.getBoundingClientRect().bottom < window.innerHeight * 0.5,
    );
    expect(past, 'the fixture can scroll the article above the middle').toBe(true);
    await expect.poll(() => isAway(page)).toBe(true);
  });

  test('faded is not removed: its links stay in the tab order, and focus brings it back', async ({ page }) => {
    await open(page, 1600, 900);
    // Focus the last link in the article (which scrolls to it), then scroll back
    // to the top, so the ruler is faded while the key press happens. The ruler
    // follows `main` in the document, so the key after that link is the
    // ruler's first.
    await page.locator('main a:visible').last().focus();
    await scrollTo(page, 0);
    await expect.poll(() => isAway(page)).toBe(true);
    // Safari tabs to links only with Option held (its default preference);
    // chromium tabs to them plainly.
    await page.keyboard.press(test.info().project.name === 'webkit' ? 'Alt+Tab' : 'Tab');
    // The labels fade in over 150ms, so wait for them rather than sample once.
    await expect
      .poll(() =>
        page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          const label = el.querySelector('.moss-contents-ruler-label') as HTMLElement | null;
          return !!label && getComputedStyle(label).visibility === 'visible' && getComputedStyle(label).opacity === '1';
        }),
      )
      .toBe(true);
    const focused = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      const outline = getComputedStyle(el);
      return {
        inRuler: !!el.closest('.moss-contents-ruler'),
        away: document.querySelector('.moss-contents-ruler')!.hasAttribute('data-away'),
        outlineWidth: outline.outlineWidth,
        outlineStyle: outline.outlineStyle,
      };
    });
    expect(focused.inRuler).toBe(true);
    expect(focused.away).toBe(false);
    expect(focused.outlineStyle).not.toBe('none');
    expect(focused.outlineWidth).toBe('2px');
  });

  test('under reduced motion it has no transitions', async ({ browser }) => {
    const context = await browser.newContext({ reducedMotion: 'reduce', viewport: { width: 1600, height: 900 } });
    const page = await context.newPage();
    await page.goto(ARTICLE);
    await page.waitForSelector('.moss-contents-ruler', { state: 'attached' });
    const durations = await page.evaluate(() =>
      ['.moss-contents-ruler', '.moss-contents-ruler-dash', '.moss-contents-ruler-label'].map((s) =>
        getComputedStyle(document.querySelector(s)!).transitionDuration,
      ),
    );
    expect(durations.map((d) => d.split(',').every((x) => parseFloat(x) === 0))).toEqual([true, true, true]);
    await context.close();
  });
});

test.describe('across a morph', () => {
  test('exactly one ruler remains and it lists the new page', async ({ page }) => {
    await open(page, 1600, 900);
    await intoArticle(page);
    expect(await page.locator('.moss-contents-ruler').count()).toBe(1);

    const morphTo = (path: string) =>
      page.evaluate(async (url) => {
        const html = await (await fetch(url)).text();
        const next = new DOMParser().parseFromString(html, 'text/html');
        // Only `main` and the island change, as a preview morph leaves the rest
        // of the document in place — the ruler sits outside both.
        document.querySelector('main')!.replaceWith(next.querySelector('main')!);
        document.querySelector('.moss-nav-island')!.replaceWith(next.querySelector('.moss-nav-island')!);
        window.scrollTo(0, 0);
        document.dispatchEvent(new CustomEvent('moss-morph-patched'));
      }, path);
    const titles = () => page.$$eval('.moss-contents-ruler a', (as) => as.map((a) => a.getAttribute('aria-label')));

    await morphTo(SHORT);
    await frames(page);
    expect(await page.locator('.moss-contents-ruler').count()).toBe(1);
    expect(await titles()).toEqual(SHORT_SECTIONS);

    await morphTo(ARTICLE);
    await frames(page);
    expect(await page.locator('.moss-contents-ruler').count()).toBe(1);
    expect(await titles()).toEqual(SECTIONS);
  });
});

test.describe('on a phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });

  test('the sections panel cuts long titles in the middle too, and none overflows its row', async ({ page }) => {
    await page.goto(ARTICLE);
    await page.waitForSelector('.moss-nav-island[data-shown]');
    await page.evaluate(() => document.fonts.ready);
    await jumpTo(page, 3);
    await scrollTo(page, (await page.evaluate(() => window.scrollY)) + 80);
    await scrollTo(page, (await page.evaluate(() => window.scrollY)) - 80);
    await page.waitForFunction(() => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true');
    await page.tap('.moss-nav-island-sections');
    await expect(page.locator('[data-island-menu="sections"]')).toBeVisible();
    const rows = await page.$$eval('[data-island-menu="sections"] a', (as) =>
      as.map((a) => ({ text: a.textContent ?? '', fits: a.scrollWidth <= a.clientWidth })),
    );
    rows.forEach((row, i) => expect(row.fits, `row ${i} fits`).toBe(true));
    for (const long of [LONG_CJK_TITLE, LONG_LATIN_TITLE]) {
      const text = rows[SECTIONS.indexOf(long)].text;
      const [head, tail] = text.split('…');
      expect(text, 'cut in the middle').toContain('…');
      expect(long.startsWith(head) && head.length > 0).toBe(true);
      expect(long.endsWith(tail) && tail.length > 0).toBe(true);
    }
  });

  test('the bar stays inside the screen, folds, and the button names the section in a fixed box', async ({ page }) => {
    await page.goto(ARTICLE);
    await page.waitForSelector('.moss-nav-island[data-shown]');
    await page.evaluate(() => document.fonts.ready);
    await expect(page.locator('.moss-contents-ruler')).toBeHidden();

    /** Scroll down to `y`, then up a little: the gesture that shows the island. */
    const reveal = async (y: number) => {
      await scrollTo(page, y + 80);
      await scrollTo(page, y);
      await page.waitForFunction(() => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true');
    };
    const state = () =>
      page.evaluate(() => {
        const bar = document.querySelector('.moss-nav-island-bar')!.getBoundingClientRect();
        const button = document.querySelector('.moss-nav-island-sections') as HTMLElement;
        const name = button.querySelector('.moss-nav-island-section-name') as HTMLElement;
        return {
          innerWidth: window.innerWidth,
          scrollWidth: document.documentElement.scrollWidth,
          barRight: bar.right,
          barHeight: bar.height,
          folded: !(document.querySelector('.moss-nav-island-more') as HTMLElement).hidden,
          buttonWidth: button.getBoundingClientRect().width,
          nameWidth: name.getBoundingClientRect().width,
          fontSize: parseFloat(getComputedStyle(name).fontSize),
          name: name.textContent ?? '',
          nameShown: getComputedStyle(name).display !== 'none',
        };
      });

    // Before the first section: nothing named, same box.
    const firstTop = await page.evaluate(() => document.querySelector('main h2[id]')!.getBoundingClientRect().top + window.scrollY);
    await reveal(firstTop - 400);
    const before = await state();
    expect(before.name).toBe('');

    for (const k of [1, 4]) {
      await jumpTo(page, k);
      await scrollTo(page, (await page.evaluate(() => window.scrollY)) + 80);
      await scrollTo(page, (await page.evaluate(() => window.scrollY)) - 80);
      await page.waitForFunction(() => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true');
      const s = await state();
      expect(s.innerWidth).toBe(390);
      expect(s.scrollWidth).toBeLessThanOrEqual(390);
      expect(s.barRight).toBeLessThanOrEqual(390);
      // Height, not position: the island slides in over a moment.
      expect(s.barHeight, 'the bar is no taller with a name than without').toBeCloseTo(before.barHeight, 0);
      expect(s.folded, 'the deep trail folds').toBe(true);
      expect(s.nameShown).toBe(true);
      expect(s.name).toContain('…');
      expect(s.nameWidth).toBeCloseTo(6 * s.fontSize, 0);
      expect(s.buttonWidth, 'the button is the same width with and without a name').toBeCloseTo(before.buttonWidth, 0);

      // WCAG 2.5.3: what is spoken contains what is shown (the ellipsis aside).
      const [head, tail] = s.name.split('…');
      const button = page.locator('.moss-nav-island-sections');
      await expect(button).toHaveAccessibleName(new RegExp(`^Sections on this page .*${head.trim()}.*${tail.trim()}$`.replace(/[()]/g, '\\$&')));
    }
  });
});

test.describe('the island on a window with no ruler', () => {
  test('the sections panel leads every row with a dash: darker once read, long and accented for the current one, never bold', async ({ page }) => {
    await open(page, 900, 800);
    await jumpTo(page, 5);
    await scrollTo(page, (await page.evaluate(() => window.scrollY)) + 120);
    await scrollTo(page, (await page.evaluate(() => window.scrollY)) - 120);
    await page.waitForFunction(() => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true');
    await page.click('.moss-nav-island-sections');
    await expect(page.locator('[data-island-menu="sections"]')).toBeVisible();

    const rows = await page.$$eval('[data-island-menu="sections"] a', (as) =>
      as.map((a) => {
        const dash = getComputedStyle(a, '::before');
        const label = a.getBoundingClientRect();
        return {
          current: a.getAttribute('aria-current') === 'true',
          read: a.hasAttribute('data-read'),
          width: dash.width,
          height: dash.height,
          opacity: dash.opacity,
          dashColor: dash.backgroundColor,
          weight: getComputedStyle(a).fontWeight,
          color: getComputedStyle(a).color,
          // The text starts to the right of the dash, which ends at 14 + 18.
          textStart: (() => {
            const range = document.createRange();
            range.selectNodeContents(a);
            return range.getBoundingClientRect().left - label.left;
          })(),
        };
      }),
    );
    const current = rows.findIndex((r) => r.current);
    expect(current).toBe(5);
    rows.forEach((r, i) => {
      expect(r.weight, `row ${i} is not bold`).toBe(rows[0].weight);
      expect(r.textStart, `row ${i} text clears the longest dash`).toBeGreaterThanOrEqual(32);
      expect(r.textStart, `row ${i} text lines up with the others`).toBeCloseTo(rows[0].textStart, 0);
      if (i < current) {
        expect([r.width, r.height, r.opacity], `read row ${i}`).toEqual(['10px', '1px', '0.9']);
      } else if (i > current) {
        expect([r.width, r.height, r.opacity], `unread row ${i}`).toEqual(['10px', '1px', '0.45']);
      }
    });
    expect([rows[current].width, rows[current].height, rows[current].opacity]).toEqual(['18px', '2px', '1']);
    expect(rows[current].dashColor, 'the current dash is the accent').not.toBe(rows[0].dashColor);
    expect(rows[current].color, 'the current label is the full text colour').not.toBe(rows[0].color);
  });
});

test.describe('on a window too narrow for the name', () => {
  test.use({ viewport: { width: 330, height: 800 }, isMobile: true, hasTouch: true });

  test('the button falls back to its glyph and the trail still fits', async ({ page }) => {
    await page.goto(ARTICLE);
    await page.waitForSelector('.moss-nav-island[data-shown]');
    await scrollTo(page, 3000);
    await scrollTo(page, 2900);
    await page.waitForFunction(() => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true');
    const m = await page.evaluate(() => {
      const trail = document.querySelector('.moss-nav-island-trail') as HTMLElement;
      const end = trail.getBoundingClientRect().right;
      return {
        nameShown: getComputedStyle(document.querySelector('.moss-nav-island-section-name')!).display !== 'none',
        overflow: Math.max(...[...trail.children].filter((c) => !(c as HTMLElement).hidden).map((c) => c.getBoundingClientRect().right)) - end,
        scrollWidth: document.documentElement.scrollWidth,
      };
    });
    expect(m.nameShown).toBe(false);
    expect(m.overflow).toBeLessThanOrEqual(1);
    expect(m.scrollWidth).toBeLessThanOrEqual(330);
  });
});

test.describe('on a touch screen wide enough for the ruler', () => {
  test.use({ viewport: { width: 1280, height: 800 }, isMobile: true, hasTouch: true });

  test('the first tap opens the labels, the second follows the link', async ({ page }) => {
    await page.goto(ARTICLE);
    await page.waitForSelector('.moss-contents-ruler', { state: 'attached' });
    await page.evaluate(() => document.fonts.ready);
    await frames(page);
    expect(await page.evaluate(() => matchMedia('(hover: hover)').matches), 'the profile has no hover').toBe(false);
    await jumpTo(page, 5);
    await expect.poll(() => isAway(page)).toBe(false);

    const g = await geometry(page);
    const k = 8;
    const labelVisible = () =>
      page.evaluate(
        () => getComputedStyle(document.querySelector('.moss-contents-ruler-label')!).visibility === 'visible',
      );
    expect(await labelVisible()).toBe(false);

    const before = await page.evaluate(() => location.hash);
    await page.touchscreen.tap(g.dashes[k].x + 4, g.dashes[k].y);
    await frames(page);
    expect(await labelVisible(), 'first tap opens the labels').toBe(true);
    expect(await page.evaluate(() => location.hash), 'and does not follow the link').toBe(before);

    await page.touchscreen.tap(g.dashes[k].x + 4, g.dashes[k].y);
    await frames(page);
    expect(await page.evaluate(() => decodeURIComponent(location.hash.slice(1)))).toBe(g.ids[k]);
  });
});
