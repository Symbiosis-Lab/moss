// Tap targets under 44 by 44 CSS px at phone width — the WCAG 2.5.8 AA
// floor. Measured on a real build at 390px: nav links rendered 39 by 24.5px
// with no block padding at all; the hamburger button is 32px; footer links
// (plain text, no padding) were 20 to 27px tall; the subscribe input and
// button were one line of body text — 30px at the default reading size.
//
// The two nav icon controls (language switch, theme toggle) are covered
// elsewhere: develop's header-controls audit (5f253fd1) gives every header
// control its own 44px hit area and its own render gate
// (playwright/header-hit-areas.config.ts) — this file does not duplicate
// that work, and the fixture below still renders the icon cluster (for a
// realistic `.nav-icons` layout) without asserting on it.
//
// Nav links and footer links grow the HIT AREA only — an absolutely
// positioned, invisible `::after` around the real control — never the
// control's own box: a first attempt grew `.nav-links a` itself
// (`min-block-size` + flex), which took part in layout and, at 390px, grew
// an eight-link wrapped nav from 109 to 163px tall and moved an active
// link's underline ~23px below its label. `position: relative` on the
// control changes only its positioning context, so the control's rendered
// size, the nav bar's own height, and an active link's `border-block-end`
// position are all unaffected. The subscribe input/button instead grow
// their own `block-size` — no neighbor to overlap, and no independent "row
// height must not change" constraint on that form — so that fix is
// unchanged from the first round.
//
// Two real scratch sites (footer.md, and the generated fallback) are built
// and served via buildScratchSite, the same idiom as hero-tone.spec.ts: the
// footer fix has to reach whatever moss actually emits for a real footer.md
// (`<ul><li><a>` for a list, a plain inline `<a>` in a sentence — no class on
// either), not a hand-built lookalike carrying `.footer-link`. Nav and
// subscribe stay stylesheet-injection (`page.setContent`): their markup
// carries no analogous "a hand-built fixture would miss this" risk.
//
// Below, every assertion checks the MEASURED extended hit area against the
// 44px floor on both axes, not just that a `::after` rule exists — a cap
// halved against the wrong gap can produce a rule that exists and still
// falls short. One exception, on the inline axis only: a link running
// inside a sentence (a footer.md byline, or the generated fallback's
// middot-joined `<p class="footer-default">` list) keeps its own text
// width rather than reaching 44px, because widening it would spill the
// clickable area into the words or the neighbor link beside it. Nav links
// and a footer markdown list get no such exception — both are short
// discrete labels with room on either side, not prose.
import { test, expect, Page, Locator } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';
import { gatePort } from '../../../playwright/gate-ports';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css/site.css'), 'utf8');

const TOKENS = `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
  --moss-reading-size-base:1.125rem;
  --moss-reading-size:var(--moss-reading-size-base);
  --moss-content-width:calc(42 * var(--moss-reading-size));
  --moss-site-max-width:1200px;
  --moss-container-padding:clamp(1rem, 5vw, 2rem);
  --moss-space-2xs:4px; --moss-space-xs:8px; --moss-space-sm:16px;
  --moss-space-md:24px; --moss-space-lg:32px; --moss-space-xl:48px; --moss-space-2xl:64px;
  --moss-color-surface:#f4f1ec; --moss-color-bg:#fff; --moss-color-text:#2c2825;
  --moss-color-muted:#716d69; --moss-color-text-secondary:#5d5853;
  --moss-color-accent:#2d5a2d; --moss-color-ui-accent:var(--moss-color-accent);
  --moss-border-light:#ddd; --moss-border-medium:#bbb; --moss-border-strong:#8e8b85;
  --moss-size-xs:0.875rem; --moss-size-sm:1rem; --moss-size-md:1.125rem; --moss-size-lg:1.5rem;
  --moss-font-heading-weight:480;
} }`;

// Real emitted shapes (nav.rs `generate_navigation`), not a lookalike.
function themeToggleBtn() {
  return `<button class="nav-theme-btn" type="button" aria-label="Toggle theme"><svg class="theme-toggle-icon" width="1em" height="1em" viewBox="0 0 32 32"><circle cx="16" cy="16" r="9.34"/></svg></button>`;
}

function langToggle() {
  return `<div class="nav-lang-toggle" aria-label="Language">`
    + `<span class="nav-lang-current">EN</span> / `
    + `<a href="/zh/" class="nav-lang-link" hreflang="zh">中</a>`
    + `</div>`;
}

