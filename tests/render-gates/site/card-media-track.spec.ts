// The media track of a listing grid, on both sides of the `@supports` gate.
//
// A grid card is cover-on-top / content-below. When a card has no cover, its
// content band must still start where its covered siblings' bands start, or a
// mixed row reads as broken. moss did that with a per-card shim — an empty
// `.moss-card-cover.moss-card-no-cover` div painted as a 4:3 gradient — which
// works in a mixed row and is absurd in a listing where NO card has a cover
// (a term root: 56 grey rectangles).
//
// Under `@supports (grid-template-rows: subgrid)` the track belongs to the
// row: cards span two implicit tracks and adopt them, so the media track is
// sized by the real covers in that row and collapses to zero when there are
// none. Both paths ship, so both are proved here, in both engines.
//
// Design: docs/archive/2026-09-07-card-media-track-subgrid.md
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(
  path.join(mossBuildAssets(), 'css/site.css'), 'utf8');

// The fallback path is asserted against the REAL rules with the `@supports`
// block excised, rather than against a hand-written copy that would drift.
// The block is found by its opening line and removed by brace balance.
function withoutSupports(css) {
  const open = css.indexOf('@supports (grid-template-rows: subgrid)');
  if (open === -1) throw new Error('the @supports subgrid block is gone — this gate is stale');
  let i = css.indexOf('{', open), depth = 0;
  for (; i < css.length; i++) {
    if (css[i] === '{') depth++;
    else if (css[i] === '}' && --depth === 0) break;
  }
  return css.slice(0, open) + css.slice(i + 1);
}

const TOKENS = `:root{
  --moss-space-2xs:4px; --moss-space-xs:6px; --moss-space-sm:8px;
  --moss-space-md:24px; --moss-space-lg:32px; --moss-space-2xl:64px;
  --moss-color-surface:#f4f4f4; --moss-color-bg:#fff; --moss-color-text:#111;
  --moss-color-muted:#666; --moss-border-light:#ddd; --moss-color-accent:#2d5a2d;
  --moss-reading-size:18px; --moss-card-min:160px;
}`;

// A 1x1 transparent GIF: a real cover, so the media track has something to be
// sized by, without the gate depending on a fixture file.
const PIXEL = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';

// Byte-for-byte grid_card.rs's two shapes: a covered card carries a real
// `<img>` inside `.moss-card-cover`; a coverless card carries the empty
// placeholder div. `data-list-has-covers` is emitted iff ANY card has one.
function card(title, covered, described) {
  const cover = covered
    ? `<div class="moss-card-cover"><img src="${PIXEL}" alt=""></div>`
    : `<div class="moss-card-cover moss-card-no-cover"></div>`;
  return `<a href="#" class="moss-card">${cover}`
    + `<div class="moss-card-content">`
    + `<span class="moss-card-meta"></span>`
    + `<span class="moss-card-title">${title}</span>`
    + (described ? `<p class="moss-card-description">A sentence of prose.</p>` : '')
    + `</div></a>`;
}

// Deliberately ragged: one short title, one that wraps to several lines. A
// row of cards must be equal height regardless, which is what the listing
// grid's half of the retired `grid-equal-height` second test used to assert.
const TITLES = ['A', 'This Is A Deliberately Very Long Multi Word Card Title That Wraps',
  'Two Words', 'C', 'Another Fairly Long Title Here'];

