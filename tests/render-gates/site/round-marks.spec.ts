// Stylesheet injection, no site built: the partials the build appends inside the
// shortcodes layer are appended the same way, and the comments sheet stays
// unlayered, as it is linked. The subscribe script is moss's real bundle against
// a mocked service.
import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';
import { readFocus, focusProblem, readFieldButton, fieldButtonProblem, readMarks, markProblem, hitMisses } from '../../support/form-marks.mjs';

const ASSETS = mossBuildAssets();
const read = (rel: string) => fs.readFileSync(path.join(ASSETS, rel), 'utf8');
const SITE = read('css/site.css');
const PARTIALS = ['forms', 'search', 'country-map'].map((n) => read(`css/site/${n}.css`)).join('\n');
const COMMENTS = read('css/comments.css');
const EMAIL = read('css/email.css');
const SUBSCRIBE_JS = read('js/subscribe.js');

const PALETTE = {
  light: { bg: '#fff', text: '#2c2825', muted: '#716d69', accent: '#2d5a2d', strong: '#8e8b85', surface: '#f4f1ec' },
  dark: { bg: '#101110', text: '#ece9e4', muted: '#a9a59f', accent: '#8aa778', strong: '#8a8680', surface: '#1c1d1b' },
};

const tokens = (scheme: 'light' | 'dark') => {
  const p = PALETTE[scheme];
  return `@layer reset, tokens, base, layout, shortcodes, plugins, themes;
@layer tokens { :root{
  color-scheme:${scheme};
  --moss-reading-size-base:1.125rem; --moss-reading-size:var(--moss-reading-size-base);
  --moss-space-2xs:4px; --moss-space-xs:8px; --moss-space-sm:16px; --moss-space-md:24px; --moss-space-lg:32px;
  --moss-color-surface:${p.surface}; --moss-color-bg:${p.bg}; --moss-color-text:${p.text};
  --moss-color-muted:${p.muted}; --moss-color-text-secondary:${p.muted};
  --moss-color-accent:${p.accent}; --moss-color-ui-accent:${p.accent}; --moss-focus-ring:var(--moss-color-ui-accent);
  --moss-border-light:${p.strong}; --moss-border-medium:${p.strong}; --moss-border-strong:${p.strong};
  --moss-size-xs:0.875rem; --moss-size-sm:1rem; --moss-size-md:1.125rem; --moss-size-lg:1.5rem;
} }
body { background:${p.bg}; color:${p.text}; font:16px/1.5 system-ui, sans-serif; margin:0; padding:16px; }`;
};

const document_ = (scheme: 'light' | 'dark', body: string, extra = '') =>
  `<!doctype html><html><head><meta charset="utf-8"><style>${tokens(scheme)}</style><style>${SITE}\n@layer shortcodes {\n${PARTIALS}\n}</style><style>${COMMENTS}</style><style>@layer plugins {
${EMAIL}
}</style><style>${extra}</style></head><body>${body}</body></html>`;

const ENDPOINT = 'https://service.example.test/subscribe';
const SUBSCRIBE_FORM = `<form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true" data-state="idle" method="post" action="${ENDPOINT}">
  <input type="hidden" name="scope" value="en">
  <input type="email" name="email" class="moss-input" placeholder="you@example.com" required>
  <span class="moss-btn-slot"><button type="submit" class="moss-btn"><span class="moss-btn__label">Subscribe</span><span class="moss-btn__spinner" aria-hidden="true"></span><span class="moss-btn__check" aria-hidden="true"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg></span></button></span>
  <div class="moss-subscribe-status" aria-live="polite"><svg class="moss-subscribe-status__icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7l8 6 8-6M4 7v10a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1V7" fill="none" stroke="currentColor" stroke-width="2"/></svg><span data-copy="check-email" hidden></span><span data-copy="already-subscribed" hidden></span><span data-copy="error" hidden></span></div>
</form>`;

