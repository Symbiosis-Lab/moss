// A `content_width: screen` page: what a picture in a full-width block
// fetches, and where the nav sits against the blocks below it.
//
// The page's width is known only after its body is rendered, so a picture in
// a top-level grid or band is first given the text column's `sizes=` and the
// build restates it for the widened block (`sizes_in_screen_block`,
// contract/sizes.rs). The golden file next to this spec holds the restated
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
const GOLDEN: Record<'grid2' | 'grid3' | 'band', string> = JSON.parse(
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

function page(body: string, theme = '') {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style><style>${theme}</style></head>
<body data-page="home" data-path="/" data-content-width="full">
<header><nav class="main-nav container"><div class="nav-content">
<div class="nav-left"><a href="/" class="site-name">Home</a></div>
<div class="nav-right"><div class="nav-links" id="nav-links"><a href="/about/">About</a></div></div>
</div></nav></header>
<main id="main-content"><article class="container">${body}</article></main></body></html>`;
}

const grid = (columns: number, sizes: string) =>
  `<div class="moss-grid" data-columns="${columns}" data-width="screen"><div class="moss-grid-card">${picture('pic', sizes)}</div>${
    '<div class="moss-grid-card"><p>Cell</p></div>'.repeat(columns - 1)}</div>`;
const band = (sizes: string) => `<div class="band" data-width="screen">${picture('pic', sizes)}</div>`;

async function fetchVsBox(p: Page, body: string) {
  await p.route(`${RUNG_HOST}/**`, (route) => route.fulfill({
    contentType: 'image/svg+xml',
    body: `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}"><rect width="100%" height="100%" fill="#2050c0"/></svg>`,
  }));
  await p.setContent(page(body));
  return p.locator('#pic').evaluate(async (img: HTMLImageElement) => {
    await img.decode().catch(() => {});
    const m = img.currentSrc.match(/\/(\d+)\.svg$/);
    return { picked: m ? Number(m[1]) : 0, need: img.getBoundingClientRect().width * devicePixelRatio };
  });
}

const VIEWPORTS: Array<[number, number]> = [[1280, 800], [1920, 1080], [390, 844]];
const BLOCKS: Array<[string, string]> = [
  ['a 2-column grid', grid(2, GOLDEN.grid2)],
  ['a 3-column grid', grid(3, GOLDEN.grid3)],
  ['a band', band(GOLDEN.band)],
];

for (const [name, body] of BLOCKS) {
  for (const [vw, vh] of VIEWPORTS) {
    for (const scale of [1, 2]) {
      test(`a picture in ${name} at ${vw}x${vh}@${scale}x fetches a rung covering its drawn width`, async ({ browser }) => {
        const ctx = await browser.newContext({ viewport: { width: vw, height: vh }, deviceScaleFactor: scale });
        const { picked, need } = await fetchVsBox(await ctx.newPage(), body);
        await ctx.close();
        expect(picked).toBeGreaterThanOrEqual(need - STEP);
        // An unparsed value falls back to `100vw`, which a grid cell overshoots.
        // A phone's grid is one column, and a band's picture is the window less its gutters.
        expect(picked).toBeLessThanOrEqual(need * 1.5 + STEP);
      });
    }
  }
}

test('the text column\'s value falls short in a screen grid, which is why it is restated', async ({ browser }) => {
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  const { picked, need } = await fetchVsBox(await ctx.newPage(), grid(2, 'calc(min(47.25rem, 100vw) / 2)'));
  await ctx.close();
  expect(picked).toBeLessThan(need - STEP);
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
