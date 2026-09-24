// The reading measure and the page bands under `writing-mode: vertical-rl`.
//
// `--moss-content-width` is an inline-axis limit, so site.css caps every
// consumer with `max-inline-size`. Written physically (`max-width`) the same
// token binds the BLOCK axis of a vertical page: a folder listing's 57 cards
// were clamped to 531px of block progression and overflowed leftward, THROUGH
// the footer and the colophon (found on a real vertical-writing site,
// 2026-09-05). Horizontally the two spellings are one property, so no
// horizontal gate can see the difference — only a vertical body laid out by
// an engine can.
//
// The same is true of every band this gate measures: the nav rule, the cards,
// the folder cover, the footer rule and the colophon all take their inset from
// `.container`'s inline padding, so the header rule, the listing and the
// colophon share one `y` extent under vertical-rl by construction. A physical
// `padding-top` on any of them lands on the wrong axis and only an engine
// laying out a vertical body can see it.
//
// Fixture: the emitted shape of a folder page (nav strip, article.container
// with a cover row and a list of cards, footer.container, colophon) under
// site.css + vertical.css, the same two files `emit/stylesheet.rs` concatenates
// for a `typesetting = "vertical"` site.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { tokenBlock } from './tokens-block';
import { mossBuildAssets } from '../../support/crate-paths';

const cssDir = path.join(mossBuildAssets(), 'css');
const CSS = fs.readFileSync(path.join(cssDir, 'site.css'), 'utf8');
const VERTICAL = fs.readFileSync(path.join(cssDir, 'site/vertical.css'), 'utf8');

// The real @layer tokens block, generated from tokens.json (tokens-block.ts)
// rather than hand-copied: a hand-copied list stayed lang-blind (no
// --moss-reading-script-scale term) and hid the CJK divergence between the
// fixed --moss-size-md the minimal-listing title used to pin and the
// lang-scaled --moss-reading-size its sibling .date inherits — see the
// horizontal 年表 test below, which is exactly the assertion that needed this.
const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
${tokenBlock('light')}
} }`;

// A real, decodable image: the emitted `<img>` carries the source's
// `width`/`height` presentational hints, and it is those hints — not the
// pixels — that the cover rules have to beat (issue 3).
const PORTRAIT_IMG = 'data:image/svg+xml,' + encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="2339" height="3994"><rect width="100%" height="100%" fill="#c04020"/></svg>');

const CARD_COUNT = 40;
// Cards 0 and 1 carry a kicker and a cover, card 2 a cover only, the rest
// neither: the three shapes issue 1 aligns. Card 3 is a folder card — meta
// slot, no cover. The fixture had no meta at all, which is how a folder
// card that dropped its count shipped unseen (2026-09-10).
const card = (i) => {
  const kicker = i < 2 ? '<div class="moss-card-kicker">畫冊</div>' : '';
  const meta = i === 3 ? '<div class="moss-card-meta">三十八篇</div>' : '';
  const cover = i < 3
    ? `<div class="moss-card-cover"><img src="${PORTRAIT_IMG}" width="2339" height="3994" alt=""></div>`
    : '';
  return `<a href="#${i}" class="moss-card"><div class="moss-card-row"><div class="moss-card-body">`
    + `${kicker}${meta}<h3 class="moss-card-title">畫 ${i}</h3>`
    + `<p class="moss-card-description">紙本墨筆。立軸。範例美術館藏。</p>`
    + `</div>${cover}</div></a>`;
};

const colophon = '<div class="moss-colophon"><a href="#" lang="zh-Hant">'
  + '<span class="moss-colophon-icon moss-mark"><svg viewBox="0 0 288 288"><circle cx="144" cy="144" r="120"/></svg></span>'
  + '<span class="moss-colophon-label moss-wordmark">青苔</span></a></div>';

const head = (vertical) => `<!doctype html><html lang="zh-Hant"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style>${vertical ? `<style>${VERTICAL}</style>` : ''}
</head>`;
// The three-crumb trail a real site's article pages emit (breadcrumb.rs),
// with the hidden fold controls, so the flex floors that once pushed the
// last segment off the line are under test.
const nav = '<header><nav class="main-nav container"><div class="nav-content"><div class="nav-left">'
  + '<a href="/" class="site-name" data-trail-crumb>墨石山房</a><span class="breadcrumb-separator">/</span>'
  + '<button type="button" class="moss-breadcrumb-more" hidden aria-expanded="false" aria-label="hidden">…</button><span class="breadcrumb-separator" data-trail-more-separator hidden>/</span>'
  + '<a href="/畫/" class="breadcrumb-segment" data-trail-crumb><span class="breadcrumb-label">畫</span></a><span class="breadcrumb-separator">/</span>'
  + '<a href="/畫/範例畫/" class="breadcrumb-segment" data-trail-crumb><span class="breadcrumb-label">範例畫</span></a>'
  + '</div><div class="moss-breadcrumb-menu" hidden></div>'
  + '<div class="nav-right"><div class="nav-icons"><button class="nav-theme-btn" type="button" aria-label="切換主題"><svg class="theme-toggle-icon" viewBox="0 0 32 32"><circle cx="16" cy="16" r="9"/></svg></button></div></div>'
  + '</div></nav></header>';

function folderPage({ vertical }) {
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content">
<article class="container">
<div class="moss-collection-cover-row"><div class="moss-collection-cover"><img src="${PORTRAIT_IMG}" width="2339" height="3994" alt="畫 cover"></div><div class="moss-collection-cover-body"><h1 class="moss-folder-title">畫</h1></div></div>
<div class="moss-cards-container"><div class="moss-cards" data-layout="list">
${Array.from({ length: CARD_COUNT }, (_, i) => card(i)).join('\n')}
</div></div>
<p id="p1">第一段。</p><p id="p2">第二段。</p>
</article>
<nav class="moss-series-nav"><div class="moss-series-nav-links"><a href="#">上一篇</a></div></nav>
</main>
<footer class="container"><p>版權所有</p></footer>
${colophon}
</body></html>`;
}

// The home page: prose, then the children listing (issue 5). The footer is
// empty, as the site this was found on has it, so the empty-footer collapse
// is under test too.
function homePage({ vertical }) {
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content">
<article class="container">
<p id="prose">硯中墨色猶未乾，窗外山影已西斜。</p>
<div class="moss-cards-container"><div class="moss-cards" data-layout="list">
${[0, 1, 2].map(card).join('\n')}
</div></div>
</article>
</main>
<footer class="container"></footer>
${colophon}
</body></html>`;
}

// A folder's `{.moss-embed-more}` link after its listing (child_list.rs's
// truncated-listing exit). `margin-block-start` on `.moss-embed-more` in
// site.css must read as a horizontal gap under vertical-rl, not a physical
// top offset that drops the link below the grid's head edge.
const MORE_LINK = '<p class="moss-embed-more"><a href="/archive/">目錄 →</a></p>';

function moreLinkPage({ vertical }) {
  const cards = SHELF.map(([n, c, covered]) => shelfCard(n, c, covered)).join('\n');
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content">
<article class="container">
<div class="moss-cards-container"><div class="moss-cards" data-layout="grid">${cards}</div></div>
${MORE_LINK}
</article>
</main>
<footer class="container"></footer>
${colophon}
</body></html>`;
}

// The home's shelf: the author's own `:::grid` (`.moss-grid`) and the generated
// listing grid (`.moss-cards[data-layout="grid"]`), each holding the same three
// folder cards — two with a plate, one without. grid_cells.rs resolves a
// `:::grid` wikilink cell into the SAME `<a class="moss-card">` markup a listing
// card uses, so the two must render identically; before 2026-09-11 the vertical
// rules named only the second and the first collapsed to blank colour bands.
const shelfCard = (name, count, covered) => {
  const cover = covered
    ? `<div class="moss-card-cover"><img src="${PORTRAIT_IMG}" width="2339" height="3994" alt=""></div>`
    : '<div class="moss-card-cover moss-card-no-cover"></div>';
  return `<a href="#${name}" class="moss-card"${covered ? ' data-cover-color style="--moss-cover-color: hsla(38,26%,42%,1)"' : ''}>`
    + `${cover}<div class="moss-card-content"><span class="moss-card-meta">${count}</span>`
    + `<span class="moss-card-title">${name}</span></div></a>`;
};
const SHELF = [['書', '四篇', true], ['畫', '三十八篇', true], ['文', '四篇', false]];

