/**
 * Render gate: the reading scale is monotonic — everywhere.
 *
 * For every combination of the four reader font-scale steps, the two viewport
 * sides of the 48rem breakpoint, and `{zh-Hant, en}`, the computed font sizes
 * of
 *
 *   article title > h1 > h2 > h3 > h4 > body > h5 > h6 > caption
 *
 * must be strictly descending. Nothing else asserted heading sizes anywhere
 * under tests/render-gates/, and a CJK typography audit of a real site found
 * three inversions living in that gap: `h4` below CJK body at every viewport, the
 * mobile title below a `#` heading, and `###` sinking below body as soon as the
 * reader pressed Aa once — each one a collision between two of the three
 * incompatible bases the reading scale used to be written on.
 *
 * Only a real engine can answer this: the sizes are `calc()` over custom
 * properties resolved against a media query and a `lang` attribute, and jsdom
 * computes none of that.
 *
 * The `lang` half is a runtime attribute swap rather than a second site: the
 * only thing `lang` drives here is site.css's `html[lang^="zh"]` block, so the
 * swap exercises the real selector.
 *
 * Run via:
 *   npx playwright test -c playwright/reading-scale-order.config.ts
 */

import { test, expect } from "@playwright/test";

/** Descending order is the assertion, so this array IS the contract. */
const LEVELS = ["title", "h1", "h2", "h3", "h4", "body", "h5", "h6", "caption"] as const;
type Level = (typeof LEVELS)[number];

// One page carries the whole ladder: moss injects the masthead h1 from the
// frontmatter title above the body's own `# H1`, so `article h1` alone would
// match the masthead. See READING_SCALE_ORDER_GATE in
// tests/e2e/helpers/gate-sites.ts.
const PAGE = "ladder/";

const SELECTORS: Record<Level, string> = {
  title: ".moss-article-title",
  h1: "article h1:not(.moss-article-title)",
  h2: "article h2",
  h3: "article h3",
  h4: "article h4",
  body: "body",
  // h5/h6 sit BELOW body in the ramp (0.889 and 0.778 of the reading size), so
  // they belong after it in LEVELS. They are asserted because the mobile block
  // this gate replaced existed for exactly one reason: left alone, h4 and h5
  // both landed on 16px on phones and the two adjacent levels became
  // indistinguishable. Without these two rows the gate cannot see that return.
  h5: "article h5",
  h6: "article h6",
  caption: "article figure figcaption",
};

const VIEWPORTS = [
  { name: "desktop", size: { width: 1280, height: 900 } },
  // Below the 48rem (768px) breakpoint.
  { name: "mobile", size: { width: 390, height: 844 } },
];

// The reader's Aa control writes this key and site JS applies `scale-<value>`
// to <html> before first paint (crates/moss-build/src/js-src/site/theme.ts).
// "" is the default step.
const SCALES = ["", "small", "large", "xlarge"];

const LANGS = ["zh-Hant", "en"];

for (const lang of LANGS) {
  for (const viewport of VIEWPORTS) {
    for (const scale of SCALES) {
      const step = scale || "default";
      test(`${lang} · ${viewport.name} · Aa ${step}: sizes descend`, async ({
        page,
      }) => {
        await page.setViewportSize(viewport.size);
        await page.addInitScript((value) => {
          localStorage.setItem("moss-font-scale", value);
        }, scale);

        await page.goto(PAGE, { waitUntil: "domcontentloaded" });
        const sizes = await page.evaluate(
          ({ selectors, lang }) => {
            document.documentElement.lang = lang;
            const out: Record<string, number | null> = {};
            for (const [level, selector] of Object.entries(selectors)) {
              const el = document.querySelector(selector as string);
              out[level] = el ? parseFloat(getComputedStyle(el).fontSize) : null;
            }
            return out;
          },
          { selectors: SELECTORS, lang },
        );
        for (const level of LEVELS) {
          expect(
            sizes[level],
            `${level} (${SELECTORS[level]}) must exist on ${PAGE}`,
          ).not.toBeNull();
        }

        // One message carrying the whole ladder: a single inversion is only
        // readable next to the levels around it.
        const ladder = LEVELS.map((l) => `${l}=${sizes[l]}px`).join(" > ");
        for (let i = 0; i < LEVELS.length - 1; i++) {
          const above = LEVELS[i];
          const below = LEVELS[i + 1];
          expect(
            sizes[above]!,
            `${above} must be larger than ${below} — measured ${ladder}`,
          ).toBeGreaterThan(sizes[below]!);
        }
      });
    }
  }
}