// What a theme or a site might do to the button and the status text.
const VARIANTS: { name: string; css: string }[] = [
  { name: 'plain', css: '' },
  { name: 'hostile button', css: '@layer shortcodes { .moss-btn { min-height: 44px; min-width: 60px; padding: 10px 16px; } }' },
  { name: 'larger status text', css: '.moss-subscribe-form { --moss-subscribe-status-size: 1.4rem; line-height: 1.5; }' },
  // Unlayered theme rules outrank moss's layers; the success geometry must hold against them.
  { name: 'theme button with padding and border', css: '.moss-subscribe-form .moss-btn { padding: 9px 16px; border: 2px solid; border-radius: 0; font: inherit; }' },
  { name: 'theme button with a 44px floor', css: '.moss-btn { min-height: 44px; }' },
  { name: 'theme button with line height 1', css: '.moss-btn { display: inline-flex; align-items: center; min-block-size: 44px; padding: 0 20px; border-radius: 2px; font: 600 1rem/1 sans-serif; }' },
  { name: 'theme button filling its row', css: '.moss-subscribe .moss-btn { padding: 0 1.6rem; height: 100%; border-radius: 0; }' },
];

const CORS = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': '*' };

async function mockService(page: Page, answer: { status: number; body: object }) {
  await page.route(ENDPOINT, (route) =>
    route.request().method() === 'OPTIONS'
      ? route.fulfill({ status: 204, headers: CORS })
      : route.fulfill({ status: answer.status, headers: CORS, contentType: 'application/json', body: JSON.stringify(answer.body) }),
  );
}

async function openSubscribe(page: Page, scheme: 'light' | 'dark', css: string, placement: 'card' | 'footer' = 'card') {
  const card = `<div class="moss-subscribe">${SUBSCRIBE_FORM}</div>`;
  await page.setContent(document_(scheme, placement === 'footer' ? `<footer>${card}</footer>` : `<main>${card}</main>`, css));
  await page.addScriptTag({ content: SUBSCRIBE_JS });
}