// A title long enough to force the band past its floor —
// longer than any real title on either demo site, deliberately, so the
// test proves the floor is a minimum rather than a clip point.
const LONG_TITLE = '一個很長很長很長很長的標題用來測試band的floor會不會裁切文字';

function shelfPage({ vertical, longTitle }) {
  const shelf = longTitle
    ? [SHELF[0], [LONG_TITLE, SHELF[1][1], SHELF[1][2]], SHELF[2]]
    : SHELF;
  const cards = shelf.map(([n, c, covered]) => shelfCard(n, c, covered)).join('\n');
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content">
<article class="container">
<p id="prose">硯中墨色猶未乾。</p>
<div class="moss-grid" data-columns="3">${cards}</div>
<div class="moss-cards-container"><div class="moss-cards" data-layout="grid" data-list-axis="date" data-list-has-covers>${cards}</div></div>
</article>
</main>
<footer class="container"></footer>
${colophon}
</body></html>`;
}

// The 年表: a year-grouped minimal listing, the shape a vertical CJK home uses
// for a chronology. `child_list::render` emits the rows; `year_group::render`
// the sections. Two years, the first with one work, the second with two, and
// one work whose date names only its year (so it carries no month prefix).
const chronRow = (month, title) =>
  `<p class="moss-card"><a href="#${title}" class="moss-prefix-link">`
  + (month ? `<span class="moss-prefix-link-prefix date">${month}</span>` : '')
  + `<span class="moss-prefix-link-title title">${title}</span></a></p>`;

const chronSection = (year, rows) =>
  `<section class="moss-cards-minimal-year-group minimal">\n<h2>${year}</h2>\n${rows.join('\n')}\n</section>`;

function chronologyPage({ vertical }) {
  const sections = [
    chronSection('一八二〇', [chronRow('三月', '範例畫作')]),
    chronSection('一八一八', [chronRow('十二月', '溪邊寫生圖'), chronRow('', '秋江圖')]),
  ].join('\n');
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content">
<article class="container">
<div class="moss-cards-container"><div class="moss-cards" data-layout="minimal">${sections}</div></div>
</article>
</main>
<footer class="container"></footer>
</body></html>`;
}

// An inline album-leaf plate, as a real vertical-writing site's albums embed
// them: a markdown `![]()` paragraph with no caption pattern, inside the
// article body — not the `:::hero {.plate}` at the top of the page. That
// site runs with `implicit_figure = false` (turned off 2026-09-10 so alt
// text stops rendering as a horizontal figcaption under vertical type), so
// the actual
// emitted shape is the bare `<p><picture><img></picture></p>` moss's
// structural-html-emission step 8 falls back to, NOT a `<figure>`. The
// owner's complaint: this picture read short of the band's full height,
// unlike the hero plate.
function platePage() {
  return `${head(true)}<body data-typesetting="vertical">
${nav}
<main id="main-content"><article class="container">
<h1 class="moss-article-title">試題冊頁範例</h1>
<p id="prose">此為排版測試用的範例題跋文字，純屬虛構，不對應任何真實文獻。</p>
<p><picture><img src="${PORTRAIT_IMG}" width="2339" height="3994" alt="試題冊頁範例"></picture></p>
</article></main>
<footer class="container"></footer>
${colophon}
</body></html>`;
}

// A body figure as the synthesizer emits it for a laddered source: the
// source's `width`/`height` hints on the `<img>`, the rungs on a `<source>`
// with the horizontal column's `sizes=`. A srcset image's intrinsic width IS
// its `sizes=` value, so the only thing that may size it is the column —
// never the fetch hint. The landscape source is wider than the column at
// every viewport here, so filling the column's height is the whole claim.
// The rung is a real 800px-wide candidate, as `photo.w800.webp` is: the
// density `800w` over `sizes=` is computed against its own pixels.
const landscape = (w, h) => 'data:image/svg+xml,' + encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="#2050c0"/></svg>`);
const LANDSCAPE_IMG = landscape(2400, 1771);
const LANDSCAPE_RUNG = landscape(800, 590);

function figurePage({ vertical }) {
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content"><article class="container">
<p id="prose">正文。</p>
<figure class="moss-image"><picture><source srcset="${LANDSCAPE_RUNG} 800w" type="image/svg+xml" sizes="(min-width: 48rem) 47.25rem, 100vw"><img src="${LANDSCAPE_IMG}" width="2400" height="1771" alt=""></picture></figure>
</article></main>
<footer class="container"></footer>
</body></html>`;
}

// A hero page, as that site's plates are: the hero is emitted outside <main>,
// so it owns no band of its own. A blockquote stands in for the inscriptions.
function heroPage() {
  return `${head(true)}<body data-typesetting="vertical">
${nav}
<div class="moss-hero"><img src="${PORTRAIT_IMG}" alt=""></div>
<main id="main-content"><article class="container"><h1 class="moss-article-title">範例畫</h1><p>正文。</p><blockquote><p>題畫。</p></blockquote></article></main>
<footer class="container"></footer>
${colophon}
</body></html>`;
}

// An ordinary article body: prose around a Markdown table, the shape a raw
// `|a|b|` block emits. §3 of the thermo review found `vertical.css` names no
// horizontal-tb exception for `table` — a vertical page with a table inherits
// `vertical-rl` on every cell, which no engine lays out sensibly.
function articlePage({ vertical }) {
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
${nav}
<main id="main-content">
<article class="container">
<h1 class="moss-article-title">範例畫題跋考</h1>
<p id="before-table">正文第一段。</p>
<table>
<thead><tr><th>年</th><th>作品</th></tr></thead>
<tbody><tr><td>一八二〇</td><td>範例畫作</td></tr><tr><td>一八一八</td><td>溪邊寫生圖</td></tr></tbody>
</table>
<p id="after-table">表後正文。</p>
<div class="moss-article-colophon"><p>款識：墨石山房。</p></div>
</article>
</main>
<footer class="container"></footer>
${colophon}
</body></html>`;
}

const rect = (page, sel) =>
  page.locator(sel).first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height,
      centerX: (r.left + r.right) / 2, centerY: (r.top + r.bottom) / 2 };
  });

// The list card's cover stretches to the body's cross extent (`align-items:
// stretch` on `.moss-card-row`) instead of a fixed size tuned for one
// script's line count; the axis that IS the cross axis flips with writing
// mode, so callers pass whichever physical field that is.
const crossExtentsMatch = (coverExtent, bodyExtent) =>
  expect(Math.abs(coverExtent - bodyExtent)).toBeLessThanOrEqual(2);

// The content box, i.e. the band an element lays its children in.
const contentBox = (page, sel) =>
  page.locator(sel).first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return {
      top: r.top + parseFloat(s.paddingTop), bottom: r.bottom - parseFloat(s.paddingBottom),
      left: r.left + parseFloat(s.paddingLeft), right: r.right - parseFloat(s.paddingRight),
    };
  });

const borders = (page, sel) =>
  page.locator(sel).first().evaluate((el) => {
    const s = getComputedStyle(el);
    return { top: s.borderTopWidth, left: s.borderLeftWidth, bottom: s.borderBottomWidth, right: s.borderRightWidth };
  });

// The footer divider and the series-nav divider are the same pattern (the
// series-nav comment in site.css says so); both must sit on the block-start edge.
const dividerBorders = (page, sel) =>
  page.evaluate((sel) => {
    const s = getComputedStyle(document.querySelector(sel), '::before');
    return { top: s.borderTopWidth, right: s.borderRightWidth };
  }, sel);

