// A card auto-built from a page's own description clamps to two lines with
// no ellipsis — `.moss-card-description`'s `max-block-size: calc(1.5em * 2)`
// clips overflow with a bare `overflow: hidden`, so a truncated sentence
// just stops mid-word with nothing to tell a reader it was cut.
// `display: -webkit-box` + `-webkit-line-clamp: 2` would paint the ellipsis
// for free, but that legacy multi-line-truncation box lays out along the
// PHYSICAL horizontal axis regardless of the element's own writing-mode: in
// a vertical-typesetting site it rotates the description sideways while the
// title beside it stays correctly vertical (see the long comment on the base
// rule in site.css). Neither shipping engine implements the unprefixed
// `line-clamp` property, which would not have that restriction.
//
// The ellipsis clamp is unconditional in core (site.css), and undone in
// site/vertical.css — the gated partial a horizontal site never ships —
// because core CSS must never reference `data-typesetting` at
// all (stylesheet_tests.rs's gates_change_what_ships/gates_are_independent
// enforce that for every horizontal build, and a `:not([data-typesetting=
// "vertical"])` in core would still carry the substring). So a
// horizontal-typesetting site (the overwhelming default, LTR or CJK) gets a
// visible ellipsis, and a vertical-typesetting site keeps exactly today's
// plain block clip.
//
// No binary and no scratch site: injects the branch's real site.css (and,
// for the vertical case, site/vertical.css) straight into `page.setContent`,
// the same idiom as card-cover-fit.spec.ts / footer-subscribe-alignment.spec.ts.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');
const VERTICAL_CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site/vertical.css'), 'utf8');

// Just enough of the token contract for the description's own font math
// (`max-block-size: calc(1.5em * 2)` reads `--moss-size-sm` through `1em`)
// and for the grid/list container rules around it to lay out realistically.
const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
  --moss-reading-size-base:1.125rem;
  --moss-reading-size:var(--moss-reading-size-base);
  --moss-content-width:calc(42 * var(--moss-reading-size));
  --moss-site-max-width:1200px;
  --moss-container-padding:clamp(1rem, 5vw, 2rem);
  --moss-space-2xs:4px; --moss-space-xs:6px; --moss-space-sm:8px;
  --moss-space-md:24px; --moss-space-lg:32px; --moss-space-xl:48px; --moss-space-2xl:64px;
  --moss-color-surface:#f4f1ec; --moss-color-bg:#fff; --moss-color-text:#2c2825;
  --moss-color-muted:#716d69; --moss-color-text-secondary:#5d5853;
  --moss-color-accent:#2d5a2d; --moss-color-ui-accent:var(--moss-color-accent);
  --moss-border-light:#ddd; --moss-border-medium:#bbb; --moss-border-strong:#8e8b85;
  --moss-size-xs:0.875rem; --moss-size-sm:1rem; --moss-size-md:1.125rem;
  --moss-font-heading-weight:480;
} }`;

const LONG_EN =
  'This description is written long enough on purpose that it must wrap ' +
  'across more than two lines inside a narrow card column, so the clamp ' +
  'has to cut it off somewhere in the middle of a sentence rather than at ' +
  'a tidy full stop, which is exactly the shape this gate exists to catch.';

const LONG_CJK =
  '這是一段刻意寫得很長的描述文字用來測試在窄版卡片欄位裡文字換行超過兩行之後' +
  '截斷處理是否能夠正確顯示省略號而不是把句子硬生生地從中間切斷讓讀者看不出' +
  '內容其實被截短了這正是這個測試想要抓住的問題。';

const SHORT = 'A short description.';

// Real emitted shapes, not a lookalike: grid_card.rs's `render_item` puts
// `.moss-card-cover` and `.moss-card-content` as direct children of
// `.moss-card` (grid layout's subgrid rule places them by that assumption),
// while child_summary.rs's list layout wraps kicker/meta/title/description in
// `.moss-card-body`, a sibling of `.moss-card-cover` inside `.moss-card-row`.
function card(layout: 'grid' | 'list', title: string, description: string) {
  const inner = `<span class="moss-card-meta"></span>`
    + `<span class="moss-card-title">${title}</span>`
    + `<p class="moss-card-description">${description}</p>`;
  if (layout === 'grid') {
    return `<a href="#" class="moss-card">`
      + `<div class="moss-card-cover moss-card-no-cover"></div>`
      + `<div class="moss-card-content">${inner}</div>`
      + `</a>`;
  }
  const listInner = `<h3 class="moss-card-title">${title}</h3>`
    + `<p class="moss-card-description">${description}</p>`;
  return `<a href="#" class="moss-card"><div class="moss-card-row">`
    + `<div class="moss-card-body">${listInner}</div>`
    + `<div class="moss-card-cover moss-card-no-cover"></div>`
    + `</div></a>`;
}

function page_(layout: string, cards: string[], { vertical = false, extraCss = '' } = {}) {
  return `<!doctype html><html><head><style>${TOKENS}</style><style>${CSS}</style>${vertical ? `<style>${VERTICAL_CSS}</style>` : ''}${extraCss ? `<style>${extraCss}</style>` : ''}</head>
