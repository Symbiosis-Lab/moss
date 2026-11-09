import { describe, it, expect, beforeAll } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/**
 * Regression: the seam severity dot must anchor to the hairline's RIGHT end
 * (the left-anchored progress fill reaches the right last, so the
 * dot stays visible). The base `.moss-severity-dot` rule (used by the notices
 * rail) sets `left: …`; the seam-scoped `.moss-pill-cluster .moss-severity-dot`
 * therefore MUST set `left: auto` or, with both left+right on a fixed-width
 * absolute box, `left` wins (LTR) and the dot renders on the LEFT (the bug).
 */
describe("seam severity dot — right-anchored", () => {
  let rule = "";
  beforeAll(() => {
    const css = readFileSync(resolve(__dirname, "../../style.css"), "utf8");
    // Direct-child combinator: the seam rule must NOT leak onto in-panel notice
    // dots (the panel is mounted inside .moss-pill-cluster).
    const start = css.indexOf(".moss-pill-cluster > .moss-severity-dot {");
    expect(start).toBeGreaterThan(-1);
    rule = css.slice(start, css.indexOf("}", start));
  });

  it("anchors to the right edge (right: var(--moss-pill-radius))", () => {
    expect(rule).toMatch(/right:\s*var\(--moss-pill-radius\)/);
  });

  it("clears the inherited left so right wins (left: auto)", () => {
    expect(rule).toMatch(/left:\s*auto/);
  });
});

/**
 * The seam/hairline layer must paint ABOVE the anchored
 * progress panel. The panel uses `z-index: var(--z-toast)` (200); the seam (z:2),
 * rebuild-fill track (z:3) and severity dot (z:4) were all BELOW it, so the
 * panel's glass top covered the hairline whenever the panel opened (the
 * "window blocks the hairline" bug). Each must now exceed the panel's z.
 *
 * NOTE: string lock only — paint order is a cascade/stacking-context property a
 * jsdom/string test cannot evaluate. The real guarantee is a render check
 * (Playwright elementFromPoint at the weld with pointer-events probed).
 */
describe("seam layer stacks above the anchored panel (regression)", () => {
  let css = "";
  beforeAll(() => {
    css = readFileSync(resolve(__dirname, "../../style.css"), "utf8");
  });
  const ruleBody = (selector: string) => {
    const start = css.indexOf(selector);
    expect(start).toBeGreaterThan(-1);
    return css.slice(start, css.indexOf("}", start));
  };

  it("seam z-index exceeds the panel (calc(var(--z-toast) + 1))", () => {
    expect(ruleBody(".moss-progress-seam {")).toMatch(/z-index:\s*calc\(var\(--z-toast\)\s*\+\s*1\)/);
  });
  it("rebuild-fill track z-index exceeds the panel (calc(var(--z-toast) + 2))", () => {
    expect(ruleBody(".moss-preview-hairline-track {")).toMatch(/z-index:\s*calc\(var\(--z-toast\)\s*\+\s*2\)/);
  });
  it("severity dot z-index exceeds the panel (calc(var(--z-toast) + 3))", () => {
    expect(ruleBody(".moss-pill-cluster > .moss-severity-dot {")).toMatch(/z-index:\s*calc\(var\(--z-toast\)\s*\+\s*3\)/);
  });
});
