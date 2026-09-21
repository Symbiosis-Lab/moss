// @ts-check
// The INDEX state: `.moss-cards[data-layout="grid"]` with no cover on any card.
//
// A generated term root (`/authors/`, `/tags/`) is a listing of bare labels.
// Before 2026-09-06 it rendered as `data-layout="list"` — one name per row,
// 56 names and ~2,400px of scroll on the reference vault. It now selects the
// existing grid, whose coverless state site.css styles as a roster.
//
// Only a browser can see any of this. A Rust test proves which style string
// was selected; it cannot see a track count, a name broken across two lines,
// or a cover box still occupying height. Per docs/reference/proving-a-change.md
// ("a layout that depends on glyph widths, locale, or panel width -> sweep a
// BAND of widths"), the track assertions run across a band rather than at
// three sampled widths, in both engines — glyph advance for CJK differs
// between Chromium and WebKit, and that is exactly what decides a wrap.
//
// Design: docs/archive/2026-09-06-authors-index-design-decision.md
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { tokenBlock } from './tokens-block.js';
import { mossBuildAssets } from '../../support/crate-paths';

// Plain Tab does not reach a link in macOS Safari/WebKit unless the user has
// enabled System Settings' "Keyboard navigation" — Option+Tab reaches it
// regardless of that setting, so the gate drives focus the way a Mac
// keyboard user actually would.
const TAB = process.platform === 'darwin' ? 'Alt+Tab' : 'Tab';

const CSS = fs.readFileSync(
  path.join(mossBuildAssets(), 'css/site.css'), 'utf8');

// site.css is rules-only; tokens are generated from tokens.json and prepended
// at build time. tokens-block.js builds the same :root block from the real
// contract instead of a hand-kept list — a hand list drifts (it once held
// --moss-space-2xs, which no token or rule has ever used, and --moss-space-xs/-sm
// at values neither tokens.json nor site.css's own comments agree with) and
// never gains a token this suite didn't know to ask for, elevation included.
const TOKENS = `:root{
  ${tokenBlock('light')}
  /* --moss-reading-size is normally a calc() over --moss-reading-script-scale,
     a value site.css sets contextually per :lang() rather than at :root — so
     left alone here it would be invalid outside a real page. This suite's
     pixel-band assertions need one fixed, engine-independent size instead of
     that calc chain, so it stays a literal override after the generated
     block. */
  --moss-reading-size: 18px;
}`;

// The real mixed-script shape of the reference vault: 2-4 character Han names,
// a Han name containing a full-width enumeration comma, a mixed Han/Latin
// name, a Latin name carrying a full-width parenthetical, and the longest
// Latin name in the set. The last is the one the track floor is sized around.
const NAMES = [
  '方六', '王舜薇', '烏日漢', '糜緒洋', '顧玉玲', '黃鈺晴', '茉莉', '斑戈',
  '方六、常籮', 'Yan Chen 陳研', 'Kayla（呂適之）', 'mao', 'Scarly',
  'Hongyu Jasmine Zhu',
];

// Byte-for-byte the shape grid_card.rs emits for a coverless term child: an
// empty placeholder cover div, an empty meta slot, the label. No
// `data-list-has-covers` on the wrapper — that flag is emitted iff ANY card
// has a cover, so its absence is what proves every cover box here is empty.
// A 1x1 transparent GIF stands in for a real cover image in the `covers`
// fixture. It has to be a REAL cover: `data-list-has-covers` is emitted iff a
// card carries one, and under `@supports (grid-template-rows: subgrid)` the
// empty placeholder is removed from the grid, so a fixture that set the flag
// over placeholder-only cards would be asserting markup moss never emits.
const PIXEL = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';

function indexPage(width, { covers = false, descriptions = false, theme = null, labels = NAMES } = {}) {
  const cards = labels.map((n, i) =>
    `<a href="#" class="moss-card">`
    + (covers && i === 0
        ? `<div class="moss-card-cover"><img src="${PIXEL}" alt=""></div>`
        : `<div class="moss-card-cover moss-card-no-cover"></div>`)
    + `<div class="moss-card-content">`
    + `<span class="moss-card-meta"></span>`
    + `<span class="moss-card-title">${n}</span>`
    + (descriptions ? `<p class="moss-card-description">一段說明文字。</p>` : '')
    + `</div></a>`).join('');
  return `<!doctype html><html lang="zh-hant"${theme ? ` data-theme="${theme}"` : ''}><head><meta charset="utf-8">
<style>${TOKENS}</style><style>${CSS}</style>
<style>body{margin:0;font-size:var(--moss-reading-size)}#wrap{width:${width}px}</style>
</head><body><div id="wrap">
<div class="moss-cards-container">
<div class="moss-cards" data-layout="grid" data-list-axis="date"${covers ? ' data-list-has-covers' : ''}>${cards}</div>
</div></div></body></html>`;
}