const settled = (page: Page, state: string) =>
  page.waitForFunction(
    (s) => {
      const form = document.querySelector('form')!;
      return form.dataset.state === s && form.getAnimations({ subtree: true }).every((a) => !(a instanceof CSSTransition) || a.playState !== 'running');
    },
    state,
    { timeout: 10_000 },
  );

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [390, 1280]) {
    test.describe(`${scheme} ${width}px`, () => {
      test.use({ viewport: { width, height: 800 }, colorScheme: scheme });

      for (const variant of VARIANTS) {
        test(`the success circle is one message line across, ${variant.name}`, async ({ page }, testInfo) => {
          await mockService(page, { status: 200, body: { subscribed: true, confirmationSent: true } });
          await openSubscribe(page, scheme, variant.css);
          await page.locator('input[type="email"]').fill('reader@example.invalid');
          await page.locator('.moss-btn').click();
          await settled(page, 'success');
          await page.locator('form').screenshot({ path: testInfo.outputPath(`success-${variant.name}.png`) });
          expect(markProblem('circle', await page.evaluate(readMarks, 'form'))).toBeNull();
          expect(await page.locator('.moss-btn').evaluate((b) => getComputedStyle(b).borderTopLeftRadius)).toBe('50%');
          const { check, mark } = await page.evaluate(() => ({
            check: document.querySelector('.moss-btn__check svg')!.getBoundingClientRect().width,
            mark: document.querySelector('.moss-btn')!.getBoundingClientRect().width,
          }));
          expect(check / mark, `check ${check}px in a ${mark}px circle`).toBeCloseTo(0.7, 1);
          // 24px of hit area everywhere, 44px under the touch floor (a viewport under 48rem)
          const reach = width < 768 ? 21 : 11;
          expect(await page.evaluate(hitMisses, { form: 'form', reach }), `points ${reach}px from the circle's centre that miss the button`).toBe(0);
        });

        test(`the error icon is square and within the message line, ${variant.name}`, async ({ page }, testInfo) => {
          await mockService(page, { status: 500, body: {} });
          await openSubscribe(page, scheme, variant.css);
          await page.locator('input[type="email"]').fill('reader@example.invalid');
          await page.locator('.moss-btn').click();
          await settled(page, 'error');
          await page.locator('form').screenshot({ path: testInfo.outputPath(`error-${variant.name}.png`) });
          expect(markProblem('icon', await page.evaluate(readMarks, 'form'))).toBeNull();
        });
      }

      test('every round mark moss ships is square', async ({ page }) => {
        await page.setContent(
          document_(
            scheme,
            `<button class="moss-btn"><span class="moss-btn__spinner"></span></button>
<div class="moss-hero" data-slides="2"><label class="moss-hero-pause"><input type="checkbox"><span>Pause</span></label></div>
<div class="moss-scroll-dots"><button id="dot" type="button"></button><button id="dot-now" type="button" aria-current="true"></button></div>
<div class="lightbox"><button class="lightbox-close">&times;</button><button class="lightbox-nav lightbox-prev">&lsaquo;</button><button class="lightbox-nav lightbox-next">&rsaquo;</button></div>
<div class="moss-ambient-video"><button class="moss-ambient-toggle">II</button></div>
<div class="immersive-fs-active"><button class="immersive-fullscreen-btn">x</button></div>
<div class="media-item" data-type="video" style="position:relative;width:200px;height:120px"></div>`,
          ),
        );
        await page.locator('#dot').focus();
        const { found, sizes } = await page.evaluate((marks) => {
          const found = new Set<string>();
          const walk = (rules: CSSRuleList) => {
            for (const rule of Array.from(rules) as CSSStyleRule[]) {
              if ((rule as unknown as CSSGroupingRule).cssRules) walk((rule as unknown as CSSGroupingRule).cssRules);
              if (rule.style?.getPropertyValue('border-radius') === '50%') found.add(rule.selectorText);
            }
          };
          for (const sheet of Array.from(document.styleSheets)) walk(sheet.cssRules);
          const sizes: Record<string, { width: number; height: number }[]> = {};
          for (const [selector, targets] of Object.entries(marks)) {
            sizes[selector] = targets.filter(({ from }) => !from || innerWidth >= from).map(({ at, pseudo }) => {
              const el = document.querySelector(at)!;
              if (pseudo) { const c = getComputedStyle(el, pseudo); return { width: parseFloat(c.width), height: parseFloat(c.height) }; }
              const r = el.getBoundingClientRect();
              return { width: r.width, height: r.height };
            });
          }
          return { found: [...found], sizes };
        }, ROUND_MARKS);
        const unlisted = found.filter((selector) => !(selector in ROUND_MARKS) && !COVERED_ELSEWHERE.includes(selector));
        expect(unlisted, 'a rule gives a new mark border-radius 50%: add it to ROUND_MARKS with markup').toEqual([]);
        for (const [selector, boxes] of Object.entries(sizes)) {
          for (const s of boxes) {
            expect(s.width, `${selector} has a size`).toBeGreaterThan(0);
            expect(Math.abs(s.width - s.height), `${selector} is ${s.width} x ${s.height}`).toBeLessThanOrEqual(1);
          }
        }
      });

      for (const placement of ['card', 'footer'] as const) {
        // An in-page card stacks the field above the button under 32rem; there only the heights must agree.
        const stacked = placement === 'card' && width < 512;
        test(`the subscribe field and button are one height${stacked ? '' : ' on one text line'}, ${placement}`, async ({ page }) => {
          await openSubscribe(page, scheme, '', placement);
          const m = await page.evaluate(readFieldButton, 'form');
          expect(fieldButtonProblem(stacked ? { ...m, offset: 0 } : m)).toBeNull();
        });
      }

      test('each text field shows exactly one focus treatment, on its own edge, at 3:1', async ({ page }) => {
        await page.setContent(
          document_(
            scheme,
            `<main style="display:grid;gap:16px;max-width:360px">
<input id="underline" class="moss-input" aria-label="a">
<form class="moss-subscribe-form" data-moss-hosted="true"><input id="subscribe" class="moss-input" aria-label="b"></form>
<form class="moss-form"><textarea id="boxed" class="moss-input moss-input--boxed" aria-label="c"></textarea></form>
<form class="comment-form"><textarea id="comment-text" aria-label="d"></textarea><input id="comment-field" class="comment-field" aria-label="e"></form>
<div class="moss-country-search"><input id="country" class="moss-input--boxed" type="text" aria-label="f"></div>
<div id="search-panel" class="moss-search__panel"><div class="moss-search__field"><input id="search" class="moss-search__input" aria-label="g"></div></div>
</main>`,
          ),
        );
        const found = await page.evaluate(() => {
          const out = new Set<string>();
          const walk = (rules: CSSRuleList) => {
            for (const rule of Array.from(rules) as CSSStyleRule[]) {
              if ((rule as unknown as CSSGroupingRule).cssRules) walk((rule as unknown as CSSGroupingRule).cssRules);
              if (rule.selectorText && /:focus/.test(rule.selectorText) && /input|textarea|comment-field/.test(rule.selectorText)) out.add(rule.selectorText);
            }
          };
          for (const sheet of Array.from(document.styleSheets)) walk(sheet.cssRules);
          return [...out];
        });
        const unlisted = found.filter((selector) => !(selector in FOCUS_RULES) && !CHECKBOX_FOCUS.includes(selector));
        expect(unlisted, 'a focus rule on a text field: add it to FOCUS_RULES with the fields it styles').toEqual([]);
        for (const [selector, ids] of Object.entries(FOCUS_RULES)) {
          expect(found, `${selector} no longer exists; drop it from FOCUS_RULES`).toContain(selector);
          for (const id of ids) {
            const problem = focusProblem(await page.evaluate(readFocus, { field: `#${id}`, drawn: id === 'search' ? '#search-panel' : undefined }));
            expect(problem, `${id} (${selector})`).toBeNull();
          }
        }
      });
    });
  }
}

