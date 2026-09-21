// @ts-check
// The footer-injected `:::subscribe` form's inline-axis anchor, under both
// writing modes, against a real vertical-writing site's own markup shape.
//
// `footer .moss-subscribe` right-anchors on desktop by putting `auto` on the
// inline-start margin so the form lines up with the content column's
// inline-end edge — 32px in from the container, the footer's own padding.
// Written as the physical `margin` shorthand (`margin: space-sm 0 0 auto`),
// that only coincides with `margin-inline-start: auto` under horizontal-tb:
// under vertical-rl, physical left is the block-end edge and physical top is
// inline-start, so the shorthand's `auto` lands on the wrong axis entirely
// and the form floats mid-column instead of sitting at the content's
// block-end (visually: bottom) edge. Horizontally the two spellings agree,
// so no horizontal-only gate can see this — only an engine laying out a
// vertical body can. See docs/archive/2026-09-17-subscribe-footer-vertical-
// alignment-design.md.
//
// Fixture: email.css's `footer .moss-subscribe` rule is injected the same
// way `enhance::wrap_style_blocks_in_layer` wraps it in production — nested
// inside `@layer plugins{…}` so it lands in `plugins.shortcodes`, which
// outranks site.css's own `shortcodes` layer — alongside site.css +
// vertical.css, on top of the real footer markup this was found against
// (auto-injected `:::subscribe` form beside a link list).
import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { openCrateDir } from '../../support/crate-paths';

const cssDir = path.join(openCrateDir('moss-build'), 'src/assets/css');
const CSS = fs.readFileSync(path.join(cssDir, 'site.css'), 'utf8');
const VERTICAL = fs.readFileSync(path.join(cssDir, 'site/vertical.css'), 'utf8');
const EMAIL = fs.readFileSync(path.join(cssDir, 'email.css'), 'utf8');

// Mirrors vertical-measure.spec.js's token block: production values for the
// layout tokens this measure reads, in @layer tokens under the production
// layer order.
const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
  --moss-reading-size-base:1.125rem;
  --moss-reading-size:var(--moss-reading-size-base);
  --moss-content-width:calc(42 * var(--moss-reading-size));
  --moss-vertical-font-size:clamp(1rem, 1.9svh, 1.4rem);
  --moss-site-max-width:1200px;
  --moss-container-padding:clamp(1rem, 5vw, 2rem);
  --moss-space-2xs:4px; --moss-space-xs:6px; --moss-space-sm:8px;
  --moss-space-md:24px; --moss-space-lg:32px; --moss-space-xl:48px; --moss-space-2xl:64px;
  --moss-color-surface:#f4f4f4; --moss-color-bg:#fff; --moss-color-text:#111;
  --moss-color-muted:#666; --moss-border-light:#ddd; --moss-border-medium:#bbb; --moss-border-strong:#8e8b85;
  --moss-size-md:1.125rem; --moss-color-text-secondary:#5d5853;
  --moss-font-heading-weight:480;
} }`;

const head = (vertical) => `<!doctype html><html lang="zh-Hant"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style>${vertical ? `<style>${VERTICAL}</style>` : ''}
<style>@layer plugins{${EMAIL}}</style>
</head>`;

// The footer shape this was found against: a link list beside the
// auto-injected `:::subscribe` form, driving
// `footer.container:has(> .moss-subscribe)`'s flex row.
const footer = '<footer class="container"><p class="footer-default">'
  + '<a href="/rss.xml">RSS</a></p>'
  + '<div class="moss-subscribe"><form class="moss-subscribe-form" data-position="inline">'
  + '<input class="moss-input" type="email" placeholder="email">'
  + '<button type="submit">訂閱</button></form></div></footer>';

function buildPage({ vertical }) {
  return `${head(vertical)}<body${vertical ? ' data-typesetting="vertical"' : ''}>
<main id="main-content"><article class="container"><p>正文。</p></article></main>
${footer}
</body></html>`;
}

const rect = (page, sel) =>
  page.locator(sel).first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  });

test.describe('footer subscribe form inline-end anchor', () => {
  test('right-anchors against the content column under horizontal-tb', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.setContent(buildPage({ vertical: false }));
    const container = await rect(page, 'footer.container');
    const form = await rect(page, '.moss-subscribe');
    expect(Math.abs(container.right - form.right - 32)).toBeLessThanOrEqual(3);
  });

  test('bottom-anchors against the content column under vertical-rl', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.setContent(buildPage({ vertical: true }));
    const container = await rect(page, 'footer.container');
    const form = await rect(page, '.moss-subscribe');
    expect(Math.abs(container.bottom - form.bottom - 32)).toBeLessThanOrEqual(3);
  });
});
