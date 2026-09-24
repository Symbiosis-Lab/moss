/**
 * Floating nav island — the assertions that need a real engine.
 *
 * Everything provable from emitted text lives in Rust
 * (`build/components/nav_tests.rs`), and the fold *decision* lives in vitest
 * (`crates/moss-build/src/js-src/site/__tests__/nav-island.test.ts`). What is
 * left here is geometry and the things only a live document can answer — focus order,
 * `inert`, computed outlines, where a popover actually lands.
 *
 * The three geometric claims:
 *
 *   §5  The trail is one line at ANY width. Not "one line above 768px" — the
 *       fold is measured, so a phone is the narrowest case of one behaviour
 *       rather than a special case that drifts out of step.
 *   §5  Folding keeps the first crumb and the current page. They are the two
 *       ends of "where am I"; dropping either is what makes a folded trail
 *       useless.
 *   §7  Every label in the sections panel starts at the same x. The first
 *       attempt used an in-flow `content: "●"` for the current row, which
 *       shifted its own label sideways — the one thing a position indicator
 *       must never do. Measured with a Range around the TEXT NODE, not the
 *       row box: a marker that moves the box would be invisible to a
 *       getBoundingClientRect on the <a>.
 *
 * Harness: playwright/fixtures/nav-island/live-css.html
 * Config:  playwright/nav-island.config.ts
 * Run via: pnpm run test:render-gates nav-island
 */
import { test, expect, type Page } from '@playwright/test';

const URL = '/playwright/fixtures/nav-island/live-css.html';

// Desktop down to a narrow phone. The fold has to hold at every one of them
// with no rule that mentions any of these numbers.
const WIDTHS = [1440, 1100, 760, 420, 330];

/**
 * Scroll to `y` and do not return until the page has actually been painted
 * there — the island reads scroll direction from a running total, so a step it
 * never saw makes the NEXT step read as the opposite direction.
 *
 * Two frames, not a fixed pause. Scroll events are delivered per animation
 * frame, so two `scrollTo`s in the same frame arrive as ONE event at the final
 * position and the island correctly reads that as a single move — a reader
 * cannot scroll that way. A `waitForTimeout(100)` is a guess that this has
 * happened; waiting for two frames is the guarantee. The guess held on a
 * developer machine and lost on a loaded CI runner, where it failed the same
 * test twice in a row.
 */