test.describe('vertical-rl folder page', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(folderPage({ vertical: true }));
  });

  test('the listing pushes footer and colophon along the block axis instead of overflowing them', async ({ page }) => {
    const cards = await rect(page, '.moss-cards');
    const lastCard = await rect(page, '.moss-card:last-of-type');
    const main = await rect(page, 'main');
    const footer = await rect(page, 'footer');
    const colophon = await rect(page, '.moss-colophon');

    // Block progression is right-to-left: the listing is many cards long and
    // its container grew with it.
    expect(cards.width).toBeGreaterThan(CARD_COUNT * 40);
    expect(main.left).toBeLessThanOrEqual(cards.left + 1);
    // The footer starts after the last card, the colophon after the footer.
    expect(footer.right).toBeLessThanOrEqual(lastCard.left);
    expect(colophon.right).toBeLessThanOrEqual(footer.left);
  });

  test('the body sits at the golden-section head margin (天 smaller than 地), on a normal and a tall viewport', async ({ page }) => {
    // Owner decision 2026-09-11: symmetric auto/auto centring read as ~400px
    // of dead head margin on a 1600px-tall display. The head margin is 0.382
    // of the free space below the body, the rest collects at the foot.
    for (const height of [900, 1600]) {
      await page.setViewportSize({ width: 1440, height });
      await page.setContent(folderPage({ vertical: true }));
      const body = await rect(page, 'body');
      expect(Math.abs(body.top - (height - body.height) * 0.382)).toBeLessThanOrEqual(2);
    }
  });

  test('type scales with viewport height: a taller viewport gets a bigger font and a taller column', async ({ page }) => {
    // --moss-vertical-font-size (clamp(1rem, 1.9svh, 1.4rem)) keeps the 38em
    // measure the same share of the page on a short and a tall screen.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(folderPage({ vertical: true }));
    const bodyShort = await rect(page, 'body');
    const fontShort = await page.$eval('body', (el) => parseFloat(getComputedStyle(el).fontSize));

    await page.setViewportSize({ width: 1440, height: 1600 });
    await page.setContent(folderPage({ vertical: true }));
    const bodyTall = await rect(page, 'body');
    const fontTall = await page.$eval('body', (el) => parseFloat(getComputedStyle(el).fontSize));

    expect(fontTall).toBeGreaterThan(fontShort);
    expect(bodyTall.height).toBeGreaterThan(bodyShort.height);
    expect(bodyTall.height).toBeLessThanOrEqual(1600 - 64 + 1);
  });

  test('a short viewport still applies the 100svh - 4rem cap, not the 38em measure', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 600 });
    await page.setContent(folderPage({ vertical: true }));
    const body = await rect(page, 'body');
    // 4rem at the default 16px root font-size.
    expect(Math.abs(body.height - (600 - 64))).toBeLessThanOrEqual(1);
    expect(body.top).toBeGreaterThanOrEqual(0);
    expect(body.bottom).toBeLessThanOrEqual(600);
  });

  test('rules sit only after the nav and before the footer; cards are separated by the gap', async ({ page }) => {
    // border-block-start under vertical-rl is the RIGHT edge — a physical
    // border-top would draw a horizontal line along the top of the strip.
    expect(await dividerBorders(page, 'footer.container')).toEqual({ top: '0px', right: '1px' });
    expect(await dividerBorders(page, '.moss-series-nav')).toEqual({ top: '0px', right: '1px' });

    // The nav rule is the block-end edge of `.nav-content` — its LEFT here.
    // `border-block-end` and `border-left` name the same edge under
    // vertical-rl; a physical `border-bottom` would underline the strip.
    expect(await borders(page, '.nav-content')).toEqual({ top: '0px', left: '1px', bottom: '0px', right: '0px' });

    // Cards carry no hairline at all (issue 2): print separates entries with
    // a blank column, and the listing's block-axis gap is that column.
    expect(await borders(page, '.moss-card:first-of-type')).toEqual({ top: '0px', left: '0px', bottom: '0px', right: '0px' });
    expect(await borders(page, '.moss-card:last-of-type')).toEqual({ top: '0px', left: '0px', bottom: '0px', right: '0px' });
    const c0 = await rect(page, '.moss-card:nth-of-type(1)');
    const c1 = await rect(page, '.moss-card:nth-of-type(2)');
    expect(Math.round(c0.left - c1.right)).toBe(32);

    // `article p + p { margin-block-start }`: the second paragraph sits 0.75
    // lines of body text to the LEFT of the first, not the old fixed space-md.
    // A physical margin-top would stack it below. 26, not 24: this fixture's
    // <html lang="zh-Hant"> makes the paragraph gap's own line (--moss-read-line)
    // 19.08px x 1.8 CJK leading, and 0.75 of that rounds to 26 — a real move
    // from the old fixed 24px, the same on this page whether it's laid out
    // vertical-rl or horizontal-tb, since neither changes --moss-reading-size.
    const p1 = await rect(page, '#p1');
    const p2 = await rect(page, '#p2');
    expect(Math.round(p1.left - p2.right)).toBe(26);
    expect(Math.abs(p1.top - p2.top)).toBeLessThanOrEqual(1);
  });

  test('the list card is the horizontal card rotated: body at the head, cover at the foot', async ({ page }) => {
    // Owner decision, 2026-09-11 (superseding the same day's title-first
    // hack, and 2026-09-10's cover-first hack before it): no reorder at
    // all. `.moss-card-row`'s flex row runs along the inline axis, which
    // vertical-rl turns vertical, so `.moss-card-body` sits at the row's
    // head and `.moss-card-cover` at its foot by construction — the same
    // rotation site.css already expresses, not a compensating rule.
    const title0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-title'); // cover + kicker
    const title3 = await rect(page, '.moss-card:nth-of-type(4) .moss-card-title'); // meta, no cover
    expect(Math.abs(title3.top - title0.top)).toBeLessThanOrEqual(1);

    // On the covered card, the cover comes after the body (at or past its
    // foot) and is itself flush with the row's own foot. Its cross extent
    // (width here — vertical-rl's block axis) stretches to match the body's
    // three-column CJK text block, which a fixed 90px used to fall 12.5px
    // short of; the shared block-start edge (the right, here) still holds.
    const body0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-body');
    const row0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-row');
    const cover0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-cover');
    expect(cover0.height).toBe(120);
    crossExtentsMatch(cover0.width, body0.width);
    expect(Math.abs(cover0.right - body0.right)).toBeLessThanOrEqual(1);
    expect(cover0.top).toBeGreaterThanOrEqual(body0.bottom - 1);
    expect(Math.abs(cover0.bottom - row0.bottom)).toBeLessThanOrEqual(1);
  });

  test('kicker/meta, title and description run as parallel columns', async ({ page }) => {
    // `.moss-card-body`'s own block flow becomes the column axis under
    // vertical-rl: kicker/meta (first in source), title, then description
    // land as three side-by-side columns instead of stacking, with the
    // kicker/meta column to the right of the title and the description
    // column to its left — date on the right, title in the middle, summary
    // on the left (owner decision, 2026-09-11).
    const title0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-title');
    const kicker0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-kicker');
    const desc0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-description');
    expect(kicker0.left).toBeGreaterThanOrEqual(title0.right - 1);
    expect(desc0.right).toBeLessThanOrEqual(title0.left + 1);

    // Same shape on a folder card, whose date sits in the meta slot; the
    // count is still written upright there.
    const title3 = await rect(page, '.moss-card:nth-of-type(4) .moss-card-title');
    const meta3 = await rect(page, '.moss-card:nth-of-type(4) .moss-card-meta');
    expect(meta3.left).toBeGreaterThanOrEqual(title3.right - 1);
    expect(await page.$eval('.moss-card:nth-of-type(4) .moss-card-meta',
      (el) => getComputedStyle(el).textOrientation)).toBe('upright');

    // The description is a column, not a line: the horizontal clamp's
    // `display: -webkit-box` is undone with the clamp, because WebKit lays a
    // legacy box's text horizontally under vertical-rl (found on a real
    // vertical-writing site, 2026-09-06). Checked on a coverless card, where
    // nothing else pushes it narrow.
    const desc3 = await rect(page, '.moss-card:nth-of-type(4) .moss-card-description');
    expect(desc3.height).toBeGreaterThan(desc3.width);
  });

  test('the folder cover is a portrait plate at the head with the title below it, both at the block start', async ({ page }) => {
    // Issue 3 (上圖下文). The `<img>` carries width="2339" height="3994";
    // under vertical-rl `inline-size` only beats the `height` attribute, so
    // without `block-size: auto` the plate renders 2339 px wide and is cropped
    // to a landscape strip.
    const band = await contentBox(page, 'main > article.container');
    const article = await rect(page, 'main > article.container');
    const plate = await rect(page, '.moss-collection-cover img');
    const title = await rect(page, '.moss-folder-title');
    expect(Math.abs(plate.top - band.top)).toBeLessThanOrEqual(1);
    expect(plate.width).toBeLessThan(300);
    expect(plate.height / plate.width).toBeCloseTo(1.5, 1);
    // The plate is at the band's block start. The row's space-lg block-start
    // margin collapses through the article (no block padding), so the article
    // itself starts where the row does; the margin shows as the distance from
    // the nav rule (header padding-block-end + main-nav margin-block-end + lg).
    expect(Math.abs(plate.right - article.right)).toBeLessThanOrEqual(1);
    const navRule = await rect(page, '.nav-content');
    expect(Math.round(navRule.left - plate.right)).toBe(16 + 16 + 32); // sm + sm + lg
    // Title below the plate after the row's 4rem gap, at the block start —
    // not centred under the plate.
    expect(Math.abs(title.top - (plate.bottom + 64))).toBeLessThanOrEqual(1);
    expect(Math.abs(title.right - plate.right)).toBeLessThanOrEqual(1);
  });

  test('the footer rule spans the same band as the nav rule and the colophon is centred on it', async ({ page }) => {
    // Issue 4. Every band takes its inline inset from `.container`, so the
    // header rule and the footer rule share one `y` extent.
    const navRule = await rect(page, '.nav-content');
    const footerRule = await contentBox(page, 'footer.container');
    expect(Math.abs(footerRule.top - navRule.top)).toBeLessThanOrEqual(1);
    expect(Math.abs(footerRule.bottom - navRule.bottom)).toBeLessThanOrEqual(1);

    const band = await contentBox(page, 'main > article.container');
    const anchor = await rect(page, '.moss-colophon a');
    expect(Math.abs(anchor.centerY - (band.top + band.bottom) / 2)).toBeLessThanOrEqual(2);
  });
});

