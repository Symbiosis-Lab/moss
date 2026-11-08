/**
 * Shell-side derivation of the site-identity tint
 *
 * The load-bearing rule is "transfer colour, never luminance": accents are
 * WCAG-normalised for text contrast, which makes them DARK in light theme —
 * mixed as-is into a light surface they stain instead of glow. So the
 * derivation takes hue and capped chroma from the accent and lightness from
 * the app theme, and these tests pin that behaviour numerically.
 *
 * A second rule, added after the tint always read as moss's own brand green:
 * moss's baked-in default accent (inherited by any site without a
 * `.moss/theme/style.css` override) is indistinguishable from a real site
 * identity unless it is checked for explicitly and treated as absence.
 */

import { describe, it, expect } from "vitest";
import {
  srgbToOklch,
  oklchToSrgb,
  deriveSiteTint,
  TINT_L,
  TINT_C_MAX,
  VEIL_ALPHA,
} from "../site-tint";

function parseRgbString(s: string): [number, number, number] {
  const m = /^rgba?\((\d+), (\d+), (\d+)/.exec(s);
  if (!m) throw new Error(`not an rgb string: ${s}`);
  return [Number(m[1]), Number(m[2]), Number(m[3])];
}

// A stand-in "customized" accent — deliberately NOT moss's own default, so
// the general derivation-math tests below aren't short-circuited by the
// default-accent guard tested separately.
const CUSTOM_ACCENT = "rgb(80, 40, 120)";

describe("srgbToOklch / oklchToSrgb", () => {
  it("round-trips representative colours", () => {
    for (const [r, g, b] of [
      [45, 90, 45],
      [244, 241, 236],
      [36, 32, 24],
      [255, 0, 0],
      [128, 128, 128],
    ] as const) {
      const { L, C, H } = srgbToOklch(r, g, b);
      const back = oklchToSrgb(L, C, H);
      expect(Math.abs(back[0] - r)).toBeLessThanOrEqual(1);
      expect(Math.abs(back[1] - g)).toBeLessThanOrEqual(1);
      expect(Math.abs(back[2] - b)).toBeLessThanOrEqual(1);
    }
  });

  it("reports known lightness landmarks", () => {
    // White and black anchor the scale; the default light accent sits mid-dark
    // (the number behind the design's stain-vs-glow finding).
    expect(srgbToOklch(255, 255, 255).L).toBeCloseTo(1.0, 1);
    expect(srgbToOklch(0, 0, 0).L).toBeCloseTo(0.0, 1);
    const accent = srgbToOklch(45, 90, 45); // #2d5a2d
    expect(accent.L).toBeGreaterThan(0.35);
    expect(accent.L).toBeLessThan(0.55);
  });
});

describe("deriveSiteTint", () => {
  it("lifts a dark light-theme accent to surface lightness (the stain case)", () => {
    // A dark, saturated accent painted into a 0.96 surface darkens it. The
    // derived veil must instead sit AT surface lightness, carrying only the
    // accent's hue.
    const veil = deriveSiteTint(CUSTOM_ACCENT, "light");
    expect(veil).not.toBeNull();
    const { L, C } = srgbToOklch(...parseRgbString(veil!));
    expect(L).toBeCloseTo(TINT_L.light, 1);
    expect(C).toBeLessThanOrEqual(TINT_C_MAX + 0.01);
  });

  it("caps a neon accent's chroma in both themes", () => {
    for (const theme of ["light", "dark"] as const) {
      const veil = deriveSiteTint("rgb(0, 255, 255)", theme);
      const { C } = srgbToOklch(...parseRgbString(veil!));
      expect(C).toBeLessThanOrEqual(TINT_C_MAX + 0.01);
    }
  });

  it("keeps the dark-theme veil a lift above the dark surface (a glow, not a hole)", () => {
    const veil = deriveSiteTint(CUSTOM_ACCENT, "dark");
    const { L } = srgbToOklch(...parseRgbString(veil!));
    expect(L).toBeCloseTo(TINT_L.dark, 1);
  });

  it("emits the veil at the veil alpha", () => {
    const veil = deriveSiteTint(CUSTOM_ACCENT, "light")!;
    expect(veil).toMatch(new RegExp(`, ${VEIL_ALPHA}\\)$`));
  });

  it("stays inside sRGB gamut even for hostile hue/lightness pairs", () => {
    // A saturated blue at L 0.96 is far outside sRGB; the derivation must
    // desaturate into gamut rather than emit clipped channels that shift hue.
    for (const accent of ["rgb(0, 0, 255)", "rgb(255, 0, 0)", "rgb(255, 0, 255)"]) {
      for (const theme of ["light", "dark"] as const) {
        const veil = deriveSiteTint(accent, theme);
        const [r, g, b] = parseRgbString(veil!);
        const { L } = srgbToOklch(r, g, b);
        expect(L).toBeCloseTo(TINT_L[theme], 1);
      }
    }
  });

  it("returns null for null, empty, and non-colour input", () => {
    expect(deriveSiteTint(null, "light")).toBeNull();
    expect(deriveSiteTint("", "light")).toBeNull();
    expect(deriveSiteTint("url(evil)", "light")).toBeNull();
  });

  it("clamps hostile out-of-range channels before deriving the hue", () => {
    // Output is safe either way (L forced, chroma capped, string
    // shell-constructed) — but the hue must come from a real colour.
    expect(deriveSiteTint("rgb(999, 0, 300)", "light")).toEqual(
      deriveSiteTint("rgb(255, 0, 255)", "light"),
    );
  });

  it("accepts the bridge's forms and nothing trailing them", () => {
    expect(deriveSiteTint(CUSTOM_ACCENT, "light")).not.toBeNull();
    expect(deriveSiteTint("rgba(80, 40, 120, 0.5)", "light")).not.toBeNull();
    expect(deriveSiteTint("rgb(80 40 120 / 0.5)", "light")).not.toBeNull();
    expect(deriveSiteTint(`${CUSTOM_ACCENT}garbage`, "light")).toBeNull();
  });
});

describe("default accent detection", () => {
  // Any site without a `.moss/theme/style.css` override inherits moss's own
  // baked-in default (#2d5a2d light / #6a9a5a dark, tokens.json). Deriving a
  // tint from it reads as "moss's brand", not the site's own identity — the
  // reported bug ("always green, matching the default of moss"). Checked
  // against the raw parsed channels, independent of the app theme parameter:
  // the accent's own value already encodes which of the two defaults it is.
  it("treats moss's own default accent as no site identity, in either app theme", () => {
    expect(deriveSiteTint("rgb(45, 90, 45)", "light")).toBeNull();
    expect(deriveSiteTint("rgb(45, 90, 45)", "dark")).toBeNull();
    expect(deriveSiteTint("rgb(106, 154, 90)", "light")).toBeNull();
    expect(deriveSiteTint("rgb(106, 154, 90)", "dark")).toBeNull();
  });

  it("still derives a tint for a genuinely customized accent", () => {
    expect(deriveSiteTint(CUSTOM_ACCENT, "light")).not.toBeNull();
  });
});
