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
// vertical body can.
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

// Mirrors vertical-measure.spec.ts's token block: production values for the
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

// After a sign-up the input fades and the button becomes a check circle in its
// slot; the confirmation sits on the circle's row, in the input's place, so the
// two read as one line.
// The longest confirmation the subscribe script ships is the German one.
const COPY = { 'zh-hans': '请在邮箱中确认订阅', en: 'Check your email to confirm.', de: 'Bitte bestätigen Sie das Abonnement in Ihrer E-Mail.' };

const hostedForm = (lang) => {
  const zh = lang.startsWith('zh');
  return '<div class="moss-subscribe" data-state="idle">'
    + '<form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true" data-state="idle" method="post" action="#">'
    + `<input type="email" name="email" class="moss-input" placeholder="${zh ? '邮箱' : 'your@email.com'}" required>`
    + '<span class="moss-btn-slot"><button type="submit" class="moss-btn">'
    + `<span class="moss-btn__label">${zh ? '订阅' : 'Subscribe'}</span>`
    + '<span class="moss-btn__spinner" aria-hidden="true"></span>'
    + '<span class="moss-btn__check" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7" fill="none" stroke="currentColor" stroke-width="2.5"/></svg></span>'
    + '</button></span>'
    + '<div class="moss-subscribe-status" aria-live="polite">'
    + '<svg class="moss-subscribe-status__icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7l8 6 8-6" fill="none" stroke="currentColor" stroke-width="2"/></svg>'
    + `<span data-copy="check-email">${COPY[lang]}</span>`
    + '</div></form></div>';
};

const donePage = (lang, { inFooter = true, vertical = false } = {}) => `<!doctype html><html lang="${lang}"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>${TOKENS}</style><style>${CSS}</style>${vertical ? `<style>${VERTICAL}</style>` : ''}<style>@layer plugins{${EMAIL}}</style>
</head><body${vertical ? ' data-typesetting="vertical"' : ''}><main id="main-content"><article class="container"><p>Body.</p>${inFooter ? '' : hostedForm(lang)}</article></main>
<footer class="container"><p class="footer-default"><a href="/rss.xml">RSS</a></p>${inFooter ? hostedForm(lang) : ''}</footer>
</body></html>`;

/** Submit as a reader does: focus the field, lock the slot the way the subscribe script does, succeed, settle. */
async function succeed(page) {
  await page.locator('.moss-subscribe-form .moss-input').focus();
  await page.evaluate(() => {
    const form = document.querySelector('.moss-subscribe-form');
    const slot = form.querySelector('.moss-btn-slot');
    slot.style.width = `${form.querySelector('.moss-btn').offsetWidth}px`;
    form.dataset.state = 'success';
  });
  await page.locator('.moss-subscribe-status').evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
}

const middle = (r) => (r.top + r.bottom) / 2;
const centre = (r) => (r.left + r.right) / 2;

test.describe('subscribe confirmation shares the check circle row', () => {
  for (const lang of ['zh-hans', 'en']) {
    for (const width of [390, 1280]) {
      test(`${lang} at ${width}px: the circle and the message read as one row`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await page.setContent(donePage(lang));
        await succeed(page);
        const form = await rect(page, '.moss-subscribe-form');
        const circle = await rect(page, '.moss-subscribe-form .moss-btn');
        const row = await rect(page, '.moss-subscribe-status');
        const message = await rect(page, '.moss-subscribe-status [data-copy="check-email"]');
        expect(Math.abs(middle(circle) - middle(row)), 'same row').toBeLessThanOrEqual(3);
        expect(Math.abs(row.left - form.left), 'the row starts where the field did').toBeLessThanOrEqual(2);
        expect(message.right, 'the message ends before the circle').toBeLessThanOrEqual(circle.left);
        // The field fades but keeps the focus, so typing again can return the form to idle.
        expect(await page.evaluate(() => document.activeElement?.classList.contains('moss-input'))).toBe(true);
        await page.keyboard.type('a');
        expect(await page.locator('.moss-subscribe-form .moss-input').inputValue()).toBe('a');
      });
    }
  }

  for (const [lang, width, textSize] of [['zh-hans', 390], ['en', 320], ['de', 320, '20px']]) {
    // At a larger browser text size the German copy wraps to three lines, taller than the row's minimum height.
    test(`${lang} stacked card at ${width}px${textSize ? `, ${textSize} browser text` : ''}: the message sits just above the circle, centred with it`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.setContent(donePage(lang, { inFooter: false }));
      if (textSize) await page.addStyleTag({ content: `html { font-size: ${textSize}; }` });
      await succeed(page);
      const circle = await rect(page, '.moss-subscribe-form .moss-btn');
      const icon = await rect(page, '.moss-subscribe-status__icon');
      const message = await rect(page, '.moss-subscribe-status [data-copy="check-email"]');
      const box = await rect(page, '.moss-subscribe-status');
      expect(message.bottom, 'the wrapped message stays inside its row').toBeLessThanOrEqual(box.bottom + 0.5);
      expect(message.bottom, 'above the circle').toBeLessThanOrEqual(circle.top + 1);
      expect(circle.top - message.bottom, 'and close to it').toBeLessThanOrEqual(24);
      // The visible line, icon to last glyph, not the status box: under `stretch` the box spans the card while the line sits at its start.
      expect(Math.abs(centre(circle) - (icon.left + message.right) / 2), 'centred together').toBeLessThanOrEqual(3);
    });
  }

  test('under vertical-rl the message and the circle share one column', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.setContent(donePage('zh-hans', { vertical: true }));
    await succeed(page);
    const circle = await rect(page, '.moss-subscribe-form .moss-btn');
    const row = await rect(page, '.moss-subscribe-status');
    expect(Math.abs(centre(circle) - centre(row)), 'same column').toBeLessThanOrEqual(3);
    expect(row.bottom, 'the message comes first down the column').toBeLessThanOrEqual(circle.top);
  });
});