// The wordmark's reveal. Under `reducedMotion: reduce` the rise is already
// zero at rest, so the sign of the drift is only visible without it.
test.describe('vertical-rl colophon wordmark', () => {
  test.use({ reducedMotion: 'no-preference' });

  test('青苔 is written vertically to the left of the mark, centred on it, at rest and revealed', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(homePage({ vertical: true }));
    const check = async () => {
      const icon = await rect(page, '.moss-colophon-icon');
      const label = await rect(page, '.moss-colophon-label');
      expect(label.right).toBeLessThan(icon.left);
      expect(Math.abs(label.centerY - icon.centerY)).toBeLessThan(2);
      // Glyphs stacked: the label inherits vertical-rl, so its BLOCK size —
      // its width — is the pinned 1.15 line box of the 0.75rem face. (The
      // box's height is the glyphs', which the fallback CJK font on a bare
      // box reports wrongly, so it is not what this measures.)
      expect(Math.abs(label.width - 1.15 * 12)).toBeLessThanOrEqual(1);
      await expect(page.locator('.moss-colophon-label')).toHaveCSS('writing-mode', 'vertical-rl');
    };
    await check();
    // The rest state drifts the wording AWAY from the mark; a physical
    // `translate` would drift it along the wrong axis and lose the centring.
    await page.hover('.moss-colophon a');
    await page.waitForTimeout(1000);
    await expect(page.locator('.moss-colophon-label')).toHaveCSS('opacity', '1');
    await check();
  });
});

test.describe('vertical-rl home page', () => {
  test('the listing starts at the head and breaks from the prose along the block axis', async ({ page }) => {
    // Issue 5. The section break between prose and listing is a BLOCK-axis
    // margin (leftward here); physically spelled it pushed the cards 96 px
    // down the column instead.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(homePage({ vertical: true }));
    const band = await contentBox(page, 'main > article.container');
    const container = await rect(page, '.moss-cards-container');
    const prose = await rect(page, '#prose');
    // Viewport-relative, not clientHeight: body is now centred in the
    // viewport rather than pinned to its top, so a bound on the listing has
    // to compare against body's own rect, not its height from origin 0.
    const bodyRect = await rect(page, 'body');
    expect(Math.abs(container.top - band.top)).toBeLessThanOrEqual(1);
    expect(container.bottom).toBeLessThanOrEqual(bodyRect.bottom);
    expect(prose.left - container.right).toBeGreaterThanOrEqual(64);
    const tops = await page.locator('.moss-card').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().top));
    expect(tops).toHaveLength(3);
    expect(tops[1]).toBe(tops[0]);
    expect(tops[2]).toBe(tops[0]);

    // That site's footer is empty: the collapse zeroes the BLOCK-end padding
    // so the colophon hugs the rule, not the inline inset.
    const footer = await contentBox(page, 'footer.container');
    const navRule = await rect(page, '.nav-content');
    expect(Math.abs(footer.top - navRule.top)).toBeLessThanOrEqual(1);
    expect(Math.abs(footer.bottom - navRule.bottom)).toBeLessThanOrEqual(1);
  });

  test('a hero after the header still cancels the nav rule', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(heroPage());
    expect(await borders(page, '.nav-content')).toEqual({ top: '0px', left: '0px', bottom: '0px', right: '0px' });
  });

  test('the plate stands in the text band, head-aligned and the full band tall', async ({ page }) => {
    // The hero lives outside <main>, so the band inset is granted to it in
    // vertical.css; the image then fills the inline axis — the band's height
    // — instead of stopping at a physical `max-height: 70vh`.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(heroPage());
    await page.locator('.moss-hero img').evaluate((img) => img.decode());
    const band = await contentBox(page, 'main > article.container');
    const plate = await rect(page, '.moss-hero img');
    expect(Math.abs(plate.top - band.top)).toBeLessThanOrEqual(1);
    expect(Math.abs(plate.bottom - band.bottom)).toBeLessThanOrEqual(1);
    // The title column starts on the same head line as the plate.
    const title = await rect(page, '.moss-article-title');
    expect(Math.abs(title.top - band.top)).toBeLessThanOrEqual(1);
  });

  test('the breadcrumb trail is one line: every crumb and label shares the column centre', async ({ page }) => {
    // The last segment's `min-inline-size: 5em` used to be a `min-width`,
    // i.e. a block-size floor here, which widened the segment and pushed
    // its label off the site name's line.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(heroPage());
    const site = await rect(page, '.site-name');
    for (const sel of ['.breadcrumb-segment', '.breadcrumb-segment:last-child', '.breadcrumb-segment:last-child .breadcrumb-label', '.breadcrumb-separator:not([hidden])']) {
      const r = await rect(page, sel);
      expect(Math.abs(r.centerX - site.centerX), sel).toBeLessThanOrEqual(1);
    }
  });

  test('the theme toggle ends the nav strip inside the band, on the site name\'s column', async ({ page }) => {
    // `.nav-right { flex: 0 }` zeroed the toggle's basis and it overflowed
    // the band end by its own 36px.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(heroPage());
    const band = await contentBox(page, 'main > article.container');
    const strip = await rect(page, '.nav-content');
    const btn = await rect(page, '.nav-theme-btn');
    const site = await rect(page, '.site-name');
    expect(btn.bottom).toBeLessThanOrEqual(band.bottom + 1);
    expect(btn.bottom).toBeLessThanOrEqual(strip.bottom + 1);
    expect(Math.abs(btn.centerX - site.centerX)).toBeLessThanOrEqual(2);
  });

  test('a blockquote keeps its bar on the inline start', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(heroPage());
    expect(await borders(page, 'blockquote')).toEqual({ top: '3px', left: '0px', bottom: '0px', right: '0px' });
  });
});