<body${vertical ? ' data-typesetting="vertical"' : ''}>
<article class="container"><div class="moss-cards-container"><div class="moss-cards" data-layout="${layout}">
${cards.join('')}
</div></div></article>
</body></html>`;
}

// Explicit two columns for the row-shape tests below, replacing auto-fill's
// track count so the row shape does not depend on guessing the container's
// resolved width at a given viewport.
const TWO_COLUMNS = '.moss-cards[data-layout="grid"]{grid-template-columns:1fr 1fr;}';

const descBox = (page, nth: number) =>
  page.locator('.moss-card-description').nth(nth).evaluate((el) => {
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return {
      display: cs.display,
      boxOrient: cs.webkitBoxOrient,
      lineClamp: cs.webkitLineClamp,
      overflow: cs.overflow,
      height: r.height,
    };
  });

for (const layout of ['grid', 'list']) {
  test(`a ${layout} card's overflowing English description clamps with a visible ellipsis`, async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 800 });
    await page.setContent(page_(layout, [card(layout, 'Long one', LONG_EN), card(layout, 'Short one', SHORT)]));

    const long = await descBox(page, 0);
    // `-webkit-box-orient`/`-webkit-line-clamp` are what actually paint the
    // ellipsis in both engines; the computed `display` that gets them there
    // differs (WebKit reports `-webkit-box` back literally, Chromium's newer
    // multicol-based implementation reports `flow-root`), so it is not the
    // signal to assert on.
    expect(long.boxOrient).toBe('vertical');
    expect(long.lineClamp).toBe('2');
    expect(long.overflow).toBe('hidden');
    expect(long.display).not.toBe('block');
  });

  test(`a ${layout} card's overflowing CJK description clamps with a visible ellipsis`, async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 800 });
    await page.setContent(page_(layout, [card(layout, '長標題', LONG_CJK), card(layout, '短標題', SHORT)]));

    const long = await descBox(page, 0);
    expect(long.boxOrient).toBe('vertical');
    expect(long.lineClamp).toBe('2');
    expect(long.display).not.toBe('block');
  });
}

// Row-shape assertions are grid-only: list layout stacks cards in a single
// column (`.moss-cards[data-layout="list"] .moss-card { display: block }`),
// so "same row" and "same top edge" describe grid's flex/grid row, not list's
// vertical stack. An explicit two-column override replaces the auto-fill
// track count so the row shape does not depend on guessing the container's
// resolved width.
test("grid cards in a row stay equal height regardless of description length", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.setContent(
    page_('grid', [card('grid', 'Long one', LONG_EN), card('grid', 'Short one', SHORT)], { extraCss: TWO_COLUMNS }),
  );

  const cards = page.locator('.moss-card');
  const [longBox, shortBox] = await Promise.all([
    cards.nth(0).boundingBox(),
    cards.nth(1).boundingBox(),
  ]);
  expect(Math.abs((longBox?.height ?? 0) - (shortBox?.height ?? 0))).toBeLessThanOrEqual(1);
});

test("grid card titles start at the same top edge regardless of description length", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.setContent(
    page_('grid', [card('grid', 'Long one', LONG_EN), card('grid', 'Short one', SHORT)], { extraCss: TWO_COLUMNS }),
  );

  const titles = page.locator('.moss-card-title');
  const [longTitle, shortTitle] = await Promise.all([
    titles.nth(0).boundingBox(),
    titles.nth(1).boundingBox(),
  ]);
  // The description sits BELOW the title in both markup shapes (grid_card.rs,
  // child_summary.rs), so clamping it can only ever change what happens
  // beneath the title — never the title's own top edge.
  expect(Math.abs((longTitle?.y ?? 0) - (shortTitle?.y ?? 0))).toBeLessThanOrEqual(1);
});

test('the standard line-clamp property ships beside the vendor-prefixed one', () => {
  // Inert in both shipping engines today (neither implements the unprefixed
  // property), so there is nothing for a browser assertion to observe — the
  // observable fact is that the declaration exists in the shipped rule,
  // ready for whichever engine implements it first.
  const rule = CSS.match(/(?:^|\n)\.moss-card-description\s*\{[^}]*\}/s)?.[0] ?? '';
  expect(rule).toMatch(/-webkit-line-clamp:\s*2;/);
  expect(rule).toMatch(/(?<!-webkit-)line-clamp:\s*2;/);
});

test('vertical typesetting resets the standard property too, for the layout that wants no clamp', () => {
  const rule = VERTICAL_CSS.match(
    /\[data-typesetting="vertical"\] \.moss-cards\[data-layout="list"\] \.moss-card-description\s*\{[^}]*\}/,
  )?.[0] ?? '';
  expect(rule).toMatch(/(?<!-webkit-)line-clamp:\s*none;/);
});

test('a vertical-typesetting site keeps the plain block clamp, with no forced box-orient', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.setContent(page_('grid', [card('grid', '長標題', LONG_CJK)], { vertical: true }));

  const desc = await descBox(page, 0);
  // The scoped-away state: plain block display, exactly as before this fix,
  // so nothing forces the description's own writing-mode away from the
  // page's vertical-rl.
  expect(desc.display).toBe('block');
  expect(desc.overflow).toBe('hidden');
});
