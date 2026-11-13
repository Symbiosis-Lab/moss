// A `content_width: screen` page: what a picture in a full-width block
// fetches, and where the nav sits against the blocks below it. Also what a
// picture fetches in the column of a `wide` or `full` page.
//
// The page's width is known only after its body is rendered, so a picture in
// a top-level grid or band is first given the text column's `sizes=` and the
// build restates it for the widened block or the page's column
// (`sizes_in_screen_block`, `sizes_in_page_column`, contract/sizes.rs). The golden file next to this spec holds the restated
// values, kept honest by a moss-core test, because this gate cannot call Rust.
// Only an engine can say whether the rung it picks covers the box the CSS
// draws: the pictures here are eager, which is also how an engine without
// `sizes=auto` reads every picture.
import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { tokenBlock } from './tokens-block';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css', 'site.css'), 'utf8');
const GOLDEN: Record<'grid2' | 'grid3' | 'band' | 'ratio' | 'mid' | 'wideColumn' | 'fullColumn' | 'card', string> = JSON.parse(
  fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), 'screen-page.golden.json'), 'utf8'));

const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
${tokenBlock('light')}
} }`;

// Candidate rungs every 10px, so the pick reads back the width the browser
// resolved `sizes=` to. Only the chosen one is ever requested.
const RUNG_HOST = 'https://rung.invalid';
const STEP = 10;
const SRCSET = Array.from({ length: 500 }, (_, i) => (i + 1) * STEP)
  .map((n) => `${RUNG_HOST}/${n}.svg ${n}w`).join(', ');
const [W, H] = [2400, 1350];

const picture = (id: string, sizes: string) =>
  `<figure class="moss-image"><img id="${id}" srcset="${SRCSET}" sizes="${sizes}" width="${W}" height="${H}" loading="eager" alt=""></figure>`;

function page(body: string, theme = '', contentWidth = 'full') {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style><style>${theme}</style></head>
<body data-page="home" data-path="/" data-content-width="${contentWidth}">
<header><nav class="main-nav container"><div class="nav-content">
<div class="nav-left"><a href="/" class="site-name">Home</a></div>
<div class="nav-right"><div class="nav-links" id="nav-links"><a href="/about/">About</a></div></div>
</div></nav></header>
<main id="main-content"><article class="container">${body}</article></main></body></html>`;
}

const grid = (columns: number, sizes: string) =>
  `<div class="moss-grid" data-columns="${columns}" data-width="screen"><div class="moss-grid-card">${picture('pic', sizes)}</div>${
    '<div class="moss-grid-card"><p>Cell</p></div>'.repeat(columns - 1)}</div>`;
// `:::grid 2 5:7`, the picture in the wider cell.
const ratioGrid = (sizes: string) =>
  `<div class="moss-grid" data-columns="2" data-width="screen" style="--moss-grid-ratio:minmax(0, 5fr) minmax(0, 7fr)"><div class="moss-grid-card"><p>Cell</p></div><div class="moss-grid-card">${picture('pic', sizes)}</div></div>`;
const midGrid = (sizes: string) => grid(4, sizes).replace('data-columns="4"', 'data-columns="4" data-columns-mid="2"');
// A page's card, as a grid cell that links to it becomes.
const cardGrid = (sizes: string) =>
  `<div class="moss-grid" data-columns="2" data-width="screen"><a href="#" class="moss-card"><div class="moss-card-cover"><img id="pic" srcset="${SRCSET}" sizes="${sizes}" width="${W}" height="${H}" loading="eager" alt=""></div><div class="moss-card-content"><span class="moss-card-meta"></span><span class="moss-card-title">Card</span></div></a><div class="moss-grid-card"><p>Cell</p></div></div>`;
// `:::grid {collage}` of four: the lead, then three tiles a sixth of the band each.
const collage = (sizes: string, pictured: number) =>
  `<div class="moss-grid" data-columns="1" data-collage data-count="4" data-width="screen">${[0, 1, 2, 3].map((i) =>
    `<div class="moss-grid-card" data-tile="picture"${i ? ' style="--moss-collage-span:2;--moss-collage-span-sm:2"' : ''}>${
      i === pictured ? picture('pic', sizes) : '<p>Tile</p>'}</div>`).join('')}</div>`;
const band = (sizes: string) => `<div class="band" data-width="screen">${picture('pic', sizes)}</div>`;

async function fetchVsBox(p: Page, body: string, contentWidth = 'full') {
  await p.route(`${RUNG_HOST}/**`, (route) => route.fulfill({
    contentType: 'image/svg+xml',
    body: `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}"><rect width="100%" height="100%" fill="#2050c0"/></svg>`,
  }));
  await p.setContent(page(body, '', contentWidth));
  return p.locator('#pic').evaluate(async (img: HTMLImageElement) => {
    await img.decode().catch(() => {});
    const m = img.currentSrc.match(/\/(\d+)\.svg$/);
    return { picked: m ? Number(m[1]) : 0, need: img.getBoundingClientRect().width * devicePixelRatio };
  });
}