const BAND = [1100, 980, 860, 740, 700, 640, 560, 480, 420, 380, 340];

test('no name is ever broken mid-word, across the width band', async ({ page }) => {
  // `word-break: keep-all` forbids the implicit break BETWEEN Han characters,
  // which is what would split 呂適之 down the middle. A Latin name may still
  // rag onto a second line at its spaces — that is legible and truncates
  // nothing, so it is allowed. What is never allowed is a Han run splitting,
  // or any name being clipped.
  for (const width of BAND) {
    await page.setContent(indexPage(width));
    // The mechanism, asserted directly: without `keep-all` in force, a Han
    // run may break between any two characters, and no width in this band is
    // narrow enough to demonstrate that on these names. Asserting the
    // computed property is what makes this test fail if the rule is dropped.
    const wordBreak = await page.locator('.moss-cards[data-layout="grid"] .moss-card-title')
      .first().evaluate(el => getComputedStyle(el).wordBreak);
    expect(wordBreak, `word-break at ${width}px`).toBe('keep-all');

    const bad = await page.locator('.moss-cards[data-layout="grid"] .moss-card-title')
      .evaluateAll(els => els.filter(el => {
        const style = getComputedStyle(el);
        // A clipped name is always a failure — and the clip happens at the
        // CARD, whose `overflow: hidden` (site.css, grid-shape rules) is what
        // cuts an unbreakable Han run mid-glyph. The title's own box GROWS
        // past the track instead of scrolling, so `el.scrollWidth >
        // el.clientWidth` on the title cannot see it.
        const card = el.closest('.moss-card');
        if (el.getBoundingClientRect().right > card.getBoundingClientRect().right + 1) return true;
        // A Han-only label must occupy exactly one line box.
        const hanOnly = /^[㐀-鿿、。（）]+$/.test(el.textContent || '');
        if (!hanOnly) return false;
        const lh = parseFloat(style.lineHeight);
        return el.getBoundingClientRect().height > lh * 1.5;
      }).map(el => el.textContent));
    expect(bad, `names broken or clipped at ${width}px`).toEqual([]);
  }
});

test('the cover placeholder occupies no height, across the width band', async ({ page }) => {
  // The whole reason the grid could not host a listing of labels: every
  // coverless card carries `.moss-card-no-cover`, which site.css paints as a
  // 4:3 gradient box. Fourteen grey rectangles here, 56 on the real vault.
  for (const width of BAND) {
    await page.setContent(indexPage(width));
    const heights = await page.locator('.moss-cards[data-layout="grid"] .moss-card-cover')
      .evaluateAll(els => els.map(el => el.getBoundingClientRect().height));
    expect(heights.length).toBe(NAMES.length);
    expect(Math.max(...heights), `cover box has height at ${width}px`).toBe(0);
  }
});

test('track count rises monotonically with width and is never one until narrow', async ({ page }) => {
  // The point of the change: a roster, not a column. Track count must never
  // DECREASE as the container widens (an inverted breakpoint), and the
  // single-column fallback must be reached only at genuinely narrow widths.
  let previous = 0;
  for (const width of [...BAND].reverse()) {
    await page.setContent(indexPage(width));
    const cols = await page.locator('.moss-cards[data-layout="grid"]').evaluate(
      el => getComputedStyle(el).gridTemplateColumns.split(' ').length);
    expect(cols, `track count fell as width grew, at ${width}px`).toBeGreaterThanOrEqual(previous);
    previous = cols;
    if (width >= 640) {
      expect(cols, `expected a multi-column roster at ${width}px`).toBeGreaterThanOrEqual(2);
    }
  }
});

test('the index state drops the cover-bearing chrome', async ({ page }) => {
  // Chrome belongs to cover-bearing shapes. A roster of labels that kept the
  // surface fill and rounded clip would read as 56 empty tiles.
  await page.setContent(indexPage(860));
  const card = page.locator('.moss-cards[data-layout="grid"] .moss-card').first();
  const style = await card.evaluate(el => {
    const s = getComputedStyle(el);
    const content = getComputedStyle(el.querySelector('.moss-card-content'));
    return { bg: s.backgroundColor, radius: s.borderRadius, pad: content.padding };
  });
  expect(style.bg).toBe('rgba(0, 0, 0, 0)');
  expect(style.radius).toBe('0px');
  expect(style.pad).toBe('0px');
});

