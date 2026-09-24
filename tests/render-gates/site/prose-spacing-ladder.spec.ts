/**
 * Render gate: article prose vertical spacing is a ladder of LINES of body
 * text (--moss-read-line = reading size * leading), not fixed px.
 *
 * Before this, `article p`/`h1`-`h6`/figure margins read the fixed
 * --moss-space-* rem scale — the same chrome geometry a nav island or a card
 * uses — so the gap between two paragraphs stayed the same px whether body
 * text was rendered at 16px or 24px (the four reader Aa steps), and the gap
 * above a `##` heading (48px) landed exactly on the gap below a photo (48px)
 * by coincidence rather than by any relationship between the two. Expressing
 * both in the same unit as the type they separate fixes both: the ratio holds
 * at every Aa step and script, and the figure gap (1.25L) now reads smaller
 * than the gap above a section heading (1.5L) as the design intends.
 *
 * Only a real engine resolves this: every gap is a `calc()` chain over custom
 * properties, some behind a `lang` attribute selector, some behind the same
 * `html.scale-*` classes the reading-scale-order gate uses, and margin
 * COLLAPSING between siblings — jsdom implements none of it.
 *
 * `--moss-read-line`'s own px value is read off a hidden probe element via
 * `font-size`, the standard way to pull a `calc()`-resolved length out of a
 * custom property (`getComputedStyle` never resolves a var() to px on its
 * own — only a real CSS property does).
 *
 * Run via:
 *   npx playwright test -c playwright/prose-spacing-ladder.config.ts
 */
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { tokenBlock } from './tokens-block';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');
// Layered, matching the real build's prefix (and vertical-sizes.spec.ts): almost
// all of site.css sits inside `@layer shortcodes`, and a higher layer always
// beats a lower one regardless of specificity — so an UNLAYERED `:root` block
// would make html.scale-*'s and html[lang^="zh"]'s own overrides of
// --moss-reading-size/--moss-read-leading (both inside @layer shortcodes) lose
// to this fixture's tokens instead of winning by specificity, silently testing
// one configuration under every scale/lang label. Confirmed by hand before
// this comment was written: an unlayered `:root` here left every scale and
// lang case measuring the same default reading size.
const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
${tokenBlock('light')}
} }`;

const SWATCH = 'data:image/svg+xml,' + encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect width="64" height="48" fill="#6a9a5a"/></svg>');

/** Invented placeholder prose — structural filler only, not copy from any real site. */
const COPY: Record<'en' | 'zh-Hant', Record<string, string>> = {
  en: {
    p1: 'Paragraph one sets the baseline every other gap in this fixture is measured against.',
    p2: 'Paragraph two follows paragraph one directly, with nothing between them.',
    h2Solo: 'A level-two heading, preceded by prose',
    p3: 'Paragraph three follows the level-two heading above.',
    h1Solo: 'A level-one heading, preceded by prose',
    h2AfterH1: 'A level-two heading, directly after the level-one heading above',
    p4: 'Paragraph four follows the second heading.',
    caption: 'A figure caption',
    p5: 'Paragraph five follows the figure.',
  },
  'zh-Hant': {
    p1: '第一段落是這份夾具其他每一段間距的比較基準。',
    p2: '第二段緊接在第一段之後，中間沒有任何內容。',
    h2Solo: '前面接著一段散文的二級標題',
    p3: '第三段緊接在上面的二級標題之後。',
    h1Solo: '前面接著一段散文的一級標題',
    h2AfterH1: '緊接在上面一級標題之後的二級標題',
    p4: '第四段緊接在第二個標題之後。',
    caption: '圖說文字',
    p5: '第五段緊接在圖片之後。',
  },
};

type Lang = keyof typeof COPY;

function page(lang: Lang, scaleClass: string) {
  const c = COPY[lang];
  const htmlAttrs = [`lang="${lang}"`, scaleClass ? `class="${scaleClass}"` : null]
    .filter(Boolean).join(' ');
  return `<!doctype html><html ${htmlAttrs}><head><meta charset="utf-8">
