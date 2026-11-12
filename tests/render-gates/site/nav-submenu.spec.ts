/**
 * Render gate: the header's second level.
 *
 * The panel is positioned in script from measured geometry, so only an engine
 * can say whether it stays on screen. Checked at desktop, 390px and 321px
 * (just above the 20rem sheet), where the CJK parent sits at the row's end and
 * its panel is wider than the room to its right.
 */
import { test, expect, type Page } from '@playwright/test';

const toggle = (page: Page) => page.locator('.moss-nav-sub-toggle');
const panel = (page: Page) => page.locator('.moss-nav-sub');

async function panelEdges(page: Page) {
  return page.evaluate(() => {
    const r = document.querySelector('.moss-nav-sub')!.getBoundingClientRect();
    return { left: r.left, right: r.right, width: document.documentElement.clientWidth };
  });
}

for (const width of [1280, 390, 321]) {
  test.describe(`submenu at ${width}px`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto('/');
    });

    test('only a folder with nav children has a toggle; its children are not top-level links', async ({ page }) => {
      await expect(toggle(page)).toHaveCount(1);
      await expect(toggle(page)).toBeVisible();
      await expect(panel(page)).toBeHidden();
      await expect(page.locator('.nav-links > a[href*="tides"]')).toHaveCount(0);
      await expect(page.locator('.nav-links a[href*="harbour"]')).toHaveCount(1);
      await expect(page.locator('.nav-links > .moss-nav-group > a')).toHaveText('社區保護');
    });

    test('keyboard opens it, Escape closes it and returns focus', async ({ page }) => {
      await toggle(page).focus();
      await page.keyboard.press('Enter');
      await expect(panel(page)).toBeVisible();
      await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
      await page.keyboard.press('Escape');
      await expect(panel(page)).toBeHidden();
      await expect(toggle(page)).toBeFocused();
    });

    test('Tab walks the open panel and past its last link closes it', async ({ page, browserName }) => {
      // WebKit's default Tab order skips links.
      test.skip(browserName === 'webkit', 'Tab does not focus links in WebKit');
      await toggle(page).focus();
      await page.keyboard.press('Enter');
      await expect(panel(page)).toBeVisible();
      for (let i = 0; i < 2; i++) {
        await page.keyboard.press('Tab');
        await expect(panel(page).locator('a').nth(i)).toBeFocused();
        await expect(panel(page)).toBeVisible();
      }
      await page.keyboard.press('Tab');
      await expect(panel(page)).toBeHidden();
    });

    test('a click outside closes it', async ({ page }) => {
      await toggle(page).click();
      await expect(panel(page)).toBeVisible();
      await page.locator('main').click({ position: { x: 5, y: 5 } });
      await expect(panel(page)).toBeHidden();
    });

    test('the open panel stays inside the viewport on both edges and the page does not scroll sideways', async ({ page }) => {
      await toggle(page).click();
      await expect(panel(page)).toBeVisible();
      const { left, right, width: visible } = await panelEdges(page);
      expect(left).toBeGreaterThanOrEqual(0);
      expect(right).toBeLessThanOrEqual(visible);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(overflow).toBeLessThanOrEqual(0);
    });
  });
}

test('at 300px the sheet lists the children under their entry with no toggle', async ({ page }) => {
  await page.setViewportSize({ width: 300, height: 700 });
  await page.goto('/');
  await expect(toggle(page)).toBeHidden();
  await page.locator('.mobile-menu-button').click();
  await expect(panel(page)).toBeVisible();
  await expect(panel(page).locator('a')).toHaveCount(2);
});

/** Relative-luminance contrast of two computed `rgb()`/`rgba()` colours. */
function contrast(a: string, b: string): number {
  const lum = (css: string) => {
    const [r, g, bl] = (css.match(/[\d.]+/g) ?? []).slice(0, 3).map((v) => {
      const c = Number(v) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

test('over a header=over hero the open panel keeps readable ink on its own background', async ({ page, browserName }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/cover/');
  await toggle(page).click();
  const link = panel(page).locator('a').first();
  await expect(link).toBeVisible();
  const colours = await page.evaluate(() => {
    const ul = document.querySelector('.moss-nav-sub')!;
    const a = ul.querySelector('a')!;
    return { ink: getComputedStyle(a).color, bg: getComputedStyle(ul).backgroundColor };
  });
  expect(contrast(colours.ink, colours.bg)).toBeGreaterThanOrEqual(4.5);
  // WebKit's Tab order skips links, so no keyboard focus ring to read there.
  if (browserName === 'webkit') return;
  await link.focus();
  await page.keyboard.press('Tab');
  await page.keyboard.press('Shift+Tab');
  const ring = await page.evaluate(() => getComputedStyle(document.querySelector('.moss-nav-sub a:focus-visible')!).outlineColor);
  expect(contrast(ring, colours.bg)).toBeGreaterThanOrEqual(3);
});

test('at 390px the toggle is a 44px square that hits itself and leaves the link its own centre', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto('/');
  const result = await page.evaluate(() => {
    const t = document.querySelector('.moss-nav-sub-toggle')!;
    const link = document.querySelector('.moss-nav-group > a')!;
    const r = t.getBoundingClientRect();
    const l = link.getBoundingClientRect();
    const hits = (x: number, y: number) => document.elementFromPoint(x, y);
    const misses: string[] = [];
    for (const fx of [0.05, 0.25, 0.5, 0.75, 0.95]) {
      for (const fy of [0.05, 0.25, 0.5, 0.75, 0.95]) {
        const el = hits(r.left + r.width * fx, r.top + r.height * fy);
        if (el !== t) misses.push(`${fx},${fy}:${el?.tagName}.${el?.className}`);
      }
    }
    return {
      width: r.width,
      height: r.height,
      misses,
      linkCentre: hits(l.left + l.width / 2, l.top + l.height / 2) === link,
    };
  });
  expect(result.width).toBeGreaterThanOrEqual(44);
  expect(result.height).toBeGreaterThanOrEqual(44);
  expect(result.misses).toEqual([]);
  expect(result.linkCentre).toBe(true);
});