test('a cover anywhere restores the cover-bearing chrome (the flag is the switch)', async ({ page }) => {
  // The Rust and the CSS must agree by construction: `data-list-has-covers` is
  // emitted iff ANY card has a cover, and the index CHROME rules key on its
  // absence. One real cover among 14 labels must restore the surface fill,
  // the rounded clip and the content padding a roster drops.
  //
  // Cover HEIGHT is deliberately not asserted here — that is the media track's
  // business and it belongs to tests/render-gates/site/card-media-track.spec.js,
  // which proves it on both sides of the `@supports` gate.
  await page.setContent(indexPage(860, { covers: true }));
  const style = await page.locator('.moss-cards[data-layout="grid"] .moss-card')
    .first().evaluate(el => ({
      bg: getComputedStyle(el).backgroundColor,
      radius: getComputedStyle(el).borderRadius,
      pad: getComputedStyle(el.querySelector('.moss-card-content')).padding,
    }));
  expect(style.bg, 'index chrome survived a real cover').not.toBe('rgba(0, 0, 0, 0)');
  expect(style.radius).not.toBe('0px');
  expect(style.pad).not.toBe('0px');
});

// A `/tags/` root is the same state with longer labels — the 11em floor was
// measured against author names, and a policy tag runs two to three times
// their length. `keep-all` gives an unbreakable Han run no break opportunity
// at all, so without `overflow-wrap: anywhere` the card's `overflow: hidden`
// cuts these mid-glyph, with no ellipsis and no indication.
const LONG_TAGS = [
  '原住民族轉型正義委員會', '長期照顧與社會安全網', '氣候變遷因應法',
  '移工', '看護',
];

test('a long label wraps rather than being clipped, across the width band', async ({ page }) => {
  for (const width of BAND) {
    await page.setContent(indexPage(width, { labels: LONG_TAGS }));
    const clipped = await page.locator('.moss-cards[data-layout="grid"] .moss-card-title')
      .evaluateAll(els => els.filter(el => {
        const card = el.closest('.moss-card');
        return el.getBoundingClientRect().right > card.getBoundingClientRect().right + 1
          || el.getBoundingClientRect().bottom > card.getBoundingClientRect().bottom + 1;
      }).map(el => el.textContent));
    expect(clipped, `labels clipped at ${width}px`).toEqual([]);
  }
});

test('a label keeps a hover and a keyboard-focus affordance', async ({ page }) => {
  // The index state cancels the grid's hover lift, because chrome belongs to
  // cover-bearing shapes. Cancelling it without a replacement leaves a page
  // whose entire job is "click somewhere next" showing 56 links that respond
  // to nothing, and giving a keyboard user only the UA default ring.
  await page.setContent(indexPage(860));
  const title = page.locator('.moss-cards[data-layout="grid"] .moss-card-title').first();
  const resting = await title.evaluate(el => getComputedStyle(el).color);

  await page.locator('.moss-cards[data-layout="grid"] .moss-card').first().hover();
  expect(await title.evaluate(el => getComputedStyle(el).color),
    'hover leaves the label unchanged').not.toBe(resting);

  await page.mouse.move(0, 0);
  await page.keyboard.press(TAB);
  expect(await page.evaluate(() => document.activeElement?.className),
    'Tab did not reach the first card').toContain('moss-card');
  expect(await title.evaluate(el => getComputedStyle(el).color),
    'keyboard focus leaves the label unchanged').not.toBe(resting);
});

test('dark mode paints no drop shadow around a box that is not drawn', async ({ page }) => {
  // `[data-theme="dark"] .moss-cards[data-layout="grid"] .moss-card:hover`
  // declares the lift's shadow separately from the light-mode rule, at equal
  // specificity to a short reset and LATER in source. Cancelling only the
  // light one leaves a 4px-offset shadow hovering around a transparent card.
  await page.setContent(indexPage(860, { theme: 'dark' }));
  await page.locator('.moss-cards[data-layout="grid"] .moss-card').first().hover();
  const style = await page.locator('.moss-cards[data-layout="grid"] .moss-card')
    .first().evaluate(el => {
      const s = getComputedStyle(el);
      return { shadow: s.boxShadow, transform: s.transform };
    });
  expect(style.shadow).toBe('none');
  expect(style.transform === 'none' || style.transform === 'matrix(1, 0, 0, 1, 0, 0)').toBe(true);
});

test('a coverless grid that carries descriptions is NOT rostered', async ({ page }) => {
  // The CSS selector must be the Rust predicate. `resolve_children_config`
  // auto-selects this layout only when NO child is rich, and `has_rich` is
  // cover OR resolvable description — so an author's explicit
  // `children_style: grid` over coverless children that DO carry descriptions
  // must keep the archive shape. Matching the cover half alone would give it
  // 11em tracks and unpadded content with prose still rendering.
  await page.setContent(indexPage(860, { descriptions: true }));
  const style = await page.locator('.moss-cards[data-layout="grid"] .moss-card')
    .first().evaluate(el => ({
      bg: getComputedStyle(el).backgroundColor,
      pad: getComputedStyle(el.querySelector('.moss-card-content')).padding,
    }));
  expect(style.bg, 'index chrome leaked onto a described grid').not.toBe('rgba(0, 0, 0, 0)');
  expect(style.pad).not.toBe('0px');
});
