import { describe, it, expect } from 'vitest';
import * as fs from 'node:fs';
import * as path from 'node:path';

const SITE_CSS_PATH = path.resolve(import.meta.dirname, '../../../../../crates/moss-build/src/assets/css/site.css');

/**
 * Extract CSS rule blocks from a CSS file.
 * Returns an array of { selector, properties } objects.
 * Simple parser — handles the flat structure of site.css.
 */
function extractRules(cssContent: string): Array<{ selector: string; properties: string }> {
  // Remove comments
  const noComments = cssContent.replace(/\/\*[\s\S]*?\*\//g, '');

  const rules: Array<{ selector: string; properties: string }> = [];
  // Match selector { properties } — non-greedy, handles nested media queries poorly
  // but sufficient for flat rules
  const ruleRegex = /([^{}@]+?)\{([^{}]*?)\}/g;
  let match;
  while ((match = ruleRegex.exec(noComments)) !== null) {
    const selector = match[1].trim();
    const properties = match[2].trim();
    if (selector && properties) {
      rules.push({ selector, properties });
    }
  }
  return rules;
}

describe('site.css — button hover safety', () => {
  const css = fs.readFileSync(SITE_CSS_PATH, 'utf-8');
  const rules = extractRules(css);

  it('article a:hover does not apply to .moss-btn links', () => {
    // Find rules that match "article a:hover" or "article a:focus"
    // and set a color property
    const dangerousRules = rules.filter(r => {
      const sel = r.selector;
      // Matches "article a:hover" or "article a:focus" (with possible comma separation)
      const isArticleLinkHover = /article\s+a:(hover|focus)/.test(sel);
      const setsColor = /\bcolor\s*:/.test(r.properties);
      // Safe if it excludes .moss-btn via :not(.moss-btn)
      const excludesBtn = /a:(?:where\()?:not\(\.moss-btn\)/.test(sel);
      return isArticleLinkHover && setsColor && !excludesBtn;
    });

    expect(dangerousRules).toEqual([]);
  });

  it('global a:hover does not apply to .moss-btn links', () => {
    // Find rules matching bare "a:hover" or "a:focus" (not scoped to article, nav, etc.)
    // that set color
    const dangerousRules = rules.filter(r => {
      const sel = r.selector;
      // Match "a:hover" or "a:focus" that is NOT already scoped
      // (i.e., the selector is just "a:hover" or "a:hover, a:focus")
      const parts = sel.split(',').map(s => s.trim());
      const hasBareAHover = parts.some(p => /^a:(hover|focus)$/.test(p));
      const setsColor = /\bcolor\s*:/.test(r.properties);
      const excludesBtn = /a:(?:where\()?:not\(\.moss-btn\)/.test(sel);
      return hasBareAHover && setsColor && !excludesBtn;
    });

    expect(dangerousRules).toEqual([]);
  });

  it('article a base rule does not style .moss-btn links', () => {
    // Find rules matching bare "article a" (not hover/focus)
    // that set text-decoration or color (would add underline to buttons)
    const dangerousRules = rules.filter(r => {
      const sel = r.selector;
      // Match "article a" without pseudo-classes
      const isArticleA = /^article\s+a$/.test(sel.trim());
      const setsTextDecoration = /text-decoration\s*:/.test(r.properties);
      const excludesBtn = /a:(?:where\()?:not\(\.moss-btn\)/.test(sel);
      return isArticleA && setsTextDecoration && !excludesBtn;
    });

    expect(dangerousRules).toEqual([]);
  });
});

describe('site.css — buttons directive styles', () => {
  const css = fs.readFileSync(SITE_CSS_PATH, 'utf-8');

  it('has .moss-buttons container styles', () => {
    expect(css).toContain('.moss-buttons');
    expect(css).toMatch(/\.moss-buttons\s*\{[^}]*display:\s*flex/);
  });

  it('has primary button styles with background', () => {
    // v1 contract: button kinds collapsed to .moss-btn[data-role="primary"]
    expect(css).toContain('.moss-btn[data-role="primary"]');
    expect(css).toMatch(/\.moss-btn\[data-role="primary"\]\s*\{[^}]*background:/);
  });

  it('has secondary button styles with border', () => {
    expect(css).toContain('.moss-btn[data-role="secondary"]');
    expect(css).toMatch(/\.moss-btn\[data-role="secondary"\]\s*\{[^}]*border:/);
  });

  it('has hover states for primary and secondary', () => {
    expect(css).toMatch(/\.moss-btn\[data-role="primary"\]:(hover|focus)/);
    expect(css).toMatch(/\.moss-btn\[data-role="secondary"\]:(hover|focus)/);
  });
});