/** `activeIndex` marks one link `class="active"`, matching nav.rs when the current page is that link's target. */
function navLinks(labels: string[], { open = false, activeIndex = -1 }: { open?: boolean; activeIndex?: number } = {}) {
  const links = labels
    .map((label, i) => `<a href="/${label.toLowerCase()}"${i === activeIndex ? ' class="active"' : ''}>${label}</a>`)
    .join('');
  return `<div class="nav-links${open ? ' mobile-open' : ''}" id="nav-links">${links}</div>`;
}

function hamburger() {
  return `<button class="mobile-menu-button" aria-label="Toggle menu" aria-expanded="false" aria-controls="nav-links"><svg viewBox="0 0 24 24" width="24" height="24"><line x1="3" y1="12" x2="21" y2="12"/></svg></button>`;
}

function subscribeForm() {
  return `<div class="moss-subscribe"><form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true" method="post" action="#">`
    + `<input type="hidden" name="scope" value="en">`
    + `<input type="email" name="email" class="moss-input" placeholder="you@example.com" required>`
    + `<span class="moss-btn-slot"><button type="submit" class="moss-btn">`
    + `<span class="moss-btn__label">Subscribe</span>`
    + `<span class="moss-btn__spinner" aria-hidden="true"></span>`
    + `<span class="moss-btn__check" aria-hidden="true"></span>`
    + `</button></span></form></div>`;
}

function page_({ labels, navOpen = false, activeIndex = -1 }: { labels: string[]; navOpen?: boolean; activeIndex?: number }) {
  return `<!doctype html><html><head><style>${TOKENS}</style><style>${CSS}</style></head>
<body>
<header>
  <nav class="main-nav container">
    <div class="nav-content">
      <div class="nav-left"><a href="/">Site name</a></div>
      <div class="nav-right">
        ${hamburger()}
        ${navLinks(labels, { open: navOpen, activeIndex })}
        <div class="nav-icons">${langToggle()}${themeToggleBtn()}</div>
      </div>
    </div>
  </nav>
</header>
<main id="main-content"><article class="container"><p>Body copy.</p></article></main>
<footer class="container">
  ${subscribeForm()}
</footer>
</body></html>`;
}

const FOUR_LABELS = ['About', 'Blog', 'Work', 'Contact'];
const EIGHT_LABELS = ['About', 'Blog', 'Work', 'Writing', 'Projects', 'Photos', 'Notes', 'Contact'];

const box = (page: Page, selector: string, nth = 0) => page.locator(selector).nth(nth).boundingBox();

const MIN = 44;

/**
 * The `::after` hit-area's rect, reconstructed from the real element's own
 * `getBoundingClientRect()` plus the pseudo-element's computed physical
 * `top`/`right`/`bottom`/`left` (browsers resolve `inset-block`/`inset-inline`
 * to these on read, regardless of which form the source CSS used). A
 * pseudo-element carries no box of its own to query directly.
 */
async function extendedRect(locator: Locator) {
  return locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el, '::after');
    const top = parseFloat(cs.top) || 0;
    const right = parseFloat(cs.right) || 0;
    const bottom = parseFloat(cs.bottom) || 0;
    const left = parseFloat(cs.left) || 0;
    return {
      top: r.top + top,
      left: r.left + left,
      right: r.right - right,
      bottom: r.bottom - bottom,
      position: cs.position,
    };
  });
}