<style>${TOKENS}</style><style>${CSS}</style></head>
<body><main><article class="container">
<p id="p1">${c.p1}</p>
<p id="p2">${c.p2}</p>
<h2 id="h2-solo">${c.h2Solo}</h2>
<p id="p3">${c.p3}</p>
<h1 id="h1-solo">${c.h1Solo}</h1>
<h2 id="h2-after-h1">${c.h2AfterH1}</h2>
<p id="p4">${c.p4}</p>
<figure id="fig1"><img src="${SWATCH}" width="64" height="48" alt=""><figcaption>${c.caption}</figcaption></figure>
<p id="p5">${c.p5}</p>
</article></main>
<div id="line-probe" style="position:absolute;visibility:hidden;width:0;height:0;font-size:var(--moss-read-line);">x</div>
</body></html>`;
}

/** One line of body text, in px, resolved off the hidden probe's font-size. */
async function lineLengthPx(pg: import('@playwright/test').Page): Promise<number> {
  return pg.locator('#line-probe').evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
}

async function gapPx(pg: import('@playwright/test').Page, aboveId: string, belowId: string): Promise<number> {
  return pg.evaluate(({ aboveId, belowId }) => {
    const above = document.getElementById(aboveId)!.getBoundingClientRect();
    const below = document.getElementById(belowId)!.getBoundingClientRect();
    return below.top - above.bottom;
  }, { aboveId, belowId });
}

// The reader's Aa control applies one of these classes to <html> (site.css
// ~2667, `html.scale-*`) — pure CSS, so setting the class directly here
// exercises the real rule without needing the site JS that normally writes it.
const SCALES: Record<string, string> = {
  default: '',
  small: 'scale-small',
  large: 'scale-large',
  xlarge: 'scale-xlarge',
};

const LANGS: Lang[] = ['en', 'zh-Hant'];

// gap / L for every rule the ladder sets, ± this much (task tolerance).
const TOLERANCE = 0.03;

for (const lang of LANGS) {
  for (const [step, scaleClass] of Object.entries(SCALES)) {
    test(`${lang} · Aa ${step}: the spacing ladder holds in lines of body text`, async ({ page: pg }) => {
      await pg.setContent(page(lang, scaleClass));
      const L = await lineLengthPx(pg);
      expect(L, 'the probe must resolve --moss-read-line to a real px length').toBeGreaterThan(0);

      const ratios = {
        pp: (await gapPx(pg, 'p1', 'p2')) / L,
        aboveH2: (await gapPx(pg, 'p2', 'h2-solo')) / L,
        belowH2: (await gapPx(pg, 'h2-solo', 'p3')) / L,
        aboveH1: (await gapPx(pg, 'p3', 'h1-solo')) / L,
        h2AfterH1: (await gapPx(pg, 'h1-solo', 'h2-after-h1')) / L,
        figure: (await gapPx(pg, 'p4', 'fig1')) / L,
      };

      const line = Object.entries(ratios).map(([k, v]) => `${k}=${v.toFixed(3)}L`).join(' ');

      expect(Math.abs(ratios.pp - 0.75), `paragraph gap must be 0.75L ±${TOLERANCE} — measured ${line}`).toBeLessThanOrEqual(TOLERANCE);
      expect(Math.abs(ratios.aboveH2 - 1.5), `gap above a solo h2 must be 1.5L ±${TOLERANCE} — measured ${line}`).toBeLessThanOrEqual(TOLERANCE);
      expect(Math.abs(ratios.belowH2 - 0.5), `gap below h2 must be 0.5L ±${TOLERANCE} — measured ${line}`).toBeLessThanOrEqual(TOLERANCE);
      expect(Math.abs(ratios.aboveH1 - 2), `gap above h1 must be 2L ±${TOLERANCE} — measured ${line}`).toBeLessThanOrEqual(TOLERANCE);
      expect(Math.abs(ratios.h2AfterH1 - 0.5), `an h2 directly after an h1 must get 0.5L ±${TOLERANCE}, not the solo 1.5L — measured ${line}`).toBeLessThanOrEqual(TOLERANCE);
      expect(Math.abs(ratios.figure - 1.25), `figure gap must be 1.25L ±${TOLERANCE} — measured ${line}`).toBeLessThanOrEqual(TOLERANCE);
      expect(ratios.figure, `figure gap (${ratios.figure.toFixed(3)}L) must be smaller than the gap above a section heading (${ratios.aboveH2.toFixed(3)}L) — measured ${line}`).toBeLessThan(ratios.aboveH2);
    });
  }
}

async function marginBlockPx(pg: import('@playwright/test').Page, id: string) {
  return pg.locator(`#${id}`).evaluate((el) => {
    const s = getComputedStyle(el);
    return { start: parseFloat(s.marginBlockStart), end: parseFloat(s.marginBlockEnd) };
  });
}

// xlarge throughout below: it separates a value that tracks --moss-read-line
// from one that doesn't by ~11-15px, comfortably outside TOLERANCE — at the
// default scale the two kinds of value coincide too closely (by design, per
// the module comment) for a wrong wiring to show up.

function heroSiblingPage(scaleClass: string) {
  return `<!doctype html><html lang="en" class="${scaleClass}"><head><meta charset="utf-8">
<style>${TOKENS}</style><style>${CSS}</style></head>
<body><div class="moss-hero"></div>
<main><article class="container">
<h2 id="hero-h2">A level-two heading under a hero, standing in for the page's H1</h2>
</article></main>
<div id="line-probe" style="position:absolute;visibility:hidden;width:0;height:0;font-size:var(--moss-read-line);">x</div>
</body></html>`;
}

test('a .moss-hero sibling h2 scales its chapter-break bump with the reading size, not a fixed px', async ({ page: pg }) => {
  await pg.setContent(heroSiblingPage('scale-xlarge'));
  const L = await lineLengthPx(pg);
  const { start } = await marginBlockPx(pg, 'hero-h2');
  expect(Math.abs(start / L - 2), `.moss-hero ~ main article h2's top margin must be 2L, got ${start}px over L=${L}px (${(start / L).toFixed(3)}L)`).toBeLessThanOrEqual(TOLERANCE);
});

function cardTitleInArticlePage(scaleClass: string) {
  return `<!doctype html><html lang="en" class="${scaleClass}"><head><meta charset="utf-8">
<style>${TOKENS}</style><style>${CSS}</style></head>
<body><main><article class="container">
<p id="lead">Lead paragraph before a card embedded by a folder-listing shortcode.</p>
<h3 class="moss-card-title" id="embedded-card-title">An embedded card's own title</h3>
</article></main>
</body></html>`;
}

test('a .moss-card-title heading embedded in <article> keeps its fixed chrome margin, not the prose ladder', async ({ page: pg }) => {
  await pg.setContent(cardTitleInArticlePage('scale-xlarge'));
  const { start, end } = await marginBlockPx(pg, 'embedded-card-title');
  // --moss-space-xl / --moss-space-md — fixed chrome geometry, same value a
  // card-title gets anywhere else, regardless of the reading scale.
  expect(start, `card-title top margin must stay the fixed --moss-space-xl (48px), got ${start}px`).toBeCloseTo(48, 0);
  expect(end, `card-title bottom margin must stay the fixed --moss-space-md (24px), got ${end}px`).toBeCloseTo(24, 0);
});
