/**
 * Structural tests for the site-identity tint's CSS consumers, in the same
 * idiom as layout-zones.test.ts: read the source, assert the properties that
 * are easy to quietly break in a refactor. Rendering is covered by the visual
 * pass.
 */

import { describe, test, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const styleCss = readFileSync(resolve(__dirname, "..", "style.css"), "utf8");
const noComments = styleCss.replace(/\/\*[\s\S]*?\*\//g, "");

function ruleOf(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = new RegExp(`^${escaped}\\s*\\{([^}]*)\\}`, "m").exec(noComments);
  expect(m, `rule not found: ${selector}`).not.toBeNull();
  return m![1];
}

describe("identity veil", () => {
  test("is defined once, fades over the titlebar height, and is inert when unset", () => {
    // The veil must FADE — a flat coat meets the untinted page below as a
    // visible seam on every page (the Safari 15 'dirty toolbar' read).
    const defs = noComments.match(/--moss-identity-veil:\s*linear-gradient\([^;]+;/g) ?? [];
    expect(defs).toHaveLength(1);
    expect(defs[0]).toContain("to bottom");
    expect(defs[0]).toContain("var(--moss-site-veil, transparent)");
    expect(defs[0]).toContain("transparent var(--moss-titlebar-height)");
  });

  test("layers over the band without touching the sampled fallback chain", () => {
    const band = ruleOf(".moss-ambient-band");
    expect(band).toContain("var(--moss-identity-veil)");
    // The existing chain survives beneath it — samples, then page colour.
    expect(band).toContain(
      "var(--moss-ambient-band, var(--moss-page-color, var(--moss-color-bg)))",
    );
  });

  test("does not tint the glass surface — the glass blurs the veil behind it", () => {
    // Tinting .moss-glass-surface too would compound veil + blur-of-veil in
    // the pill strip ('slightly too much', the exact Safari 15 failure).
    const glass = ruleOf(".moss-glass-surface");
    expect(glass).not.toContain("--moss-site-veil");
    expect(glass).not.toContain("--moss-identity-veil");
  });
});

describe("mobile room", () => {
  const room = () => ruleOf(".moss-preview-container.mobile-mode");

  test("keeps the sampled band hidden — content colours make no positional claim there", () => {
    const rule = ruleOf(".moss-preview-container.mobile-mode .moss-ambient-band");
    expect(rule).toContain("display: none");
  });

  test("is a plain surface — the mobile room no longer paints site identity", () => {
    // Painting the letterbox with the site tint made moss's own baked-in
    // default accent (#2d5a2d) read as the room's colour on every site
    // without custom branding — reported as "always green". Scoped back to
    // the desktop chrome band only.
    expect(room()).toContain("background: var(--moss-color-surface);");
    expect(room()).not.toContain("--moss-identity-veil");
  });
});

describe("phase-2 glow removed", () => {
  test("no references to the room wash or ambient glow remain in the stylesheet", () => {
    expect(noComments).not.toContain("--moss-room-wash");
    expect(noComments).not.toContain("--moss-ambient-glow");
  });
});

describe("registered properties", () => {
  test("the site veil is registered as a colour", () => {
    const reg = /@property --moss-site-veil\s*\{[^}]*syntax:\s*"<color>"[^}]*\}/;
    expect(noComments).toMatch(reg);
  });
});