test.describe('vertical-rl inline plate (album leaf, not the hero)', () => {
  for (const [label, width, height] of [['desktop', 1280, 800], ['phone', 390, 844]]) {
    test(`${label}: the bare-paragraph picture fills the band's full height, whole and uncropped, like the hero plate`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      await page.setContent(platePage());
      await page.locator('p img').evaluate((img) => img.decode());
      const band = await contentBox(page, 'main > article.container');
      const plate = await rect(page, 'p img');

      // Reaches the band's own extent, the same mechanism the hero plate
      // uses — not stopped short by the physical `margin: X 0` (and the
      // wrapping <p>'s physical `margin-bottom`) that used to sit on this
      // shape and ate the INLINE axis (physical height under vertical-rl)
      // instead of the block axis the rhythm was meant for.
      expect(plate.height).toBeGreaterThan(band.bottom - band.top - 2);
      // Never cropped: the source's own aspect ratio survives (2339 × 3994).
      expect(plate.height / plate.width).toBeCloseTo(3994 / 2339, 1);
      // Never clipped by the body's overflow-y: hidden — fully inside the band.
      expect(plate.top).toBeGreaterThanOrEqual(band.top - 1);
      expect(plate.bottom).toBeLessThanOrEqual(band.bottom + 1);
    });
  }
});

test.describe('vertical-rl body figure with a srcset', () => {
  for (const [label, width, height] of [['desktop', 1440, 900], ['phone', 390, 844]]) {
    test(`${label}: the image fills the figure's inline extent, not the width its sizes= names`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      await page.setContent(figurePage({ vertical: true }));
      await page.locator('figure img').evaluate((img) => img.decode());
      const fig = await contentBox(page, 'figure.moss-image');
      const img = await rect(page, 'figure img');

      // Under vertical-rl the inline axis is the physical height. A physical
      // `height: auto` cancelled the `height=` hint there and left the width
      // at the intrinsic (= sizes=) 756px, short of the column.
      expect(img.height).toBeGreaterThan(fig.bottom - fig.top - 2);
      expect(img.width / img.height).toBeCloseTo(2400 / 1771, 1);
    });
  }

  test('horizontally the same figure fills the column width', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(figurePage({ vertical: false }));
    await page.locator('figure img').evaluate((img) => img.decode());
    const fig = await contentBox(page, 'figure.moss-image');
    const img = await rect(page, 'figure img');
    expect(Math.abs(img.width - (fig.right - fig.left))).toBeLessThanOrEqual(2);
    expect(img.width / img.height).toBeCloseTo(2400 / 1771, 1);
  });
});

test('horizontally the logical spellings change nothing', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.setContent(folderPage({ vertical: false }));

  // The column is still the measure: 42 × the reading size, CJK-scaled —
  // this fixture's <html lang="zh-Hant"> makes --moss-reading-script-scale
  // 1.06 live, same as production.
  const article = await rect(page, 'main > article.container');
  expect(article.width).toBeCloseTo(42 * 18 * 1.06, 0);

  const footer = await rect(page, 'footer');
  const lastCard = await rect(page, '.moss-card:last-of-type');
  expect(footer.top).toBeGreaterThanOrEqual(lastCard.bottom);
  expect(await dividerBorders(page, 'footer.container')).toEqual({ top: '1px', right: '0px' });
  expect(await dividerBorders(page, '.moss-series-nav')).toEqual({ top: '1px', right: '0px' });
  expect(await borders(page, '.nav-content')).toEqual({ top: '0px', left: '0px', bottom: '1px', right: '0px' });
  const site = await rect(page, '.site-name');
  const crumb = await rect(page, '.breadcrumb-segment:last-child .breadcrumb-label');
  expect(Math.abs(crumb.centerY - site.centerY)).toBeLessThanOrEqual(1);

  // 26, not 24 — see the vertical-rl variant of this fixture's own comment:
  // lang="zh-Hant" moves the paragraph gap from the old fixed space-md to
  // 0.75 lines of the CJK-leading --moss-read-line, which rounds to 26 here.
  const p1 = await rect(page, '#p1');
  const p2 = await rect(page, '#p2');
  expect(Math.round(p2.top - p1.bottom)).toBe(26);

  // The card's cover is 120px at the inline end, stretched to the body's own
  // cross extent (height) instead of a fixed 90 that used to overhang three
  // lines of Latin by 8.8px; the kicker's top is still on the cover's top
  // (the horizontal statement of issue 1).
  const body0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-body');
  const cover0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-cover');
  const kicker0 = await rect(page, '.moss-card:nth-of-type(1) .moss-card-kicker');
  expect(cover0.width).toBe(120);
  crossExtentsMatch(cover0.height, body0.height);
  expect(Math.abs(cover0.top - body0.top)).toBeLessThanOrEqual(1);
  expect(Math.abs(kicker0.top - cover0.top)).toBeLessThanOrEqual(1);

  // Book-open folder cover: plate at the inline start, 2:3 portrait.
  const plate = await rect(page, '.moss-collection-cover img');
  expect(plate.height / plate.width).toBeCloseTo(1.5, 1);
  const title = await rect(page, '.moss-folder-title');
  expect(title.left).toBeGreaterThan(plate.right + 60);

  // The colophon is centred on the article's inline axis, the wordmark below the mark.
  const band = await contentBox(page, 'main > article.container');
  const anchor = await rect(page, '.moss-colophon a');
  expect(Math.abs(anchor.centerX - (band.left + band.right) / 2)).toBeLessThanOrEqual(2);
  const icon = await rect(page, '.moss-colophon-icon');
  const label = await rect(page, '.moss-colophon-label');
  expect(label.top).toBeGreaterThan(icon.bottom - 1);
  expect(Math.abs(label.centerX - icon.centerX)).toBeLessThan(2);
});