// 900 is between 48rem and 64rem, where `mid` sets the columns.
const VIEWPORTS: Array<[number, number]> = [[1280, 800], [1920, 1080], [900, 800], [390, 844]];
const BLOCKS: Array<[string, string, string?]> = [
  ['a 2-column grid', grid(2, GOLDEN.grid2)],
  ['a 3-column grid', grid(3, GOLDEN.grid3)],
  ['a band', band(GOLDEN.band)],
  ['the wider cell of a 5:7 grid', ratioGrid(GOLDEN.ratio)],
  ['a 4-column grid with mid=2', midGrid(GOLDEN.mid)],
  ['a card in a 2-column grid', cardGrid(GOLDEN.grid2)],
  ['the lead of a collage', collage(GOLDEN.grid2, 0)],
  ['the column of a wide page', picture('pic', GOLDEN.wideColumn), 'wide'],
  ['the column of a full page', picture('pic', GOLDEN.fullColumn), 'full'],
];

for (const [name, body, contentWidth] of BLOCKS) {
  for (const [vw, vh] of VIEWPORTS) {
    for (const scale of [1, 2]) {
      test(`a picture in ${name} at ${vw}x${vh}@${scale}x fetches a rung covering its drawn width`, async ({ browser }) => {
        const ctx = await browser.newContext({ viewport: { width: vw, height: vh }, deviceScaleFactor: scale });
        const { picked, need } = await fetchVsBox(await ctx.newPage(), body, contentWidth);
        await ctx.close();
        expect(picked).toBeGreaterThanOrEqual(need - STEP);
        // An unparsed value falls back to `100vw`, which a grid cell overshoots.
        // A phone's grid is one column, and a band's picture is the window less its gutters.
        expect(picked).toBeLessThanOrEqual(need * 1.5 + STEP);
      });
    }
  }
}

// What each picture declared before its value followed the grid's tracks, the
// cell a card fills, or the page's column.
const column = '(min-width: 48rem) 47.25rem, 100vw';
const SHORT: Array<[string, string, number, string?]> = [
  ['the text column\'s cell in a screen grid', grid(2, 'calc(min(47.25rem, 100vw) / 2)'), 1920],
  ['an even cell in a 5:7 grid', ratioGrid(GOLDEN.grid2), 1920],
  ['the 4-column cell under mid=2', midGrid('(min-width: 48rem) calc(100vw / 4), 100vw'), 900],
  ['a listing card\'s value in a screen grid', cardGrid(GOLDEN.card), 1920],
  ['the default column on a wide page', picture('pic', column), 1920, 'wide'],
  ['the default column on a full page', picture('pic', column), 1920, 'full'],
];
for (const [name, body, vw, contentWidth] of SHORT) {
  test(`${name} falls short at ${vw}`, async ({ browser }) => {
    const ctx = await browser.newContext({ viewport: { width: vw, height: 1080 } });
    const { picked, need } = await fetchVsBox(await ctx.newPage(), body, contentWidth);
    await ctx.close();
    expect(picked).toBeLessThan(need - STEP);
  });
}

test('a small collage tile does not fetch a file as wide as the band', async ({ browser }) => {
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  const { picked, need } = await fetchVsBox(await ctx.newPage(), collage(GOLDEN.grid2, 1));
  await ctx.close();
  expect(picked).toBeGreaterThanOrEqual(need - STEP);
  expect(picked).toBeLessThanOrEqual(1920 / 2 + STEP);
});

// The nav's links and a band's text share one column, so a theme has nothing
// to restate; `--moss-nav-width` alone moves the nav off it.
async function edges(p: Page, theme: string) {
  await p.setContent(page('<div class="band" data-width="screen"><p id="text">Band text.</p></div>', theme));
  return p.evaluate(() => {
    const box = (s: string) => {
      const r = document.querySelector(s)!.getBoundingClientRect();
      return { left: Math.round(r.left), right: Math.round(r.right) };
    };
    return { nav: box('.nav-content'), band: box('#text'), navBox: box('.main-nav') };
  });
}

for (const vw of [1280, 390]) {
  test(`the nav's edges are the band's at ${vw}`, async ({ browser }) => {
    const ctx = await browser.newContext({ viewport: { width: vw, height: 800 } });
    const { nav, band } = await edges(await ctx.newPage(), '');
    await ctx.close();
    expect(nav).toEqual(band);
  });
}

test('--moss-nav-width alone sets the nav\'s width', async ({ browser }) => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const { navBox } = await edges(await ctx.newPage(), ':root { --moss-nav-width: 60rem; }');
  await ctx.close();
  expect(navBox.right - navBox.left).toBe(960);
});