async function scrollAndSettle(page: Page, y: number): Promise<void> {
  await page.evaluate(
    (target) =>
      new Promise<void>((resolve) => {
        window.scrollTo(0, target);
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
    y,
  );
}

/** Scroll down past the masthead, then back up — the gesture that reveals it. */
async function reveal(page: Page): Promise<void> {
  await scrollAndSettle(page, 1500);
  await scrollAndSettle(page, 900);
  await page.waitForFunction(
    () => document.querySelector('.moss-nav-island')?.getAttribute('data-shown') === 'true',
  );
}

/** The crumbs the trail is actually showing, in order. */
async function visibleCrumbs(page: Page): Promise<string[]> {
  return page.$$eval('.moss-nav-island-trail [data-island-crumb]', (nodes) =>
    nodes.filter((n) => !(n as HTMLElement).hidden).map((n) => (n.textContent ?? '').trim()),
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto(URL);
  await page.waitForSelector('.moss-nav-island[data-shown]');
});

for (const width of WIDTHS) {
  test.describe(`viewport ${width}px`, () => {
    test.use({ viewport: { width, height: 800 } });

    test('the trail is one line — it never wraps and never overflows', async ({ page }) => {
      await reveal(page);
      const m = await page.evaluate(() => {
        const trail = document.querySelector('.moss-nav-island-trail') as HTMLElement;
        const bar = document.querySelector('.moss-nav-island-bar') as HTMLElement;
        const rows = new Set(
          [...trail.querySelectorAll<HTMLElement>('[data-island-crumb]')]
            .filter((c) => !c.hidden)
            // Round: baseline alignment across mixed scripts moves tops by a
            // fraction of a pixel without anything having wrapped.
            .map((c) => Math.round(c.getBoundingClientRect().top)),
        );
        // Overflow is measured from the crumbs' own boxes, NOT from the
        // trail's `scrollWidth`. The `…` button carries a `data-tooltip`, and
        // site.css renders that as an absolutely-positioned `::after` wider
        // than the bar — which inflates `scrollWidth` by the width of the
        // tooltip on every folded trail, and has nothing to do with whether
        // the row fits.
        const boxes = [...trail.querySelectorAll<HTMLElement>('*')]
          .filter((el) => !el.hidden && el.offsetParent !== null)
          .map((el) => el.getBoundingClientRect());
        const trailBox = trail.getBoundingClientRect();
        return {
          rows: rows.size,
          contentRight: Math.max(...boxes.map((b) => b.right)),
          contentLeft: Math.min(...boxes.map((b) => b.left)),
          trailLeft: trailBox.left,
          trailRight: trailBox.right,
          barRight: bar.getBoundingClientRect().right,
          barHeight: bar.getBoundingClientRect().height,
        };
      });

      expect.soft(m.rows, 'every visible crumb on one row').toBe(1);
      expect.soft(m.contentRight, 'nothing runs past the trail').toBeLessThanOrEqual(
        m.trailRight + 1,
      );
      expect.soft(m.contentLeft, 'nothing runs before the trail').toBeGreaterThanOrEqual(
        m.trailLeft - 1,
      );
      expect.soft(m.trailRight, 'trail stays inside the bar').toBeLessThanOrEqual(m.barRight + 1);
      // A wrapped trail would push the bar to roughly double height. 64px is
      // comfortably above one row (44px min-height) and below two.
      expect.soft(m.barHeight, 'the bar stays one row tall').toBeLessThanOrEqual(64);
      expect(test.info().errors).toHaveLength(0);
    });

    test('folding keeps the first crumb and the current page', async ({ page }) => {
      await reveal(page);
      const shown = await visibleCrumbs(page);
      expect.soft(shown[0], 'site name survives').toBe('示例 · 報導站');
      expect.soft(shown[shown.length - 1], 'current page survives').toBe(
        '地方誌調查報導範例標題',
      );

      // Whatever left the trail is reachable behind the `…`, never lost.
      const folded = await page.$$eval(
        '.moss-nav-island-trail [data-island-crumb]',
        (nodes) =>
          nodes
            .filter((n) => (n as HTMLElement).hidden)
            .map((n) => (n.textContent ?? '').trim()),
      );
      const moreHidden = await page.$eval(
        '.moss-nav-island-more',
        (el) => (el as HTMLElement).hidden,
      );
      expect.soft(moreHidden, 'the "…" appears exactly when something folded').toBe(
        folded.length === 0,
      );
      if (folded.length > 0) {
        const menuRows = await page.$$eval('[data-island-menu="levels"] a', (rows) =>
          rows.map((r) => (r.textContent ?? '').trim()),
        );
        expect.soft(menuRows, 'the menu lists exactly what folded').toEqual(folded);
        // Nearest the reader is the LAST to go: the innermost crumb locates
        // you better than the outer one does. So a fold always starts from
        // the leftmost ancestor.
        expect.soft(folded[0], 'the leftmost ancestor folds first').toBe('分類');
      }
      expect(test.info().errors).toHaveLength(0);
    });

    test('the island aligns to the text column, not the window', async ({ page }) => {
      await reveal(page);
      const m = await page.evaluate(() => {
        const bar = document.querySelector('.moss-nav-island-bar')!.getBoundingClientRect();
        const nav = document.querySelector('.nav-content')!.getBoundingClientRect();
        return { barLeft: bar.left, barRight: bar.right, navLeft: nav.left, navRight: nav.right };
      });
      // The masthead's hairline (.nav-content) is the column edge the island
      // was designed against. 2px of slack for the bar's own 1px border.
      expect.soft(Math.abs(m.barLeft - m.navLeft), 'left edge').toBeLessThanOrEqual(2);
      expect.soft(Math.abs(m.barRight - m.navRight), 'right edge').toBeLessThanOrEqual(2);
      expect(test.info().errors).toHaveLength(0);
    });
  });
}

test.describe('behaviour at desktop width', () => {
  test.use({ viewport: { width: 1100, height: 800 } });

  test('the island stays hidden until the reader scrolls back up', async ({ page }) => {
    // At the top of the page the masthead is right there; a second copy of it
    // would be noise.
    expect(await page.getAttribute('.moss-nav-island', 'data-shown')).toBe('false');
    await page.evaluate(() => window.scrollTo(0, 1500));
    await page.waitForTimeout(100);
    expect(await page.getAttribute('.moss-nav-island', 'data-shown')).toBe('false');
    await reveal(page);
    expect(await page.getAttribute('.moss-nav-island', 'data-shown')).toBe('true');
  });

  test('every label in the sections panel starts at the same x', async ({ page }) => {
    // The regression this whole gate exists for. Measured with
    // a Range around the text node: an in-flow marker moves the TEXT while
    // leaving the row box where it was, so a box measurement would pass.
    await reveal(page);
    await page.click('.moss-nav-island-sections');
    await page.waitForSelector('[data-island-menu="sections"]:not([hidden])');

    const lefts = await page.$$eval('[data-island-menu="sections"] a', (rows) =>
      rows.map((row) => {
        const range = document.createRange();
        range.selectNodeContents(row);
        return range.getBoundingClientRect().left;
      }),
    );
    expect(lefts.length).toBeGreaterThan(1);
    const spread = Math.max(...lefts) - Math.min(...lefts);
    expect(spread, `label x spread across ${lefts.length} rows`).toBeLessThanOrEqual(0.5);

    // And it still holds once a row is marked — which is the moment an in-flow
    // marker would shift exactly one label.
    const marked = await page.$$eval('[data-island-menu="sections"] a[aria-current="true"]', (r) => r.length);
    expect(marked, 'a section is marked current').toBe(1);
  });

  test("after jumping to a section, reopening the panel marks that section", async ({
    page,
  }) => {
    // Where the scrollspy line and the landing position have to agree. A row's
    // link is an ordinary fragment link, so site.css's root scroll-padding-top
    // decides where the heading parks; `scrollspyLine()` decides which heading
    // counts as current. Measuring the line from the bar plus a smaller
    // constant put it ABOVE the landing position, so reopening the panel
    // marked the PREVIOUS section — jump to 4, panel says 3.
    //
    // The marking is only recomputed while the panel is OPEN (onScroll) or at
    // the moment it opens, and a row click closes it — so the reopen is the
    // path a reader actually sees, and the only one worth asserting.
    await reveal(page);
    await page.click(".moss-nav-island-sections");
    await page.waitForSelector('[data-island-menu="sections"]:not([hidden])');

    const rows = page.locator('[data-island-menu="sections"] a');
    const count = await rows.count();
    expect(count, "fixture needs several sections to tell an off-by-one").toBeGreaterThan(2);

    // Not the last row, whose jump can clamp at max scroll and prove nothing.
    const target = rows.nth(count - 2);
    const href = await target.getAttribute("href");
    await target.click();
    await page.waitForFunction((h) => location.hash === h, href);

    // Reopen. The island hides on the downward jump, so reveal it again —
    // but LOCALLY, with a small nudge down and back. The shared `reveal()`
    // helper scrolls to an absolute y=900, and this fixture is ~35000px tall
    // with ~7000px between headings, so it would throw the jump away and put
    // the reader back in section 1.
    const landed = await page.evaluate(() => window.scrollY);
    await scrollAndSettle(page, landed + 200);
    await scrollAndSettle(page, landed);
    await page.waitForFunction(
      () =>
        document.querySelector(".moss-nav-island")?.getAttribute("data-shown") ===
        "true",
    );
    await page.click(".moss-nav-island-sections");
    await page.waitForSelector('[data-island-menu="sections"]:not([hidden])');

    const markedHref = await page.getAttribute(
      '[data-island-menu="sections"] a[aria-current="true"]',
      "href",
    );
    expect(markedHref, `jumped to ${href}, panel marked ${markedHref}`).toBe(href);
  });

  test('the "…" opens on click, and names itself on hover without opening', async ({ page }) => {
    await page.setViewportSize({ width: 420, height: 800 });
    await reveal(page);
    const more = page.locator('.moss-nav-island-more');
    await expect(more).toBeVisible();

    // Hover names the folded levels — via data-tooltip, never `title`.
    await more.hover();
    await expect(more).toHaveAttribute('data-tooltip', /分類/);
    expect(await more.getAttribute('title')).toBeNull();
    // …and does NOT open the menu. A touch device has no hover at all, which
    // is exactly the width where folding happens.
    await expect(page.locator('[data-island-menu="levels"]')).toBeHidden();

    await more.click();
    await expect(page.locator('[data-island-menu="levels"]')).toBeVisible();
    await expect(more).toHaveAttribute('aria-expanded', 'true');
  });

  test('Escape closes a panel and hands focus back to its opener', async ({ page }) => {
    await reveal(page);
    await page.click('.moss-nav-island-sections');
    await expect(page.locator('[data-island-menu="sections"]')).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(page.locator('[data-island-menu="sections"]')).toBeHidden();
    await expect(page.locator('.moss-nav-island-sections')).toHaveAttribute(
      'aria-expanded',
      'false',
    );
    const focused = await page.evaluate(() => document.activeElement?.className ?? '');
    expect(focused).toContain('moss-nav-island-sections');
  });

  test('a click outside closes the panel', async ({ page }) => {
    await reveal(page);
    await page.click('.moss-nav-island-sections');
    await expect(page.locator('[data-island-menu="sections"]')).toBeVisible();
    await page.mouse.click(20, 700);
    await expect(page.locator('[data-island-menu="sections"]')).toBeHidden();
  });

  test('a hidden island is inert — one Tab at the top cannot reach it', async ({ page }) => {
    // The inversion of what this test asserted first. The old
    // rule was "focus reveals it, so it can never trap a keyboard reader",
    // which made the island the FIRST focusable thing on every breadcrumbed
    // page: one Tab at scroll 0 revealed an invisible bar on top of the
    // masthead it duplicates, and the next scroll hid it again with focus
    // still inside. Inert is the answer instead — there is nothing to trap.
    expect(await page.getAttribute('.moss-nav-island', 'data-shown')).toBe('false');
    expect(await page.evaluate(() => (document.querySelector('.moss-nav-island') as HTMLElement).inert)).toBe(true);

    await page.keyboard.press('Tab');
    const inIsland = await page.evaluate(
      () => !!document.activeElement?.closest('.moss-nav-island'),
    );
    expect(inIsland, 'first Tab landed inside the hidden island').toBe(false);
    expect(await page.getAttribute('.moss-nav-island', 'data-shown')).toBe('false');

    // Revealed, it is reachable again — which is what makes inert acceptable.
    await reveal(page);
    expect(await page.evaluate(() => (document.querySelector('.moss-nav-island') as HTMLElement).inert)).toBe(false);
  });

  test('a panel opens against the control that opened it, and inside the bar', async ({ page }) => {
    // The panels are SIBLINGS of the bar, so their containing block is the
    // full-width fixed wrapper. Measuring the offset from the bar instead put
    // both panels nowhere near their button at every width where the two
    // boxes differ — which, on a column-aligned island, is all of them.
    for (const width of [1100, 420, 330]) {
      await page.setViewportSize({ width, height: 800 });
      await reveal(page);
      await page.click('.moss-nav-island-sections');
      await page.waitForSelector('[data-island-menu="sections"]:not([hidden])');

      const box = await page.evaluate(() => {
        const r = (sel: string) => {
          const b = document.querySelector(sel)!.getBoundingClientRect();
          return { left: b.left, right: b.right };
        };
        return {
          btn: r('.moss-nav-island-sections'),
          pop: r('[data-island-menu="sections"]'),
          bar: r('.moss-nav-island-bar'),
        };
      });

      expect(box.pop.left, `${width}: panel starts left of its button's right edge`)
        .toBeLessThanOrEqual(box.btn.right + 0.5);
      expect(box.pop.right, `${width}: panel ends right of its button's left edge`)
        .toBeGreaterThanOrEqual(box.btn.left - 0.5);
      expect(box.pop.left, `${width}: panel stays inside the bar's left edge`)
        .toBeGreaterThanOrEqual(box.bar.left - 0.5);
      expect(box.pop.right, `${width}: panel stays inside the bar's right edge`)
        .toBeLessThanOrEqual(box.bar.right + 0.5);

      await page.keyboard.press('Escape');
    }
  });

  test('the fold is minimal — the current page truncates before an ancestor goes', async ({ page }) => {
    // The current page is the ONLY crumb allowed to truncate, so
    // its slack has to be spent before any ancestor leaves the screen. The
    // first implementation charged it full width and dropped an ancestor that
    // did not need to go.
    //
    // Asserted as a rule rather than a table of widths — but only the rule
    // that actually holds. Two tempting ones do not:
    //
    //  - "an ancestor folded, so the title must be cut" — folding frees a
    //    whole ancestor at once, so the row can come back with MORE room than
    //    the plan asked for and the title ends up whole.
    //  - "the title is cut, so an ancestor must have folded" — a row can fit
    //    with the title merely trimmed and no fold needed at all.
    //
    // What holds is the thing the old implementation got backwards: a fold
    // never happens until the title has been squeezed to its floor, so
    // whenever anything folded the title is at least that wide — for as long
    // as there is still an ancestor left to drop. Once every ancestor is gone
    // the floor stops being a promise the layout can keep: there is nothing
    // further to sacrifice, so the title takes what is left and shrinks to the
    // CSS clamp instead. That is the 330px column below.
    for (const width of WIDTHS) {
      await page.setViewportSize({ width, height: 800 });
      await reveal(page);
      const m = await page.evaluate(() => {
        const crumbs = [...document.querySelectorAll('.moss-nav-island-trail [data-island-crumb]')];
        const current = crumbs[crumbs.length - 1] as HTMLElement;
        const folded = crumbs.filter((c) => (c as HTMLElement).hidden).length;
        const em = parseFloat(getComputedStyle(current).fontSize) || 16;
        return {
          folded,
          // Everything between the site name and the current page.
          foldable: crumbs.length - 2,
          currentWidth: current.getBoundingClientRect().width,
          // 7em is FOLD_FLOOR_EM in crates/moss-build/src/js-src/site/nav/nav-island.ts. Named here so
          // that lowering it — which would bring back the three-character
          // title this test exists to prevent — turns this red rather than
          // quietly passing.
          floor: 7 * em,
        };
      });
      if (m.folded > 0 && m.folded < m.foldable) {
        expect(
          m.currentWidth,
          `${width}px: ${m.folded} of ${m.foldable} ancestors folded while the title sat under its floor — one of them did not need to go`,
        ).toBeGreaterThanOrEqual(m.floor - 1);
      }
    }
  });

  test('keyboard focus in a panel is visible, and leaving the island closes it', async ({ page }) => {
    await reveal(page);

    // Opened from the KEYBOARD, deliberately. `:focus-visible` is gated on
    // input modality, so a programmatic .focus() after a mouse click can fail
    // to match it in exactly the engines this gate exists to ask. Enter on the
    // button is the real keyboard path, and it focuses the first row itself.
    await page.focus('.moss-nav-island-sections');
    await page.keyboard.press('Enter');
    await page.waitForSelector('[data-island-menu="sections"]:not([hidden])');

    // The hover tint alone measured ~1.05:1 against the panel behind it; WCAG
    // 2.2 SC 2.4.11 wants 3:1. An outline is what carries it.
    const outline = await page.evaluate(() => {
      const row = document.activeElement as HTMLElement;
      const s = getComputedStyle(row);
      return {
        inPanel: !!row.closest('[data-island-menu="sections"]'),
        matches: row.matches(':focus-visible'),
        width: parseFloat(s.outlineWidth) || 0,
        style: s.outlineStyle,
      };
    });
    expect(outline.inPanel, 'Enter moved focus into the panel').toBe(true);
    expect(outline.matches, 'the focused row matches :focus-visible').toBe(true);
    expect(outline.style, 'focused row has an outline style').not.toBe('none');
    expect(outline.width, 'focused row outline is at least 2px').toBeGreaterThanOrEqual(2);

    // Tabbing past the last row used to leave the panel open behind the page,
    // its button still claiming aria-expanded="true".
    const rows = await page.$$eval('[data-island-menu="sections"] a', (r) => r.length);
    for (let i = 0; i < rows + 2; i++) await page.keyboard.press('Tab');
    await expect(page.locator('[data-island-menu="sections"]')).toBeHidden();
    await expect(page.locator('.moss-nav-island-sections')).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  });

  test('the panels claim no interaction model they do not implement', async ({ page }) => {
    // `role="menu"` promises arrow-key roving focus; a popover of
    // plain links does not have it, and claiming it strands a screen-reader
    // user pressing ArrowDown.
    await reveal(page);
    await page.click('.moss-nav-island-sections');
    await page.waitForSelector('[data-island-menu="sections"]:not([hidden])');
    expect(await page.$$eval('.moss-nav-island [role]', (n) => n.length)).toBe(0);
    expect(await page.$$eval('.moss-nav-island [aria-haspopup]', (n) => n.length)).toBe(0);
  });

  test('a site can turn the island off with one custom property', async ({ page }) => {
    await page.addStyleTag({ content: ':root { --moss-nav-island-display: none; }' });
    // Re-run init the way a preview rebuild does.
    await page.evaluate(() => document.dispatchEvent(new CustomEvent('moss-morph-patched')));
    const island = page.locator('.moss-nav-island');
    await expect(island).toBeHidden();
    // The attribute is removed too: the markup is left exactly as emitted.
    expect(await island.getAttribute('data-shown')).toBeNull();
  });
});

/**
 * The island appears only where the page has a contents table to show.
 *
 * Since a 2026-08-30 amendment: two or more section headings, and nothing
 * else — a deep trail no longer rescues a page that has nothing to jump to
 * within itself. The harness carries a five-crumb trail, so serving it with a
 * single section is exactly the case the old or-rule showed and the new rule
 * hides.
 *
 * This is measured here rather than in jsdom because "the bar is not on the
 * screen" is a rendered fact: the gate works by leaving `data-shown` off, and
 * what that means for the reader is decided by site.css in a real engine.
 */
test.describe('the contents-table gate', () => {
  test.use({ viewport: { width: 1100, height: 800 } });

  /**
   * Reload the harness with all but `keep` of its five sections removed.
   *
   * Rewriting the response rather than the DOM is what makes this the LOAD
   * path — the island has to be dormant from the first paint, not corrected
   * afterwards. The readiness signal is the masthead's `data-fold-managed`:
   * module scripts run in document order, and `masthead-fold.ts` is listed
   * after `nav-island.ts`, so its attribute cannot appear before the island's
   * init has already run and made its decision.
   */
  async function loadWithSections(page: Page, keep: number): Promise<void> {
    await page.route(`**${URL}`, async (route) => {
      const response = await route.fetch();
      let html = await response.text();
      for (let i = keep + 1; i <= 5; i += 1) {
        html = html.replace(new RegExp(`<h2 id="sec-${i}">[^<]*</h2>`), '');
      }
      await route.fulfill({ response, body: html });
    });
    await page.goto(URL);
    await page.waitForSelector('.nav-left[data-fold-managed]');
  }

  test('a page with one section never shows the island, however deep its trail', async ({
    page,
  }) => {
    await loadWithSections(page, 1);
    const island = page.locator('.moss-nav-island');
    expect(await island.getAttribute('data-shown')).toBeNull();

    // The reveal gesture — down past the masthead, then back up — is the one
    // thing that could still bring it in. It must not.
    await scrollAndSettle(page, 1500);
    await scrollAndSettle(page, 900);
    await expect(island).toBeHidden();
    expect(await island.getAttribute('data-shown')).toBeNull();
  });

  test('two sections are enough — that is what "a contents table" means', async ({ page }) => {
    await loadWithSections(page, 2);
    await reveal(page);
    await expect(page.locator('.moss-nav-island')).toBeVisible();
    await page.click('.moss-nav-island-sections');
    await expect(page.locator('[data-island-menu="sections"] a')).toHaveCount(2);
  });

  test('a morph onto a one-section page puts the island away', async ({ page }) => {
    // A preview rebuild swaps the document under an island that survives, so
    // the gate is re-answered rather than decided once. Leaving a revealed bar
    // up over a page that no longer earns it is the stale-island failure.
    await reveal(page);
    expect(await page.getAttribute('.moss-nav-island', 'data-shown')).toBe('true');

    await page.evaluate(() => {
      document.querySelectorAll('main h2').forEach((h, i) => {
        if (i > 0) h.remove();
      });
      document.dispatchEvent(new CustomEvent('moss-morph-patched'));
    });

    const island = page.locator('.moss-nav-island');
    await expect(island).toBeHidden();
    expect(await island.getAttribute('data-shown')).toBeNull();
  });
});

/**
 * The masthead trail and the hover hints.
 *
 * Both bars fold through `breadcrumb-fold.ts` now, so this file is where the
 * masthead's geometry is measured too — the old `nav-breadcrumb-truncate`
 * spec that used to guard it asserted the flex-shrink ellipsis ladder the fold
 * replaced, ran against markup moss no longer emits, and was wired into no
 * config, so nothing had run it in months. It was deleted rather than ported.
 */
test.describe('the masthead trail', () => {
  test('its folded-levels panel hangs off the breadcrumb row, not the whole header', async ({
    page,
  }) => {
    // The bug: site.css positions the panel with `top: calc(100% + 6px)`, and
    // 100% is the CONTAINING BLOCK — `.nav-content`. On a phone that box holds
    // the breadcrumb row AND the nav/icon row below it, so the folded levels
    // opened a row and a half below the `…` that opened them, floating over
    // the article's cover image. On desktop the two boxes coincide, which is
    // why it survived review. Hence both widths here: one where the boxes are
    // the same and one where they are not.
    // A wide window needs no fold at all, so there is no `…` to open — assert
    // that first, both because it is the correct behaviour and so the narrow
    // case below cannot pass vacuously on a trail that always folds.
    await page.setViewportSize({ width: 1440, height: 800 });
    await page.evaluate(() => window.scrollTo(0, 0));
    await expect(page.locator('.moss-breadcrumb-more')).toBeHidden();

    // 280 is where this trail actually stops fitting — measured, not guessed:
    // it fits down to 300 (`.nav-left` 252px) and folds at 280 (232px). It is
    // also comfortably inside the two-row regime, where `.nav-left` ends at
    // y=56 and `.nav-content` at y=141 — the 85px gap the bug lived in.
    for (const width of [280]) {
      await page.setViewportSize({ width, height: 800 });
      await page.evaluate(() => window.scrollTo(0, 0));
      await page.waitForFunction(
        () => !document.querySelector<HTMLElement>('.moss-breadcrumb-more')?.hidden,
      );

      await page.click('.moss-breadcrumb-more');
      await page.waitForSelector('.moss-breadcrumb-menu:not([hidden])');

      const box = await page.evaluate(() => {
        const r = (sel: string) => {
          const b = document.querySelector(sel)!.getBoundingClientRect();
          return { top: b.top, bottom: b.bottom, left: b.left, right: b.right };
        };
        return {
          row: r('.nav-left'),
          content: r('.nav-content'),
          pop: r('.moss-breadcrumb-menu'),
          btn: r('.moss-breadcrumb-more'),
        };
      });

      // 6px is PANEL_GAP in breadcrumb-fold.ts, matching site.css's resting
      // `top`. Allow a pixel of sub-pixel layout slack either side.
      expect(box.pop.top - box.row.bottom, `${width}: panel sits just under the trail row`)
        .toBeGreaterThan(4);
      expect(box.pop.top - box.row.bottom, `${width}: panel sits just under the trail row`)
        .toBeLessThan(8);
      // The assertion that actually encodes the bug: when the header is taller
      // than the trail row, the panel must NOT have been pushed to its bottom.
      if (box.content.bottom - box.row.bottom > 10) {
        expect(box.pop.top, `${width}: panel is not hung off the bottom of the whole header`)
          .toBeLessThan(box.content.bottom - 4);
      }
      // And it still opens against the control, inside the trail row.
      expect(box.pop.left).toBeLessThanOrEqual(box.btn.right + 0.5);
      expect(box.pop.left).toBeGreaterThanOrEqual(box.row.left - 0.5);

      await page.keyboard.press('Escape');
    }
  });
});

/**
 * The hover hint pill is a CSS `::after`, so it has no node to measure — its
 * box has to be reassembled from computed style. That is exactly why it broke:
 * `getComputedStyle().width` resolves to the CONTENT box whatever `box-sizing`
 * says, so `hint-place.ts` clamped a number 16px narrower than the pill really
 * was (`padding: 4px 8px`) and it cropped at the screen edge — the bug the
 * module was written to end, surviving inside its own fix.
 */
async function hintBox(page: Page, selector: string): Promise<{ left: number; right: number }> {
  return page.evaluate((sel) => {
    const host = document.querySelector<HTMLElement>(sel)!;
    const pill = getComputedStyle(host, '::after');
    const px = (v: string) => parseFloat(v) || 0;
    const width =
      px(pill.width) +
      px(pill.paddingLeft) +
      px(pill.paddingRight) +
      px(pill.borderLeftWidth) +
      px(pill.borderRightWidth);
    const left = host.getBoundingClientRect().left + px(pill.left);
    return { left, right: left + width };
  }, selector);
}

test.describe('hover hints stay on screen', () => {
  // 330 is the narrowest width the trail is asserted at above; 390 is a real
  // phone. Both are widths where the island's current crumb truncates, which
  // is the only condition under which it has a hint at all.
  for (const width of [390, 330]) {
    test(`at ${width}px, neither the current crumb's hint nor the "…"'s crops`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await reveal(page);

      let checked = 0;
      for (const sel of ['.moss-nav-island-current', '.moss-nav-island-more']) {
        const host = page.locator(sel);
        // A hint exists only where one is earned: the current crumb gets one
        // while its label is truncated (breadcrumb-hint.ts), the `…` while
        // something is folded (applyFold). Neither is guaranteed at a given
        // width, and hovering a host with no `data-tooltip` places nothing.
        if (!(await host.isVisible())) continue;
        if ((await host.getAttribute('data-tooltip')) === null) continue;
        checked += 1;
        await host.hover();
        // Placement happens on pointerover; wait for the module's own marker
        // rather than a timeout.
        await page.waitForFunction(
          (s) => document.querySelector(s)?.hasAttribute('data-hint-placed'),
          sel,
        );

        const pill = await hintBox(page, sel);
        const visible = await page.evaluate(() => document.documentElement.clientWidth);
        expect(pill.left, `${sel} @ ${width}: hint does not crop at the left edge`)
          .toBeGreaterThanOrEqual(0);
        expect(pill.right, `${sel} @ ${width}: hint does not crop at the right edge`)
          .toBeLessThanOrEqual(visible);
      }
      // Without this the test passes by measuring nothing the day the trail
      // stops folding or truncating at phone widths.
      expect(checked, `${width}: at least one hint was actually measured`).toBeGreaterThan(0);
    });
  }
});
