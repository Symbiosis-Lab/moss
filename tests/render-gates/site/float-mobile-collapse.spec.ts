// Every floated placement element — an image figure, a captioned
// `.moss-embed-figure`, a bare `.moss-embed`, or a `.moss-cards-container`
// listing — un-floats and fills the full column below 48rem, sized or not.
//
// Two desktop rules can otherwise survive onto a phone:
//
//   - The `@media (max-width: 48rem)` override used to spell its selectors
//     without the `:not([data-width])` the desktop 50%-cap rule carries, so
//     it under-specified against that rule and lost regardless of source
//     order — a stylesheet-vs-stylesheet conflict, fixed by matching
//     specificity.
//   - A content-relative size (`|NN%`, or the editor's drag-resize) rides as
//     an inline `style="width:NN%"` on the outermost element, which no
//     stylesheet declaration can outrank short of `!important` — a
//     stylesheet-vs-inline-style conflict, a different bug with a different
//     fix.
//
// Only a real engine's box layout proves either: a Rust test can assert
// which class/attribute strings got emitted, but not which one the cascade
// actually painted.
//
// Self-contained: injects the real site.css into `page.setContent`. No
// server, no MOSS_BIN.
import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { tokenBlock } from './tokens-block';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');
const TOKENS = `:root{\n${tokenBlock('light')}\n}`;

const DESKTOP = { width: 1280, height: 900 };
const MOBILE = { width: 390, height: 844 };

/** A wide, unbreakable filler so an unsized floated box's shrink-to-fit
 * width exceeds any percentage cap — the same reason a float containing a
 * long unbroken line grows to meet it. Without this an empty box's
 * shrink-to-fit is ~0, under any cap, and the cap would never bind. */
const FILLER = '<div style="width:2000px;height:2px"></div>';

type Kind = 'image' | 'embed-figure' | 'embed' | 'cards';

function targetHtml(kind: Kind, sized: boolean): string {
  const size = sized ? ' style="width:40%"' : '';
  switch (kind) {
    case 'image':
      return `<figure id="target" class="moss-image moss-align-right"${size}>${FILLER}<picture><img src="a.jpg" width="10" height="10" alt=""></picture></figure>`;
    case 'embed-figure':
      return `<figure id="target" class="moss-embed-figure moss-align-right"${size}>${FILLER}<div class="moss-embed" data-type="video"></div><figcaption>A caption</figcaption></figure>`;
    case 'embed':
      return `<div id="target" class="moss-embed moss-align-right" data-type="iframe"${size}>${FILLER}</div>`;
    case 'cards':
      return `<div id="target" class="moss-cards-container moss-align-right" data-embed${size}>${FILLER}<div class="moss-cards" data-layout="list"></div></div>`;
  }
}

function articlePage(kind: Kind, sized: boolean): string {
  // `<main>` carries no `container` class in real output (see
  // article-content.html: `<main id="main-content" tabindex="-1">`) — it is
  // `article.container` alone that establishes the column. Giving `<main>`
  // the class too (an easy copy-paste from another gate's fixture) double-
  // applies `.container`'s own `max-inline-size`/padding to `<main>`, and
  // because `main { container-type: inline-size }` gives it size
  // containment, the result is a `<main>` whose width no longer depends on
  // `article` at all — it collapses to just its own padding, taking
  // `article` and everything in it down to 0 width with it.
  return `<!doctype html><html lang="zh-Hant"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style>
</head><body>
<main id="main-content" tabindex="-1"><article class="container">
<h1>山居雜記</h1>
<p id="before">測試用長句之一，純屬虛構，非真跡也。字跡工整，行氣連貫，適合作為版面測試範例，足以撐開欄寬。</p>
${targetHtml(kind, sized)}
<p id="after">段落之二，同樣純屬虛構。</p>
</article></main>
</body></html>`;
}

const rect = (page: Page, sel: string) =>
  page.locator(sel).first().evaluate((el) => el.getBoundingClientRect().toJSON());
const floatOf = (page: Page, sel: string) =>
  page.locator(sel).first().evaluate((el) => getComputedStyle(el).float);

// `.moss-cards-container` carries `container-type: inline-size` (site.css,
// so `.moss-cards` can use `@container` instead of `@media`). Combined with
// `float` — which sizes an unconstrained box by shrink-to-fit, using its
// CONTENT's preferred width as an input — inline-size containment makes
// that input indeterminate: an unsized, floated `.moss-cards-container`
// with only `max-width` (no `width`) would compute `width: 0px`,
// independent of its content (verified: even a real
// `.moss-cards[data-layout=list]` with real card links collapsed the same
// way an empty div did). site.css gives it a definite `width: 50%` for
// exactly this case, sidestepping shrink-to-fit rather than capping it —
// see the rule beside the `max-width: 50%` one in site.css.

test.describe('floated embed/figure/listing mobile collapse', () => {
  for (const kind of ['image', 'embed-figure', 'embed', 'cards'] as const) {
    for (const sized of [false, true]) {
      const label = sized ? 'sized (inline style width:40%)' : 'unsized (desktop 50% default cap)';
      test(`${kind}, ${label}: floats + capped at desktop, full column at 390px`, async ({ page }) => {
        await page.setContent(articlePage(kind, sized));

        // Desktop: still floated, still capped to its authored/default width —
        // this gate is about the mobile override, not a desktop regression.
        await page.setViewportSize(DESKTOP);
        expect(await floatOf(page, '#target')).toBe('right');
        const dTarget = await rect(page, '#target');
        const dColumn = await rect(page, '#before'); // #before's width IS the containing block's content width
        const expectedRatio = sized ? 0.4 : 0.5;
        expect(dTarget.width / dColumn.width).toBeCloseTo(expectedRatio, 1);
        expect(Math.abs(dTarget.right - dColumn.right)).toBeLessThanOrEqual(2);
        if (kind === 'image' || kind === 'embed-figure') {
          const inlineGap = await page.locator('#target').evaluate((el) => parseFloat(getComputedStyle(el).marginLeft));
          expect(inlineGap).toBe(32);
        }

        // Mobile: un-floated, full column, inline width included.
        await page.setViewportSize(MOBILE);
        expect(await floatOf(page, '#target')).toBe('none');
        const mTarget = await rect(page, '#target');
        const mColumn = await rect(page, '#before');
        expect(mTarget.width / mColumn.width).toBeCloseTo(1, 1);
        expect(Math.abs(mTarget.left - mColumn.left)).toBeLessThanOrEqual(2);
        expect(Math.abs(mTarget.right - mColumn.right)).toBeLessThanOrEqual(2);
      });
    }
  }

  test('a width-token figure drops its float-side gap on mobile', async ({ page }) => {
    await page.setContent(articlePage('image', true).replace(
      'class="moss-image moss-align-right"',
      'class="moss-image moss-align-right" data-width="wide"',
    ));
    await page.setViewportSize(MOBILE);
    expect(await floatOf(page, '#target')).toBe('none');
    const margin = await page.locator('#target').evaluate((el) => getComputedStyle(el).marginLeft);
    expect(margin).toBe('0px');
    const target = await rect(page, '#target');
    const column = await rect(page, '#before');
    expect(Math.abs(target.left - column.left)).toBeLessThanOrEqual(2);
  });
});
