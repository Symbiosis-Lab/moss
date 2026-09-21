// `.date-line`'s top-alignment with the title under `writing-mode:
// vertical-rl` (article-date design, round 2, found on a real
// vertical-writing site). `.date-line` used
// `margin-top`/`margin-bottom` — physical properties that keep pushing the
// box down the page's vertical axis no matter which axis is actually the
// block axis. In horizontal-tb the block axis IS vertical, so the physical
// margin happened to read as "gap after the title." Under vertical-rl the
// block axis rotates to horizontal, but the physical margin does not rotate
// with it: it kept shoving `.date-line` 8px down the (now-inline) vertical
// axis, the exact axis the article's own top-alignment invariant (h1 and the
// first paragraph share a top edge, by ordinary block flow) lives on.
//
// Only a real engine's box layout can see a writing-mode-rotated margin
// error — jsdom does not implement `writing-mode` at all, and a horizontal-
// only assertion can't distinguish "gap below" from "gap before," since
// physical and logical read identically when the block axis is vertical.
//
// Self-contained: injects the real site.css + vertical.css into
// `page.setContent`. No server, no MOSS_BIN.
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { openCrateDir } from '../../support/crate-paths';

const cssDir = path.join(openCrateDir('moss-build'), 'src/assets/css');
const CSS = fs.readFileSync(path.join(cssDir, 'site.css'), 'utf8');
const VERTICAL = fs.readFileSync(path.join(cssDir, 'site/vertical.css'), 'utf8');

// Tokens are generated from tokens.json at build time; site.css itself is a
// fragment (its own comment: "Token definitions … are generated at build
// time … The @layer order declaration is also part of that generated
// prefix"). Without this, `var(--moss-space-xs)` etc. are invalid at
// computed-value time and margins/font-sizes silently collapse to 0 — which
// made an early version of this gate pass on unfixed CSS for the wrong
// reason. Same values as vertical-measure.spec.js's TOKENS.
const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
  --moss-reading-size-base:1.125rem;
  --moss-reading-size:var(--moss-reading-size-base);
  --moss-content-width:calc(42 * var(--moss-reading-size));
  --moss-vertical-font-size:clamp(1rem, 1.9svh, 1.4rem);
  --moss-site-max-width:1200px;
  --moss-container-padding:clamp(1rem, 5vw, 2rem);
  --moss-space-2xs:4px; --moss-space-xs:8px; --moss-space-sm:8px;
  --moss-space-md:24px; --moss-space-lg:32px; --moss-space-xl:48px; --moss-space-2xl:64px;
  --moss-color-surface:#f4f4f4; --moss-color-bg:#fff; --moss-color-text:#111;
  --moss-color-muted:#666; --moss-border-light:#ddd; --moss-border-medium:#bbb; --moss-border-strong:#8e8b85;
  --moss-size-md:1.125rem; --moss-color-text-secondary:#5d5853;
  --moss-font-heading-weight:480;
  --moss-read-h1:2rem; --moss-read-h2:1.6rem; --moss-read-h3:1.3rem;
  --moss-read-h4:1.15rem; --moss-read-h5:1rem; --moss-read-h6:1rem;
} }`;

// The date-line/font-anchor/font-pill HTML, byte-for-byte as html.rs emits
// it for an article page with a date (mirrors the vertical-nav-chrome
// fixture's own copy of this markup).
const DATE_LINE =
  '<div class="date-line"><span class="date">一八三〇年 九月</span>' +
  '<div class="font-anchor"><button class="font-trigger size-std" aria-label="Reading preferences" aria-expanded="false" type="button"></button>' +
  '<div class="font-pill" id="fontPill"><button data-scale="small" aria-label="Small"></button>' +
  '<button data-scale="" class="active" aria-label="Standard"></button>' +
  '<button data-scale="large" aria-label="Large"></button>' +
  '<button data-scale="xlarge" aria-label="X-Large"></button></div></div></div>';

function articlePage({ vertical }: { vertical: boolean }) {
  return `<!doctype html><html lang="zh-Hant"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style>${vertical ? `<style>${VERTICAL}</style>` : ''}
</head>
<body${vertical ? ' data-typesetting="vertical"' : ''}>
<main class="container">
<article class="container">
<h1>山居雜記</h1>
${DATE_LINE}
<p id="p1">測試用長句之一，純屬虛構，非真跡也。</p>
<p>字跡工整，行氣連貫，適合作為版面測試範例。</p>
</article>
</main>
</body></html>`;
}

const rect = (page: import('@playwright/test').Page, sel: string) =>
  page.locator(sel).first().evaluate((el) => el.getBoundingClientRect());

test.describe('article date-line alignment', () => {
  test('vertical-rl: the date row shares the title\'s block-start (top) edge', async ({ page }) => {
    await page.setContent(articlePage({ vertical: true }));
    await page.setViewportSize({ width: 1400, height: 1000 });

    const dateRect = await rect(page, '.date-line');
    const h1Rect = await rect(page, 'article h1');
    const pRect = await rect(page, '#p1');

    expect(Math.abs(dateRect.top - h1Rect.top)).toBeLessThanOrEqual(1);
    expect(Math.abs(dateRect.top - pRect.top)).toBeLessThanOrEqual(1);
  });

  test('horizontal-tb: the date row keeps sharing the title\'s inline-start (left) edge', async ({ page }) => {
    await page.setContent(articlePage({ vertical: false }));
    await page.setViewportSize({ width: 1400, height: 1000 });

    const dateRect = await rect(page, '.date-line');
    const h1Rect = await rect(page, 'article h1');
    const pRect = await rect(page, '#p1');

    expect(Math.abs(dateRect.left - h1Rect.left)).toBeLessThanOrEqual(1);
    expect(Math.abs(dateRect.left - pRect.left)).toBeLessThanOrEqual(1);
    // The block-axis stacking this gate is not about: title above date
    // above body, still true after the logical-property rewrite.
    expect(dateRect.top).toBeGreaterThan(h1Rect.top);
    expect(pRect.top).toBeGreaterThan(dateRect.top);
  });
});