// The shelf a `:::grid` of folder links makes, reversed by the site owner
// the same evening: the cards stack DOWN the column (a horizontal row of three,
// transposed), and each card IS its plate — the picture edge to edge, the label
// riding on the card's block-end edge instead of taking a colour band beside it.
// The geometry this replaces (one card per 13em of scroll, a plate 0.75 of the
// measure at the card's head) is gone from the CSS and from here with it.
test.describe('vertical-rl grid cards', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(shelfPage({ vertical: true }));
    await page.locator('.moss-grid .moss-card-cover img').first().evaluate((img) => img.decode());
  });

  for (const [label, root] of [['author :::grid', '.moss-grid'], ['generated listing', '.moss-cards[data-layout="grid"]']]) {
    test(`${label}: cover and band are two boxes sharing a hard edge, as horizontally`, async ({ page }) => {
      const card = await rect(page, `${root} .moss-card`);
      const cover = await rect(page, `${root} .moss-card-cover`);
      const content = await rect(page, `${root} .moss-card-content`);

      // Both boxes run the card's full cross extent (its height) — no
      // letterbox — and split its measure (its width) between them, the way
      // the horizontal card splits its height between cover-on-top and
      // band-below. Reviewed on the live site 2026-09-12: the evening-of-
      // 2026-09-11 "card is its plate" overlay put the band OVER the picture
      // instead, which is what made the boundary between them read as
      // blurred rather than cut.
      expect(Math.abs(cover.height - card.height)).toBeLessThanOrEqual(1);
      expect(Math.abs(content.height - card.height)).toBeLessThanOrEqual(1);
      expect(Math.abs(cover.width + content.width - card.width)).toBeLessThanOrEqual(1);
      // The horizontal card's 4:3 landscape, turned: inline:block stays 4:3,
      // but inline is the physical HEIGHT under vertical-rl, so width:height
      // inverts to 3:4 — a portrait plate. `aspect-ratio` is width/height
      // with no flow-relative form, which is why one declaration in
      // vertical.css states the turned literal.
      expect(cover.width / cover.height).toBeCloseTo(3 / 4, 2);

      // Cover at the block-start edge (the card's right), band at the
      // block-end edge (its left) — the same edge the horizontal band sits
      // on — and the two meet with no gap and no overlap: a hard boundary,
      // not a gradient standing in for one.
      expect(Math.abs(cover.right - card.right)).toBeLessThanOrEqual(1);
      expect(Math.abs(content.left - card.left)).toBeLessThanOrEqual(1);
      expect(Math.abs(cover.left - content.right)).toBeLessThanOrEqual(1);
      expect(await page.$eval(`${root} .moss-card-content`,
        (el) => getComputedStyle(el).backgroundImage)).toBe('none');

      // Much wider than the overlay shape's 134px at this viewport: the
      // band now costs the card real width instead of a scrim's worth.
      expect(card.width).toBeGreaterThan(180);
    });
  }

  test('a `:::grid 3` is three cards stacked down the column, sharing one x', async ({ page }) => {
    for (const height of [900, 1600]) {
      await page.setViewportSize({ width: 1440, height });
      await page.setContent(shelfPage({ vertical: true }));
      const grid = await rect(page, '.moss-grid');
      const boxes = await page.locator('.moss-grid .moss-card').evaluateAll((els) =>
        els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, width: r.width, height: r.height }; }));
      expect(boxes).toHaveLength(3);
      // `data-columns="3"` lays three tracks on the INLINE axis, which
      // vertical-rl points down the page: one column of three, not a row.
      for (const b of boxes) expect(b.left).toBeCloseTo(boxes[0].left, 0);
      expect(boxes[1].top).toBeGreaterThanOrEqual(boxes[0].bottom);
      expect(boxes[2].top).toBeGreaterThanOrEqual(boxes[1].bottom);
      // Three cards share the column, so each is a third of it or less …
      for (const b of boxes) expect(b.height).toBeLessThanOrEqual(grid.height / 3 + 1);
      expect(boxes[0].height).toBeGreaterThan(grid.height / 4);
      // … and the shelf is exactly one card wide: the scroll continues left of it.
      expect(Math.abs(grid.width - boxes[0].width)).toBeLessThanOrEqual(1);
      // The shelf starts on the prose's head line and is separated from it
      // along the BLOCK axis. `margin: <md> 0` on `.moss-grid` spent that
      // rhythm on the inline axis instead, cutting 48px off the column.
      const prose = await rect(page, '#prose');
      expect(Math.abs(boxes[0].top - prose.top)).toBeLessThanOrEqual(1);
      expect(prose.left - grid.right).toBeGreaterThanOrEqual(16);
      // Same for the generated listing's own `margin: <2xl> 0`.
      const listing = await rect(page, '.moss-cards[data-layout="grid"] .moss-card');
      expect(Math.abs(listing.top - prose.top)).toBeLessThanOrEqual(1);
    }
  });

  // There is deliberately no separate phone-width test: neither the card's
  // COMPOSITION nor its cover ratio has one under vertical-rl, so the box
  // arithmetic below (including the ratio) holds identically at 390px and at
  // 1280px. A previous version of this gate pinned a phone-specific PHYSICAL
  // stack (the card opted itself back to `writing-mode: horizontal-tb`) —
  // that was itself two bugs: it forced a phone-width breakpoint onto a
  // vertical page whose column height comes from `100svh`, not viewport
  // width, and it introduced a second card design next to the rotated one
  // above. Checking both widths here is what proves the single rule, not
  // two rules that happen to agree. The cover ratio has no narrow-container
  // twin under vertical-rl either, and for the same underlying reason: a
  // `@container (max-width: …)` condition queries physical width, which is
  // main's uncontained BLOCK axis here (`container-type: inline-size`
  // contains only the INLINE axis, physical height under vertical-rl), so
  // site.css's narrow-container override cannot match and this rule stays pinned at its single reciprocal
  // literal — see site/vertical.css's comment.
  for (const width of [390, 1280]) {
    test(`at ${width}px the shelf card keeps the rotated cover|band split — no phone-width design of its own`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.setContent(shelfPage({ vertical: true }));
      await page.locator('.moss-grid .moss-card-cover img').first().evaluate((img) => img.decode());

      const card = await rect(page, '.moss-grid .moss-card');
      const cover = await rect(page, '.moss-grid .moss-card-cover');
      const content = await rect(page, '.moss-grid .moss-card-content');
      const meta = await rect(page, '.moss-grid .moss-card-meta');
      const title = await rect(page, '.moss-grid .moss-card-title');

      // Both boxes run the card's full cross extent (height), splitting its
      // measure (width) between them — cover at the block-start (its
      // physical right), band at block-end (its physical left) — with no
      // gap or overlap between them.
      expect(Math.abs(cover.height - card.height)).toBeLessThanOrEqual(1);
      expect(Math.abs(content.height - card.height)).toBeLessThanOrEqual(1);
      expect(Math.abs(cover.width + content.width - card.width)).toBeLessThanOrEqual(1);
      expect(Math.abs(cover.right - card.right)).toBeLessThanOrEqual(1);
      expect(Math.abs(content.left - card.left)).toBeLessThanOrEqual(1);
      expect(Math.abs(cover.left - content.right)).toBeLessThanOrEqual(1);

      // inline:block = 4:3; under vertical-rl inline is the physical height,
      // so the transposed literal is 3:4 (site/vertical.css) — a portrait
      // plate, unchanged at every width (see the comment above).
      expect(cover.width / cover.height).toBeCloseTo(3 / 4, 2);

      // Neither the card nor its band ever opts back to horizontal-tb: the
      // whole card stays in the page's own writing mode.
      expect(await page.$eval('.moss-grid .moss-card', (el) => getComputedStyle(el).writingMode)).toBe('vertical-rl');
      expect(await page.$eval('.moss-grid .moss-card-content',
        (el) => getComputedStyle(el).writingMode)).toBe('vertical-rl');
      // Meta nearer the band's block-start (its physical right), title after
      // it (left) — true source order (grid_card.rs emits meta before
      // title), no `order` override.
      expect(meta.left).toBeGreaterThanOrEqual(title.right - 1);
    });
  }

  test('a longer title grows the band past its floor instead of clipping', async ({ page }) => {
    // Three cards sharing a `:::grid 3` row stretch-equalize their bands
    // (`.moss-card-content`'s `flex: 1 1 auto` under CSS Grid's default
    // `align-items: stretch`, site.css) to the row's widest need — a real
    // layout behaviour (documented in site.css), not a bug, but it means a
    // short card's own band cannot be compared against its long neighbour
    // WITHIN one render: both get stretched to the same width regardless.
    // Compare the same (first) card's band across two renders instead — one
    // shelf where every title is short, one where the second title is long —
    // so the only thing that changes between them is whether the row needed
    // to grow.
    await page.setContent(shelfPage({ vertical: true, longTitle: false }));
    await page.locator('.moss-grid .moss-card-cover img').first().evaluate((img) => img.decode());
    const shortRowContent = await rect(page, '.moss-grid .moss-card:first-of-type .moss-card-content');

    await page.setContent(shelfPage({ vertical: true, longTitle: true }));
    await page.locator('.moss-grid .moss-card-cover img').first().evaluate((img) => img.decode());
    const longRowContent = await rect(page, '.moss-grid .moss-card:first-of-type .moss-card-content');
    const longTitle = await rect(page, '.moss-grid .moss-card:nth-of-type(2) .moss-card-title');
    const longContent = await rect(page, '.moss-grid .moss-card:nth-of-type(2) .moss-card-content');

    // `min-block-size` (site.css) is logical; under vertical-rl it floors the
    // band's physical WIDTH, not its height — the floor holds when every
    // title in the row is short (the band doesn't collapse to zero) …
    expect(shortRowContent.width).toBeGreaterThan(40);
    // … and one long title grows every band in its row well past that floor
    // rather than being clipped by the card's own `overflow: hidden`.
    expect(longRowContent.width).toBeGreaterThan(shortRowContent.width);
    expect(Math.abs(longContent.width - longRowContent.width)).toBeLessThanOrEqual(1);
    expect(longTitle.left).toBeGreaterThanOrEqual(longContent.left - 1);
    expect(longTitle.right).toBeLessThanOrEqual(longContent.right + 1);
  });

  test('a coverless card is the same band, turned — no rule of its own', async ({ page }) => {
    // Horizontally a coverless `:::grid` card shows its colour band with the
    // title on it; the transpose is that band turned. The vertical `display:
    // none` that used to collapse it (2026-09-11 morning) is gone: the card
    // keeps its siblings' box, so the shelf stays a shelf.
    expect(await page.$eval('.moss-grid .moss-card-no-cover', (el) => getComputedStyle(el).display)).not.toBe('none');
    const covered = await rect(page, '.moss-grid .moss-card');
    const coveredPlate = await rect(page, '.moss-grid .moss-card-cover:not(.moss-card-no-cover)');
    const bare = await rect(page, '.moss-grid .moss-card:last-of-type');
    const band = await rect(page, '.moss-grid .moss-card-no-cover');
    // The coverless card is still the same overall box as its covered
    // siblings — the placeholder occupies the plate's own slot, sized like a
    // real cover, with the count+title band alongside it exactly as it sits
    // on a covered card.
    expect(Math.abs(bare.width - covered.width)).toBeLessThanOrEqual(1);
    expect(Math.abs(band.width - coveredPlate.width)).toBeLessThanOrEqual(1);
    expect(Math.abs(band.height - coveredPlate.height)).toBeLessThanOrEqual(1);
  });

  test('the count sits beside the title, on the right, sharing its head line — as horizontally, turned', async ({ page }) => {
    const title = await rect(page, '.moss-grid .moss-card-title');
    const meta = await rect(page, '.moss-grid .moss-card-meta');
    const content = await rect(page, '.moss-grid .moss-card-content');
    // Horizontally meta sits above the title, sharing its left edge — one
    // physical line of overline-then-headline reading down the page. Turned,
    // "above" becomes "toward the block-start" (the card's right, closer to
    // the plate) and the shared edge becomes a shared TOP: the two run as
    // parallel columns starting on the same line, meta the nearer column.
    expect(Math.abs(meta.top - title.top)).toBeLessThanOrEqual(2);
    expect(meta.left).toBeGreaterThanOrEqual(title.right - 1);
    expect(await page.$eval('.moss-grid .moss-card-meta', (el) => getComputedStyle(el).textOrientation)).toBe('upright');
    // Both columns fit inside the widened band with room to spare — the
    // count is not clipped against the band's own edges.
    expect(meta.right).toBeLessThanOrEqual(content.right + 1);
    expect(title.left).toBeGreaterThanOrEqual(content.left - 1);
  });
});