// Every rule that gives a mark `border-radius: 50%`, keyed by its selector as the
// stylesheet writes it, with what to measure in the markup above (`from`: the narrowest viewport that shows it).
const ROUND_MARKS: Record<string, { at: string; pseudo?: string; from?: number }[]> = {
  '.moss-btn__spinner::after': [{ at: '.moss-btn__spinner', pseudo: '::after' }],
  '.moss-hero-pause': [{ at: '.moss-hero-pause' }],
  '.moss-scroll-dots button::before': [{ at: '#dot', pseudo: '::before' }, { at: '#dot-now', pseudo: '::before' }],
  '.moss-scroll-dots button:focus-visible': [{ at: '#dot' }],
  '.media-item[data-type="video"]::before': [{ at: '.media-item', pseudo: '::before' }],
  '.lightbox-close': [{ at: '.lightbox-close' }],
  '.lightbox-nav': [{ at: '.lightbox-prev', from: 768 }, { at: '.lightbox-next', from: 768 }],
  '.moss-ambient-toggle': [{ at: '.moss-ambient-toggle' }],
  '.immersive-fs-active .immersive-fullscreen-btn': [{ at: '.immersive-fullscreen-btn' }],
};
// Measured above against the real script instead.
const COVERED_ELSEWHERE = ['.moss-subscribe-form[data-state="success"] .moss-btn-slot .moss-btn'];

// Every focus rule on a text field, with the fields (ids in the markup above) it styles.
const FOCUS_RULES: Record<string, string[]> = {
  '.moss-input:focus, .comment-field:focus, .comment-form textarea:focus': ['underline', 'subscribe', 'comment-text', 'comment-field'],
  '.moss-input--boxed.moss-input--boxed:focus': ['boxed', 'country'],
  '.moss-search__panel:has(.moss-search__input:focus)': ['search'],
};
// Focus rules whose selector names an input that is a checkbox, not a text field.
const CHECKBOX_FOCUS = [
  '.moss-hero[data-slides]:has(.moss-hero-pause input:checked) :is(.moss-hero-slide, .moss-hero-caption), .moss-hero[data-slides]:has(:focus-visible:not(.moss-hero-pause input)) :is(.moss-hero-slide, .moss-hero-caption)',
  '.moss-hero-pause:is(:hover, :has(input:focus-visible))',
  '.moss-hero-pause:has(input:focus-visible)',
  '.moss-form__choice input:focus-visible',
];
