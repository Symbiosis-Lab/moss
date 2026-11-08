/**
 * WCAG gate for the anchored advisory panel, composited over the WHOLE backdrop
 * space — not just over moss's own page colour.
 *
 * Why this file exists next to severity-contrast.test.ts rather than inside it:
 *
 * 1. That gate composites over `--moss-color-bg`, i.e. it assumes the thing
 *    behind the glass is a moss surface. Behind the preview chrome it is the
 *    author's website: any colour at all. A floor that only holds over cream is
 *    not a floor.
 * 2. That gate hand-copies `rgba(255,255,255,0.08)` as "the real dark glass
 *    backing", but the panel reads the `--moss-chrome-*` family (dark pole
 *    0.14). A gate that restates its inputs drifts away from the CSS silently. Everything here is PARSED
 *    from tokens.css, so a token edit either keeps the gate green or fails it.
 *
 * The panel is a READING surface — three type registers, wrapping sentences,
 * three locales — so it is held to SC 1.4.3 (4.5:1) for text, not to the 3:1
 * that governs a glyph-bearing control. A mark on an unknown backdrop is exempt
 * on the grounds that a mark has
 * "a persistent twin"; a sentence has no twin.
 *
 * The load-bearing assertion is the SPREAD, not the floor. Contrast that
 * depends on what the author put behind the panel is contrast that is not
 * designed. See the panel token block in tokens.css.
 */

import { describe, it, expect } from "vitest";
import fs from "node:fs";
import path from "node:path";

type RGB = [number, number, number];
type RGBA = [number, number, number, number];

const TOKENS_CSS = fs.readFileSync(
  path.resolve(__dirname, "../shared/tokens.css"),
  "utf-8",
);

/** The declarations of one top-level block, by exact selector text. */
function block(selector: string): string {
  const at = TOKENS_CSS.indexOf(`\n${selector} {`);
  if (at === -1) throw new Error(`no block for selector ${selector}`);
  const end = TOKENS_CSS.indexOf("\n}", at);
  return TOKENS_CSS.slice(at, end);
}

/** Read one custom property out of a block, as written. */
function token(selector: string, name: string): string {
  const m = new RegExp(`${name}\\s*:\\s*([^;]+);`).exec(block(selector));
  if (!m) throw new Error(`${selector} does not define ${name}`);
  return m[1].trim();
}

function parseColor(css: string): RGBA {
  const hex = /^#([\da-f]{6})$/i.exec(css);
  if (hex) {
    const h = hex[1];
    return [
      parseInt(h.slice(0, 2), 16),
      parseInt(h.slice(2, 4), 16),
      parseInt(h.slice(4, 6), 16),
      1,
    ];
  }
  const rgba = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)$/i.exec(css);
  if (!rgba) throw new Error(`cannot parse colour "${css}"`);
  return [+rgba[1], +rgba[2], +rgba[3], rgba[4] === undefined ? 1 : +rgba[4]];
}

/** Alpha-composite an overlay onto an opaque base. */
function composite(overlay: RGBA, base: RGB): RGB {
  const [r, g, b, a] = overlay;
  return [
    Math.round(r * a + base[0] * (1 - a)),
    Math.round(g * a + base[1] * (1 - a)),
    Math.round(b * a + base[2] * (1 - a)),
  ];
}

/** WCAG 2.1 relative luminance. */
function luminance([r, g, b]: RGB): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(fg: RGB, bg: RGB): number {
  const [a, b] = [luminance(fg), luminance(bg)];
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/**
 * The backdrop space: whatever the author's site paints behind the chrome.
 * The two poles are the real bounds; the heroes are there so a regression that
 * only shows on a photograph still trips the gate.
 */
const BACKDROPS: Array<[string, RGB]> = [
  ["white page", [255, 255, 255]],
  ["black page", [0, 0, 0]],
  ["mid grey", [128, 128, 128]],
  ["warm photographic hero", [196, 150, 96]],
  ["dark blue hero", [28, 38, 58]],
];

const POLES = ['[data-chrome-theme="light"]', '[data-chrome-theme="dark"]'] as const;

/** Text registers the panel actually renders, with the WCAG rule each is held to. */
const TEXT_TOKENS = [
  "--moss-chrome-panel-fg",            // .moss-status-line, .moss-notice-reason (13px/500)
  "--moss-chrome-panel-fg-secondary",  // .moss-notice-more, filenames (11px)
  "--moss-chrome-panel-fg-muted",      // .moss-status-detail, .moss-status-condition (11px)
  "--moss-chrome-panel-accent",        // .moss-progress-task-action (12px/500)
  "--moss-chrome-panel-danger",        // error text
];

/** Non-text marks: SC 1.4.11 governs these at 3:1. */
const MARK_TOKENS = ["--moss-chrome-panel-degraded", "--moss-chrome-panel-broken"];

describe("advisory panel contrast, over the whole backdrop space", () => {
  for (const pole of POLES) {
    describe(pole, () => {
      const backing = (bd: RGB) => composite(parseColor(token(pole, "--moss-chrome-panel-bg")), bd);

      for (const [label, bd] of BACKDROPS) {
        for (const name of TEXT_TOKENS) {
          it(`${name} clears 4.5:1 over a ${label}`, () => {
            const fg = parseColor(token(pole, name));
            expect(contrast([fg[0], fg[1], fg[2]], backing(bd))).toBeGreaterThanOrEqual(4.5);
          });
        }
        for (const name of MARK_TOKENS) {
          it(`${name} clears 3:1 over a ${label}`, () => {
            const fg = parseColor(token(pole, name));
            expect(contrast([fg[0], fg[1], fg[2]], backing(bd))).toBeGreaterThanOrEqual(3.0);
          });
        }
      }

      // The invariant the floors alone do not express: the panel's legibility
      // must not be a function of the author's backdrop. Before the opaque
      // material the dark pole spread 6.8x (1.30 … 8.89) and the low end was
      // the reported bug. An alpha regression trips this long before it trips
      // any 4.5 threshold, which is the point of having it.
      it("body-text contrast is backdrop-independent (spread <= 1.3x)", () => {
        const fg = parseColor(token(pole, "--moss-chrome-panel-fg"));
        const ratios = BACKDROPS.map(([, bd]) =>
          contrast([fg[0], fg[1], fg[2]], backing(bd)),
        );
        expect(Math.max(...ratios) / Math.min(...ratios)).toBeLessThanOrEqual(1.3);
      });
    });
  }

  // The defect that started this: the panel's material was on the backdrop axis
  // and its ink on the app axis, so app-light ink over a dark-chrome panel
  // measured 1.02:1. Both poles defining a full ink set is what makes that
  // unrepresentable — there is no longer an app-axis colour in the subtree.
  it("both poles define every ink the panel renders", () => {
    for (const pole of POLES) {
      for (const name of [...TEXT_TOKENS, ...MARK_TOKENS, "--moss-chrome-panel-bg"]) {
        expect(() => token(pole, name)).not.toThrow();
      }
    }
  });
});