function listing(covers, { supports = true, width = 900, described = false } = {}) {
  const cards = covers.map((c, i) => card(TITLES[i % TITLES.length], c, described)).join('');
  const hasCovers = covers.some(Boolean) ? ' data-list-has-covers' : '';
  const css = supports ? CSS : withoutSupports(CSS);
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<style>${TOKENS}</style><style>${css}</style>
<style>body{margin:0;font-size:var(--moss-reading-size)}#wrap{width:${width}px}</style>
</head><body><div id="wrap"><div class="moss-cards-container">
<div class="moss-cards" data-layout="grid" data-list-axis="date"${hasCovers}>${cards}</div>
</div></div></body></html>`;
}

// Offset of each title from the top of its own card: the number that says
// whether content bands line up across a row.
// Every card, grouped into visual rows by its own top edge. Alignment is a
// WITHIN-row question: subgrid sizes the media track per grid row, so a row
// that happens to contain no cover collapses its track independently of a row
// that does. Comparing across rows would assert the opposite of the design.
const rows = (p) => p.evaluate(() => {
  const wrapper = document.querySelector('.moss-cards[data-layout="grid"]');
  const cells = [...wrapper.querySelectorAll('.moss-card')].map(c => {
    const box = c.getBoundingClientRect();
    const title = c.querySelector('.moss-card-title').getBoundingClientRect();
    const cover = c.querySelector('.moss-card-cover');
    return {
      top: Math.round(box.top),
      height: Math.round(box.height),
      titleOffset: Math.round(title.top - box.top),
      coverHeight: Math.round(cover.getBoundingClientRect().height),
      covered: !cover.classList.contains('moss-card-no-cover'),
    };
  });
  const byTop = new Map();
  for (const c of cells) {
    if (!byTop.has(c.top)) byTop.set(c.top, []);
    byTop.get(c.top).push(c);
  }
  return [...byTop.values()];
});

test('subgrid is what this branch relies on, and both engines have it', async ({ page }) => {
  await page.setContent('<!doctype html><html><body></body></html>');
  expect(await page.evaluate(() => CSS.supports('grid-template-rows', 'subgrid'))).toBe(true);
});

test.describe('subgrid path', () => {
  test('a mixed row aligns every content band, and the placeholder is gone', async ({ page }) => {
    // Five cards over two tracks: row 1 is mixed (one cover, one bare), rows 2
    // and 3 are all-bare. Within the mixed row every band must start at the
    // same offset with no placeholder occupying height; the all-bare rows must
    // collapse their media track independently, which is the whole point of
    // putting the track on the row instead of inside the card.
    await page.setContent(listing([true, false, false, false, false], { width: 900 }));
    const grid = await rows(page);
    expect(grid.length, 'expected three visual rows over two tracks').toBe(3);

    const [mixed, ...bare] = grid;
    expect(mixed.length).toBe(2);
    expect(mixed[0].titleOffset, 'a real cover did not size the media track').toBeGreaterThan(40);
    expect(mixed[1].titleOffset, 'a coverless band did not adopt the row media track')
      .toBe(mixed[0].titleOffset);
    expect(mixed[1].coverHeight, 'the placeholder still occupies height').toBe(0);
    expect(Math.abs(mixed[0].height - mixed[1].height),
      'cards in one row are not equal height').toBeLessThanOrEqual(1);

    for (const row of bare) {
      for (const c of row) {
        expect(c.coverHeight).toBe(0);
        expect(c.titleOffset, 'an all-bare row reserved media space anyway').toBeLessThan(40);
      }
    }
  });

  test('a coverless listing the INDEX state cannot reach still collapses its media track', async ({ page }) => {
    // Deliberately outside the roster: these cards carry descriptions, so the
    // index state's `:not(:has(.moss-card-description))` excludes them and its
    // cover-hiding rule never fires. This is an author's `children_style: grid`
    // over coverless children — the case where moss used to draw a gradient
    // rectangle above every card for media that does not exist. Nothing but
    // the media track being owned by the row can collapse it here.
    await page.setContent(listing([false, false, false], { described: true }));
    const cells = (await rows(page)).flat();
    expect(cells.length).toBe(3);
    for (const c of cells) {
      expect(c.coverHeight, 'a placeholder still reserved a 4:3 box').toBe(0);
      expect(c.titleOffset, 'a title sits below reserved media space').toBeLessThan(40);
    }
  });

  test('an all-covered listing is unchanged', async ({ page }) => {
    await page.setContent(listing([true, true, true]));
    const cells = (await rows(page)).flat();
    for (const c of cells) {
      expect(c.coverHeight).toBeGreaterThan(0);
      expect(c.titleOffset).toBe(cells[0].titleOffset);
    }
  });

  // docs/archive/2026-09-11-home-feed-cards-and-archive-link.md §5: measured
  // on a real site at 1280px, a 4:3 cover box 157px tall left the content
  // band starting ~40px below it — bare page colour between picture and
  // band. The media track is sized by the cover (proved above), so the gap
  // was the subgrid's OWN row-gap: `.moss-cards[data-layout="grid"]` sets one
  // `gap` for every row boundary it owns, and a subgridded card's two
  // tracks (media, content) are row boundaries of THIS grid too — the value
  // meant to separate one card-row from the next was leaking inside a
  // single card. The band must meet the picture with nothing between them.
  const bandGap = (page) => page.evaluate(() => {
    const cards = [...document.querySelectorAll('.moss-cards[data-layout="grid"] .moss-card')];
    return cards.map((c) => {
      const cover = c.querySelector('.moss-card-cover').getBoundingClientRect();
      const content = c.querySelector('.moss-card-content').getBoundingClientRect();
      return Math.round(content.top - cover.bottom);
    });
  });

  test('the content band meets the picture — no gap — at 1280px', async ({ page }) => {
    await page.setContent(listing([true, true, true], { width: 1280 }));
    // ≤1px, not exactly 0: the cover's `aspect-ratio` box has a fractional
    // height at most widths (e.g. 156.77px), and the subgrid row track that
    // sizes off it can snap to a different sub-pixel than the cover's own
    // `getBoundingClientRect` — the same fractional-aspect-ratio rounding
    // the `-1px` overshoot comment on `.moss-card-cover > img` describes.
    // Before this fix the gap was the full row-gap (~31px at this width),
    // not a rounding artifact.
    for (const gap of await bandGap(page)) {
      expect(Math.abs(gap), 'a gap opened between the cover box and its content band').toBeLessThanOrEqual(1);
    }
  });

  test('at 390px (below the 36rem phone-row threshold) the fix still carries no row-gap', async ({ page }) => {
    // Below 36rem `.moss-card` drops to `grid-row: auto` (a single-row span,
    // §2 of the same doc) rather than the two-row `span 2` this fix targets,
    // so there is no longer an internal gap for `row-gap: 0` to remove — the
    // assertion here is just that the declaration travels with the card at
    // this width too, not a geometry claim.
    //
    // NOT asserted here: that the cover and content actually sit side by
    // side — `grid-mobile-collapse.spec.ts` owns that geometry (§2's own
    // `display: flex` fix on the `@container` rule below, which this
    // block's `.moss-card` selector also matches).
    await page.setContent(listing([true, true, true], { width: 390 }));
    const rowGap = await page.locator('.moss-cards[data-layout="grid"] .moss-card')
      .first().evaluate((el) => getComputedStyle(el).rowGap);
    expect(rowGap).toBe('0px');
  });
});

test.describe('fallback path (engine without subgrid)', () => {
  test('the placeholder still reserves the box, so a mixed row still aligns', async ({ page }) => {
    // What a visitor on an older engine gets: exactly today's behaviour. The
    // shim is the alignment device, and it must keep working — the `@supports`
    // block is an enhancement, never a load-bearing dependency.
    await page.setContent(listing([true, false], { supports: false }));
    const cells = (await rows(page)).flat();
    for (const c of cells) {
      expect(c.coverHeight, 'the fallback lost the placeholder box').toBeGreaterThan(0);
      expect(c.titleOffset).toBe(cells[0].titleOffset);
    }
    const heights = cells.map(c => c.height);
    expect(Math.max(...heights) - Math.min(...heights),
      'ragged titles made a row uneven on the fallback path').toBeLessThanOrEqual(1);
  });

  test('the placeholder is painted, so it does not read as a hole', async ({ page }) => {
    await page.setContent(listing([true, false, false], { supports: false }));
    const bg = await page.locator('.moss-card-no-cover').first()
      .evaluate(el => getComputedStyle(el).backgroundImage);
    expect(bg, 'the fallback placeholder lost its gradient').toContain('linear-gradient');
  });
});
