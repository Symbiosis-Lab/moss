// What a vertical page's body image fetches, against the column the real CSS
// lays it out in.
//
// Under vertical-rl the column is a HEIGHT (`--moss-vertical-column` in
// vertical.css) and an image's physical width is that height times its aspect.
// The horizontal `sizes=` named a 47.25rem width instead, so a painting laid
// out ~794px wide fetched the 800w rung and a wide scroll a fraction of what it
// needed (found on a real vertical-writing site, 2026-09-23). The value now
// comes from `sizes_vertical_body` (contract/sizes.rs); the golden file next to
// this spec is what the synthesizer emits for each source shape, kept honest by
// a moss-core test, because this gate cannot call Rust.
//
// Two things only an engine can answer: whether it parses the value at all
// (WebKit drops `min()`/`clamp()` in `sizes=` and falls back to `100vw`), and
// whether the rung it picks covers the box the CSS actually draws. The second
// also pins drift: if `--moss-vertical-column` ever outgrows `100vh - 4rem`,
// the chosen rung falls short here.
import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { tokenBlock } from './tokens-block';
import { mossBuildAssets } from '../../support/crate-paths';

const cssDir = path.join(mossBuildAssets(), 'css');
const CSS = fs.readFileSync(path.join(cssDir, 'site.css'), 'utf8');
const VERTICAL = fs.readFileSync(path.join(cssDir, 'site/vertical.css'), 'utf8');
const GOLDEN: Record<string, string> = JSON.parse(
  fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), 'vertical-sizes.golden.json'), 'utf8'));

const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
${tokenBlock('light')}
} }`;

// Candidate rungs every 10px, so the pick reads back the width the browser
// resolved `sizes=` to. Only the chosen one is ever requested.
const RUNG_HOST = 'https://rung.invalid';
const STEP = 10;
const srcset = Array.from({ length: 3000 }, (_, i) => (i + 1) * STEP)
  .map((n) => `${RUNG_HOST}/${n}.svg ${n}w`).join(', ');

function page(w: number, h: number, sizes: string) {
  return `<!doctype html><html lang="zh-Hant"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style><style>${VERTICAL}</style></head>
<body data-typesetting="vertical"><main id="main-content"><article class="container">
<p>正文。</p>
<figure class="moss-image"><img srcset="${srcset}" sizes="${sizes}" width="${w}" height="${h}" alt=""></figure>
</article></main></body></html>`;
}

async function fetchVsBox(p: Page, w: number, h: number, sizes: string) {
  await p.route(`${RUNG_HOST}/**`, (route) => route.fulfill({
    contentType: 'image/svg+xml',
    body: `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="#2050c0"/></svg>`,
  }));
  await p.setContent(page(w, h, sizes));
  return p.locator('figure img').evaluate(async (img: HTMLImageElement) => {
    await img.decode().catch(() => {});
    const m = img.currentSrc.match(/\/(\d+)\.svg$/);
    return { picked: m ? Number(m[1]) : 0, width: img.getBoundingClientRect().width, dpr: devicePixelRatio };
  });
}

const VIEWPORTS: Array<[number, number]> = [[1440, 900], [1920, 1080], [1280, 600], [390, 844]];

for (const [shape, sizes] of Object.entries(GOLDEN)) {
  const [w, h] = shape.split('x').map(Number);
  for (const [vw, vh] of VIEWPORTS) {
    for (const scale of [1, 2]) {
      test(`${shape} at ${vw}x${vh}@${scale}x fetches a rung covering its drawn width, without gross excess`, async ({ browser }) => {
        const ctx = await browser.newContext({ viewport: { width: vw, height: vh }, deviceScaleFactor: scale });
        const p = await ctx.newPage();
        const { picked, width, dpr } = await fetchVsBox(p, w, h, sizes);
        await ctx.close();
        const need = width * dpr;
        // Never short of the drawn box (a short rung is upscaled: blur).
        expect(picked).toBeGreaterThanOrEqual(need - STEP);
        // The bound drops the 38em term, so it over-states by ~1.3–1.45×;
        // a `100vw` fallback (an unparsed value) over-states far more on
        // desktop and falls short on a phone.
        expect(picked).toBeLessThanOrEqual(need * 1.5 + STEP);
      });
    }
  }
}