for (const width of [1440, 390]) {
  // 1440px is well above both the 768px mobile-grid-collapse breakpoint and
  // the 36rem (576px) container-query breakpoint; 390px is below both.
  // site.css collapses `.moss-grid[data-columns]` to 1 column below 768px
  // for horizontal-tb (restored by 2381269 after a9db42542 briefly dropped
  // it for every writing mode — see that commit's message and
  // preview-cards.md's "Vertical typesetting" section, which scopes the
  // "no phone breakpoint" rule to vertical-rl only). Only vertical.css
  // counters the collapse back to N columns, and only under
  // `[data-typesetting="vertical"]`; this test renders `vertical: false`,
  // so nothing counters it here.
  const collapsed = width < 768;
  const expectedRatio = width < 576 ? 3 / 4 : 4 / 3;
  const shape = width < 576 ? 'portrait' : 'landscape';
  const layout = collapsed ? 'a single column' : 'a 3-track grid';
  test(`horizontally at ${width}px the shelf is ${layout} of ${shape} plates`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent(shelfPage({ vertical: false }));
    await page.locator('.moss-grid .moss-card-cover img').first().evaluate((img) => img.decode());
    const boxes = await page.locator('.moss-grid .moss-card').evaluateAll((els) =>
      els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, left: r.left }; }));
    if (collapsed) {
      // Below 768px the row becomes a single column: cards share a left
      // edge and stack down the page instead of running abreast.
      expect(Math.abs(boxes[1].left - boxes[0].left)).toBeLessThanOrEqual(1);
      expect(boxes[1].top).toBeGreaterThan(boxes[0].top);
    } else {
      // Three abreast on one row, left to right.
      expect(boxes[1].left).toBeGreaterThan(boxes[0].left);
      expect(boxes[1].top).toBeCloseTo(boxes[0].top, 0);
    }
    const card = await rect(page, '.moss-grid .moss-card');
    const cover = await rect(page, '.moss-grid .moss-card-cover');
    const content = await rect(page, '.moss-grid .moss-card-content');
    expect(cover.width / cover.height).toBeCloseTo(expectedRatio, 1);
    // And the band is still IN FLOW below the poster, not over it: the overlay
    // belongs to the turned card only.
    expect(content.top).toBeGreaterThanOrEqual(cover.bottom - 1);
    expect(card.height).toBeGreaterThan(cover.height + 1);
  });
}

// Both `chromium` and `webkit` projects (playwright/vertical-measure.config.ts)
// run this file, so this one test already covers both engines.
test.describe('vertical-rl "more" link after a listing', () => {
  test("keeps the column's top edge and sits left of the grid with the gap", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(moreLinkPage({ vertical: true }));
    await page.locator('.moss-cards[data-layout="grid"] .moss-card-cover img').first().evaluate((img) => img.decode());

    const grid = await rect(page, '.moss-cards[data-layout="grid"]');
    const more = await rect(page, '.moss-embed-more');

    // Same block-start edge as the listing — a physical `margin-top`
    // instead pushed the link DOWN the column, off the grid's head line.
    expect(Math.abs(more.top - grid.top)).toBeLessThanOrEqual(2);
    // And it clears the listing along the block axis (leftward here). The
    // grid carries its own block-end rhythm (--moss-space-2xl, 64px,
    // `margin-block` on `.moss-cards[data-layout="grid"]`) and the link adds
    // its own block-start gap (--moss-space-xl, 48px) on top — the two don't
    // collapse, in EITHER writing mode (verified against the physical,
    // horizontal-tb rendering of this same markup: 112px there too). A
    // physical `margin-top` on `.moss-embed-more` put this 112px on the
    // wrong axis instead of losing it.
    expect(grid.left - more.right).toBeGreaterThanOrEqual(112 - 2);
    expect(grid.left - more.right).toBeLessThanOrEqual(112 + 2);
  });
});

