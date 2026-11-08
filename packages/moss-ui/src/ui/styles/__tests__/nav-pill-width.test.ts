/**
 * The nav pill's width is INVARIANT across its states: the pill cluster's width,
 * which is var(--moss-surface-width) unless the title row is narrower (measured
 * by browser tests of the main shell caption), and only its height
 * ever changes. The pill and the drawer welded under it both take the
 * cluster's width, so they cannot disagree.
 *
 * String gate — reads style.css and asserts the rule bodies. Does NOT assert any
 * @media nav-pill rule (none exists; the 180px in the 700px block is
 * .moss-settings-tabs, unrelated). jsdom cannot see the resulting geometry;
 * a render gate measuring preview surface geometry
 * measures the pill and the panel in both the resting and .editing states.
 */

import { describe, it, expect, beforeAll } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

// style.css lives two directories up from styles/__tests__/
const css = fs.readFileSync(
  path.resolve(__dirname, '../../style.css'),
  'utf-8',
);

/**
 * Extract the body of a CSS rule anchored on a given selector string.
 * The selector must be preceded by a newline (to avoid matching nested
 * rules like `.parent .child` when looking for `.child`).
 * Uses dotAll so `[\s\S]` matches newlines.
 */
function extractRule(selector: string): string {
  const escaped = selector.replace(/[.*+?^$()|[\]{}\\]/g, '\\$&');
  // Require a newline before the selector so we don't match sub-selectors
  const re = new RegExp(`(?:^|\\n)${escaped}\\s*\\{([\\s\\S]*?)\\}`, 's');
  const match = css.match(re);
  if (!match) throw new Error(`No rule found for selector: ${selector}`);
  return match[1];
}

/** Any width, max-width or min-width declaration in a rule body. */
const ANY_WIDTH_DECL = /(?:^|[;{\s])(?:min-|max-)?width\s*:/;

describe('nav-pill width contract', () => {
  describe('.moss-nav-pill base rule (anchored on ".moss-nav-pill {")', () => {
    let body: string;
    beforeAll(() => {
      body = extractRule('.moss-nav-pill');
    });

    it("takes the cluster's width", () => {
      expect(body).toMatch(/(?:^|[;\s])width:\s*100%/);
    });

    it('carries no min/max clamp that could override it', () => {
      expect(body).not.toMatch(/(?:min|max)-width\s*:/);
    });
  });

  describe('.moss-nav-pill.editing rule', () => {
    let body: string;
    beforeAll(() => {
      body = extractRule('.moss-nav-pill.editing');
    });

    it('introduces NO width declaration — the editing pill keeps the surface width', () => {
      expect(body).not.toMatch(ANY_WIDTH_DECL);
    });

    it('keeps overflow:visible and the focus ring', () => {
      expect(body).toMatch(/overflow:\s*visible/);
      expect(body).toMatch(/box-shadow:/);
    });
  });

  describe('.moss-url-autocomplete-container rule', () => {
    let body: string;
    beforeAll(() => {
      body = extractRule('.moss-url-autocomplete-container');
    });

    it('has no hardcoded width — it flexes to fill the pill between the arrows', () => {
      expect(body).not.toMatch(ANY_WIDTH_DECL);
    });
  });

  describe('.moss-url-text rule', () => {
    let body: string;
    beforeAll(() => {
      body = extractRule('.moss-url-text');
    });

    it('has min-width:0 (prevents flex overflow)', () => {
      expect(body).toMatch(/min-width:\s*0/);
    });
  });
});

/**
 * The anchored progress panel rendered NARROWER than the pill (270px vs 300px).
 * Root cause: the base `.moss-progress-panel` rule carries `max-width: 90%` (for
 * the legacy centered-over-preview placement). When anchored, the panel is
 * absolutely positioned inside the `width:fit-content` .moss-pill-cluster (which
 * shrink-wraps the pill to --moss-surface-width), so `90%` clamps the panel to
 * 0.9 × surface-width. The `[data-anchor="nav-pill"]` rule MUST override it.
 *
 * NOTE: this is a STRING lock — it prevents the fix from being removed, but a
 * string assertion cannot SEE the cascade/containing-block interaction that
 * caused the bug (that requires a real render — see the preview-surface
 * Playwright geometry check). Keep both.
 */
describe('anchored progress panel width contract (regression: pill == panel)', () => {
  let body: string;
  beforeAll(() => {
    body = extractRule('.moss-progress-panel[data-anchor="nav-pill"]');
  });

  it("takes the cluster's width, the same as the pill", () => {
    expect(body).toMatch(/(?:^|[;\s])width:\s*100%/);
  });

  it('neutralizes the inherited max-width:90% clamp (max-width: none)', () => {
    expect(body).toMatch(/max-width:\s*none/);
  });

  it('neutralizes the inherited 280px floor, so a narrowed pill keeps its drawer (min-width: 0)', () => {
    expect(body).toMatch(/min-width:\s*0/);
  });
});
