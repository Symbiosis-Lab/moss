import { describe, it, expect, beforeAll } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/**
 * Regression: the advisory notice detail (.moss-notice-detail-content) must
 * never render JUSTIFIED — LEFT-aligned, whole words intact — NOT
 * `word-break: break-all`.
 *
 * `word-break: break-all` breaks between ANY two characters, which splits
 * English words mid-word and packs every line flush to both edges, producing
 * a false "justified / aligned-on-both-ends" look (no `text-align: justify`
 * is set — break-all's edge-to-edge fill IS the faux-justify). `<html
 * lang="zh…">` does not restyle the detail, so the one blanket break-all rule
 * mis-renders it for a CJK-locale UI too.
 *
 * The detail is the tail of an Advisory `what` string (progress-panel-stage.ts
 * `groupNotices`, split at the single em-dash " — "); it is English in practice
 * today (English `what` strings use " — "; localized-Chinese ones —
 * crates/moss-build/src/infra/app_advisory.rs — use the " —— " double-em-dash
 * convention, which does not contain the split substring, so they yield no
 * tail). But the fix is deliberately language-AGNOSTIC and does NOT rely on
 * that: left-align + word-break:normal + overflow-wrap:anywhere gives whole
 * words never split + ragged-right for English, correct glyph-wrapping for CJK,
 * and a pathologically-long single token (URL/path/hash) still wrapping rather
 * than overflowing the panel.
 *
 * Intentionally NOT `html[lang^="zh"]`-gated to restore justify: the gate keys
 * on UI locale, but the detail under a zh locale is still predominantly English,
 * so a locale-keyed justify would re-break the same bug.
 *
 * String lock only — jsdom does not load style.css, so a computed-style test is
 * not possible here; the real look is confirmed by live visual verification.
 */
/**
 * The advisory notice HEAD (.moss-notice-reason) is the wording contract's
 * requirement that notices remain visible at rest: publish-verify-strings.ts
 * picks an EN dash precisely so the splitter can't fold the sentence away,
 * and localized-Chinese strings use the "——" convention so their ENTIRE text
 * is head. The head must WRAP — no nowrap, no ellipsis, no line-clamp — with
 * the same Latin-safe trio as the detail below, plus tabular digits so the
 * 1 Hz elapsed ticker can't jitter the line width. Real-engine line budgets
 * live in the advisory-text-fit render gate; this locks the rule strings
 * jsdom can see.
 */
describe(".moss-notice-reason — the head wraps, never ellipsizes", () => {
  let rule = "";
  beforeAll(() => {
    const css = readFileSync(resolve(__dirname, "../../style.css"), "utf8");
    const start = css.indexOf(".moss-notice-reason {");
    expect(start).toBeGreaterThan(-1);
    rule = css.slice(start, css.indexOf("}", start));
  });

  it("does NOT force a single line (no white-space: nowrap)", () => {
    expect(rule).not.toMatch(/white-space:\s*nowrap/);
  });

  it("does NOT ellipsize or clamp — the head is 'visible at rest' by design", () => {
    expect(rule).not.toMatch(/text-overflow/);
    expect(rule).not.toMatch(/line-clamp/);
  });

  it("keeps whole Latin words while still breaking a pathological token", () => {
    expect(rule).toMatch(/word-break:\s*normal/);
    expect(rule).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("uses tabular digits so the 1 Hz elapsed ticker cannot shimmer the wrap", () => {
    expect(rule).toMatch(/font-variant-numeric:\s*tabular-nums/);
  });
});

describe(".moss-notice-detail-content — Latin-safe wrap", () => {
  let rule = "";
  beforeAll(() => {
    const css = readFileSync(resolve(__dirname, "../../style.css"), "utf8");
    const start = css.indexOf(".moss-notice-detail-content {");
    expect(start).toBeGreaterThan(-1);
    rule = css.slice(start, css.indexOf("}", start));
  });

  it("does NOT use word-break: break-all (the culprit that split words + faux-justified)", () => {
    expect(rule).not.toMatch(/word-break:\s*break-all/);
  });

  it("keeps whole words intact (word-break: normal)", () => {
    expect(rule).toMatch(/word-break:\s*normal/);
  });

  it("still wraps a pathologically-long token so it can't overflow (overflow-wrap: anywhere)", () => {
    expect(rule).toMatch(/overflow-wrap:\s*anywhere/);
  });

  it("aligns LEFT explicitly so it never inherits a justify from an ancestor", () => {
    expect(rule).toMatch(/text-align:\s*left/);
  });

  it("preserves whitespace/newlines from the technical output (white-space: pre-wrap)", () => {
    expect(rule).toMatch(/white-space:\s*pre-wrap/);
  });
});