test.describe('vertical-rl 年表', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(chronologyPage({ vertical: true }));
  });

  test('a year heading starts on its rows\' head line and leads them down the scroll', async ({ page }) => {
    // `margin-top`/`margin-bottom` on the heading are BLOCK-axis jobs — the gap
    // from the previous year, and the gap to its own rows. Spelled physically
    // they landed on the inline axis instead, hanging the heading 24px down the
    // column while its rows started at the head line.
    const heads = await page.locator('.moss-cards-minimal-year-group h2').evaluateAll((els) =>
      els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, right: r.right, left: r.left }; }));
    const rows = await page.locator('.moss-cards-minimal-year-group .moss-card').evaluateAll((els) =>
      els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, right: r.right, left: r.left }; }));
    expect(heads).toHaveLength(2);
    for (const h of heads) expect(Math.abs(h.top - rows[0].top)).toBeLessThanOrEqual(1);
    // Heading first, then its rows, leftward.
    expect(rows[0].right).toBeLessThanOrEqual(heads[0].left + 1);
    expect(rows[1].right).toBeLessThanOrEqual(heads[1].left + 1);
  });

  test('successive years are separated along the scroll', async ({ page }) => {
    // The section's own `margin-bottom` is that separation; physically spelled
    // it fell on the inline axis and the two years touched, which is how 21
    // year groups read as one undivided run.
    const boxes = await page.locator('.moss-cards-minimal-year-group').evaluateAll((els) =>
      els.map((e) => { const r = e.getBoundingClientRect(); return { right: r.right, left: r.left }; }));
    expect(boxes).toHaveLength(2);
    expect(boxes[0].left - boxes[1].right).toBeGreaterThanOrEqual(16);
  });

  test('a row with no month prefix still starts where a prefixed row does', async ({ page }) => {
    const rows = await page.locator('.moss-cards-minimal-year-group .moss-card').evaluateAll((els) =>
      els.map((e) => e.getBoundingClientRect().top));
    expect(rows[1]).toBeCloseTo(rows[2], 0);
  });

  // mulu.md CSS edits 1/3/4/5: the row-to-row gap and row padding scale with
  // the vertical clamp font (em) instead of a fixed rem, and the title
  // carries the same heading weight as every other card title, distinguishing
  // it from the unweighted date/month prefix beside it.
  test('rows space themselves in ems off their own font size, and the title outweighs the date', async ({ page }) => {
    const group = page.locator('.moss-cards-minimal-year-group').first();
    const [gap, fontSize] = await group.evaluate((e) => {
      const s = getComputedStyle(e);
      return [parseFloat(s.gap), parseFloat(s.fontSize)];
    });
    expect(gap).toBeCloseTo(fontSize, 0);

    const title = page.locator('.moss-cards-minimal-year-group .moss-card .title').first();
    const [titleWeight, dateWeight] = await Promise.all([
      title.evaluate((e) => getComputedStyle(e).fontWeight),
      page.locator('.moss-cards-minimal-year-group .moss-card .date').first()
        .evaluate((e) => getComputedStyle(e).fontWeight),
    ]);
    expect(Number(titleWeight)).toBe(480);
    expect(Number(titleWeight)).toBeGreaterThan(Number(dateWeight));
  });
});

test('horizontally the 年表 is unchanged: headings above their rows, years stacked', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.setContent(chronologyPage({ vertical: false }));
  const heads = await page.locator('.moss-cards-minimal-year-group h2').evaluateAll((els) =>
    els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left }; }));
  const sections = await page.locator('.moss-cards-minimal-year-group').evaluateAll((els) =>
    els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, bottom: r.bottom }; }));
  const rows = await page.locator('.moss-cards-minimal-year-group .moss-card').evaluateAll((els) =>
    els.map((e) => { const r = e.getBoundingClientRect(); return { top: r.top, left: r.left }; }));
  expect(rows[0].top).toBeGreaterThanOrEqual(heads[0].bottom - 1);
  expect(heads[1].top).toBeGreaterThan(heads[0].top);
  expect(sections[1].top - sections[0].bottom).toBeGreaterThanOrEqual(16);
  // Every row's text starts on the same left edge, prefix or none.
  expect(rows[1].left).toBeCloseTo(rows[2].left, 0);

  // A simple horizontal title/date listing carries no weight or size
  // distinction of its own — both inherit from body. Sibling-equality,
  // not a hardcoded pixel/weight pair: it holds regardless of what the
  // lang-driven --moss-reading-script-scale factor is, and this fixture's
  // `lang="zh-Hant"` <html> makes it live (1.06), which is exactly the case
  // a fixed --moss-size-md on .title used to diverge from .date's own
  // lang-scaled --moss-reading-size.
  const title = page.locator('.moss-cards-minimal-year-group .moss-card .title').first();
  const date = page.locator('.moss-cards-minimal-year-group .moss-card .date').first();
  const [titleStyle, dateStyle] = await Promise.all([
    title.evaluate((e) => { const s = getComputedStyle(e); return { weight: s.fontWeight, size: s.fontSize }; }),
    date.evaluate((e) => { const s = getComputedStyle(e); return { weight: s.fontWeight, size: s.fontSize }; }),
  ]);
  expect(titleStyle.weight).toBe(dateStyle.weight);
  expect(titleStyle.size).toBe(dateStyle.size);
});

// `vertical.css` exempted `figcaption`/`pre`/`pre code` back to horizontal-tb
// but never `table`, so a Markdown table on a vertical page inherited
// `vertical-rl` on every cell — no engine lays that out sensibly.
test.describe('vertical-rl article table', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(articlePage({ vertical: true }));
  });

  test('a table inside a vertical page stays horizontal-tb: wider than tall, rows run left to right', async ({ page }) => {
    const writingMode = await page.locator('table').evaluate((el) => getComputedStyle(el).writingMode);
    expect(writingMode).toBe('horizontal-tb');

    const table = await rect(page, 'table');
    expect(table.width).toBeGreaterThan(table.height);

    // Two header cells in the same row sit side by side: same y, second to
    // the right of the first.
    const th1 = await rect(page, 'thead th:nth-child(1)');
    const th2 = await rect(page, 'thead th:nth-child(2)');
    expect(Math.abs(th1.centerY - th2.centerY)).toBeLessThanOrEqual(1);
    expect(th2.left).toBeGreaterThanOrEqual(th1.right - 1);

    // Two first-column cells in successive rows sit one below the other:
    // same x, second below the first.
    const row1td1 = await rect(page, 'tbody tr:nth-child(1) td:nth-child(1)');
    const row2td1 = await rect(page, 'tbody tr:nth-child(2) td:nth-child(1)');
    expect(Math.abs(row1td1.centerX - row2td1.centerX)).toBeLessThanOrEqual(1);
    expect(row2td1.top).toBeGreaterThanOrEqual(row1td1.bottom - 1);
  });
});

test('horizontally a table is unaffected by the vertical exception', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.setContent(articlePage({ vertical: false }));
  const writingMode = await page.locator('table').evaluate((el) => getComputedStyle(el).writingMode);
  expect(writingMode).toBe('horizontal-tb');
  const table = await rect(page, 'table');
  expect(table.width).toBeGreaterThan(table.height);
});

// `.moss-article-colophon`'s rule and gap were spelled `border-top`/
// `padding-top`, which stay on the physical top under vertical-rl instead of
// following the block-start edge (the column's right side, since block
// progression runs right to left) the way `margin-block-start` on the same
// selector already did.
test.describe('vertical-rl article colophon', () => {
  test('the rule and gap sit on the block-start edge — the column\'s right side — matching the horizontal gap size', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(articlePage({ vertical: true }));
    const b = await borders(page, '.moss-article-colophon');
    expect(b).toEqual({ top: '0px', left: '0px', bottom: '0px', right: '1px' });
    const padding = await page.locator('.moss-article-colophon').evaluate((el) => getComputedStyle(el).paddingRight);
    expect(padding).toBe('16px'); // --moss-space-sm
  });

  test('horizontally the rule and gap are unchanged: on the physical top, same 16px gap', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.setContent(articlePage({ vertical: false }));
    const b = await borders(page, '.moss-article-colophon');
    expect(b).toEqual({ top: '1px', left: '0px', bottom: '0px', right: '0px' });
    const padding = await page.locator('.moss-article-colophon').evaluate((el) => getComputedStyle(el).paddingTop);
    expect(padding).toBe('16px'); // --moss-space-sm
  });
});