test.describe('nav at phone width (390px)', () => {
  test('four links: every hit area clears 44×44, nav bar height unchanged from the original box', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.setContent(page_({ labels: FOUR_LABELS }));

    const links = page.locator('.nav-links a');
    const count = await links.count();
    for (let i = 0; i < count; i++) {
      const link = links.nth(i);
      const real = await link.boundingBox();
      const ext = await extendedRect(link);
      expect(ext.position).toBe('absolute');
      // The real box is untouched — still under the 44px floor by itself —
      // which is what guarantees the row around it never grew.
      expect(real!.height).toBeLessThan(MIN);
      // Every link, not a sample: nav labels are short discrete words, not
      // prose, so none of them qualify for the "inline in a sentence" width
      // exception the footer's running-text link gets below.
      expect(ext.bottom - ext.top).toBeGreaterThanOrEqual(MIN);
      expect(ext.right - ext.left).toBeGreaterThanOrEqual(MIN);
    }
    // A single-row nav never renders the wider row-gap this fix pays for
    // wrapping: the row itself must not have grown from the original box.
    const navContentHeight = await page.locator('.nav-content').evaluate((el) => el.getBoundingClientRect().height);
    expect(navContentHeight).toBeLessThan(120);
  });

  test('eight links wrapping: every hit area still clears 44×44, and no two overlap', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.setContent(page_({ labels: EIGHT_LABELS }));

    const links = page.locator('.nav-links a');
    const count = await links.count();
    expect(count).toBe(8);
    const rects = await Promise.all(
      Array.from({ length: count }, (_, i) => extendedRect(links.nth(i))),
    );
    for (const r of rects) {
      expect(r.bottom - r.top).toBeGreaterThanOrEqual(MIN);
      expect(r.right - r.left).toBeGreaterThanOrEqual(MIN);
    }
    // Row boundaries by x-position, not by rounding each link's own top
    // edge: flex-wrap lays links left to right until one wraps back to the
    // start edge.
    const rowBreaks = [0];
    for (let i = 1; i < rects.length; i++) {
      if (rects[i].left < rects[i - 1].left - 0.5) rowBreaks.push(i);
    }
    const rows = rowBreaks.map((start, i) => rects.slice(start, rowBreaks[i + 1] ?? rects.length));
    // The honest cost this fix pays: wrapping now takes a taller row-gap
    // (32px) to reach 44px on the block axis, so eight short labels wrap to
    // two rows here rather than the pre-fix layout's own row count — but
    // not a third: the column-gap widened just enough to clear 44px on the
    // inline axis (19px) stops short of the width that would force a third.
    expect(rows.length).toBe(2);
    for (let i = 1; i < rows.length; i++) {
      const prevBottom = Math.max(...rows[i - 1].map((r) => r.bottom));
      const thisTop = Math.min(...rows[i].map((r) => r.top));
      expect(thisTop).toBeGreaterThanOrEqual(prevBottom - 0.5);
    }
  });

  test('an active link keeps its underline where the base commit puts it', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.setContent(page_({ labels: FOUR_LABELS, activeIndex: 1 }));

    const active = page.locator('.nav-links a.active');
    const plain = page.locator('.nav-links a').first();
    const [activeBox, plainBox, borderWidth, paddingEnd] = await Promise.all([
      active.boundingBox(),
      plain.boundingBox(),
      active.evaluate((el) => getComputedStyle(el).borderBlockEndWidth),
      active.evaluate((el) => getComputedStyle(el).paddingBlockEnd),
    ]);
    // `.main-nav a.active` has always been its own 2px border-block-end plus
    // 2px padding-block-end taller than a plain link — that is the
    // underline's own thickness and clearance, present on the base commit
    // too. The fix must add nothing beyond that: the active box is exactly
    // the plain box plus those two, not plus 44px worth of min-block-size.
    const ownExtra = parseFloat(borderWidth) + parseFloat(paddingEnd);
    expect(ownExtra).toBeCloseTo(4, 0);
    expect(activeBox!.height).toBeCloseTo(plainBox!.height + ownExtra, 0);
  });

  // The theme toggle and language switch are NOT asserted here: develop's
  // header-controls audit (5f253fd1, playwright/header-hit-areas.config.ts)
  // gives every header control — theme toggle, language toggle, search,
  // the nav island's sections button, the reading-size control, the mobile
  // menu button — its own 44px hit area and its own gate. This file covers
  // only what that work does not: nav text links, footer links, and the
  // subscribe input/button.

  test('a click inside the grown hit area, outside the visible box, still hits the link', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.setContent(page_({ labels: FOUR_LABELS }));

    // A middle link: the first and last skip extension on their one
    // outward-facing side (see the CSS comment above the `::after` rule).
    const link = page.locator('.nav-links a').nth(1);
    const real = (await link.boundingBox())!;
    // 2px above the real box's top edge — inside the ::after, outside the
    // anchor's own painted box.
    const x = real.x + real.width / 2;
    const y = real.y - 2;
    const hit = await page.evaluate(([hx, hy]) => document.elementFromPoint(hx, hy)?.tagName, [x, y]);
    expect(hit).toBe('A');
  });
});

test.describe('nav at desktop width (1280px) is unchanged', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.setContent(page_({ labels: FOUR_LABELS }));
  });

  test('nav links stay their original short box', async ({ page }) => {
    const first = await box(page, '.nav-links a', 0);
    expect(first?.height ?? 0).toBeLessThan(MIN);
  });
});

test.describe('a coarse pointer gets the nav-link fix even above 48rem (tablet landscape)', () => {
  test.use({ hasTouch: true, viewport: { width: 1024, height: 768 } });

  test('a nav link still gains a hit-area extension at 1024px on a touch device', async ({ page }) => {
    expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(true);
    await page.setContent(page_({ labels: FOUR_LABELS }));
    const position = await page.locator('.nav-links a').first().evaluate((el) => getComputedStyle(el).position);
    expect(position).toBe('relative');
  });
});

test.describe('subscribe controls', () => {
  test('clear 44px tall at phone width', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.setContent(page_({ labels: FOUR_LABELS }));
    const input = await box(page, '.moss-subscribe-form .moss-input');
    const btn = await box(page, '.moss-subscribe-form .moss-btn');
    expect(input?.height ?? 0).toBeGreaterThanOrEqual(MIN);
    expect(btn?.height ?? 0).toBeGreaterThanOrEqual(MIN);
  });

  test('stay one line of text tall on desktop with a mouse at 1280px', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.setContent(page_({ labels: FOUR_LABELS }));
    const btn = await box(page, '.moss-subscribe-form .moss-btn');
    expect(btn?.height ?? 0).toBeLessThan(MIN);
  });
});

test.describe('subscribe on a coarse-pointer tablet (1024px)', () => {
  test.use({ hasTouch: true, viewport: { width: 1024, height: 768 } });

  test('clears 44px tall', async ({ page }) => {
    await page.setContent(page_({ labels: FOUR_LABELS }));
    const btn = await box(page, '.moss-subscribe-form .moss-btn');
    expect(btn?.height ?? 0).toBeGreaterThanOrEqual(MIN);
  });
});

// ── Footer: real footer.md, and the generated fallback ──

const FOOTER_MD_BASE = `http://localhost:${gatePort('touch-targets:footer-md')}/`;
const FOOTER_FALLBACK_BASE = `http://localhost:${gatePort('touch-targets:footer-fallback')}/`;

test.describe('footer built from a real footer.md', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(FOOTER_MD_BASE);
  });

  test('a markdown-list link clears 44×44 without growing its own box', async ({ page }) => {
    const link = page.locator('footer.container li a').first();
    const real = await link.boundingBox();
    const ext = await extendedRect(link);
    expect(ext.position).toBe('absolute');
    expect(real!.height).toBeLessThan(MIN);
    // A list item has no horizontal neighbor, so both axes extend: 16px
    // each side, block and inline.
    expect(ext.bottom - ext.top).toBeGreaterThanOrEqual(MIN);
    expect(ext.right - ext.left).toBeGreaterThanOrEqual(MIN);
  });

  test('a sentence link stays inline, clears 44px tall, and keeps its own text width', async ({ page }) => {
    const link = page.locator('footer.container p a').first();
    const display = await link.evaluate((el) => getComputedStyle(el).display);
    expect(display).toBe('inline');
    const real = await link.boundingBox();
    const ext = await extendedRect(link);
    expect(ext.bottom - ext.top).toBeGreaterThanOrEqual(MIN);
    // The width exception: a link running inside a sentence must not widen
    // past its own text, or the reach would spill into the surrounding
    // words and make prose clickable where it isn't. This is the one shape
    // in this file where a hit area is allowed to stay under 44px wide.
    expect(ext.right - ext.left).toBeCloseTo(real!.width, 0);
  });

  test('adjacent list links do not overlap', async ({ page }) => {
    const links = page.locator('footer.container li a');
    const [r0, r1] = await Promise.all([extendedRect(links.nth(0)), extendedRect(links.nth(1))]);
    expect(r1.top).toBeGreaterThanOrEqual(r0.bottom - 0.5);
  });

  test('a click inside the grown hit area, outside the visible box, still hits the link', async ({ page }) => {
    const link = page.locator('footer.container li a').first();
    const real = (await link.boundingBox())!;
    const x = real.x + real.width / 2;
    const y = real.y + real.height + 2; // 2px below the real box, inside the ::after
    const hit = await page.evaluate(([hx, hy]) => document.elementFromPoint(hx, hy)?.tagName, [x, y]);
    expect(hit).toBe('A');
  });
});

test.describe('the generated fallback footer (no footer.md)', () => {
  test('a .footer-link clears 44px tall, via the same general rule', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(FOOTER_FALLBACK_BASE);
    const link = page.locator('footer.container .footer-link').first();
    const real = await link.boundingBox();
    const ext = await extendedRect(link);
    expect(ext.position).toBe('absolute');
    expect(ext.bottom - ext.top).toBeGreaterThanOrEqual(MIN);
    // The generated fallback list is `<p class="footer-default">`, links
    // joined by " · " — the same inline-text shape as a footer.md sentence
    // link above, not a `<ul><li>` list, so `footer.container li a`'s own
    // inline-axis extension does not reach it. Its width stays real text
    // width for the same reason a sentence link's does: widening would
    // spill into its middot-adjacent neighbor.
    expect(ext.right - ext.left).toBeCloseTo(real!.width, 0);
  });
});
