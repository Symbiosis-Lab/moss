/**
 * Tests for the shell half of the ambient band.
 *
 * The colours under test arrive by postMessage from previewed content, which
 * is author-controlled, and are written into a CSS custom property. So the
 * cases that matter are the hostile ones: anything that would let a `.md` file
 * in a user's site steer the app shell's styling has to be dropped, not
 * escaped and not passed through.
 */
import { describe, it, expect } from "vitest";
import { isSafeColor, buildAmbientGradient, applySiteAccent } from "../ambient-band";

// A stand-in "customized" accent — deliberately NOT moss's own default, so
// these wiring tests aren't short-circuited by the default-accent guard
// (covered separately below and in site-tint.test.ts).
const CUSTOM_ACCENT = "rgb(80, 40, 120)";

describe("isSafeColor", () => {
  it("accepts the syntaxes the sampler actually emits", () => {
    expect(isSafeColor("rgb(10, 20, 30)")).toBe(true);
    expect(isSafeColor("rgba(10, 20, 30, 0.5)")).toBe(true);
    expect(isSafeColor("#abc")).toBe(true);
    expect(isSafeColor("#0a141e")).toBe(true);
    // Build-time --moss-cover-color is emitted as hsla by prepare_cover_color_muted.
    expect(isSafeColor("hsla(212, 14%, 29%, 1)")).toBe(true);
    expect(isSafeColor("  rgb(1, 2, 3)  ")).toBe(true);
  });

  it("rejects anything that could carry a payload into the stylesheet", () => {
    expect(isSafeColor("url(https://evil.example/x.png)")).toBe(false);
    expect(isSafeColor("var(--anything)")).toBe(false);
    expect(isSafeColor("red; position: fixed; inset: 0")).toBe(false);
    expect(isSafeColor("rgb(1,2,3); background: url(x)")).toBe(false);
    expect(isSafeColor("image-set(url(x))")).toBe(false);
    expect(isSafeColor("</style><script>alert(1)</script>")).toBe(false);
  });

  it("rejects structurally invalid colours, which would blank the band", () => {
    // These carry no injection risk, but an invalid value makes
    // `background: var(--moss-ambient-band, <fallback>)` invalid-at-computed-
    // value-time, which resolves to `unset` — NOT to the fallback. The band
    // paints transparent instead of degrading to the flat page colour.
    expect(isSafeColor("#12345")).toBe(false);
    expect(isSafeColor("#1234567")).toBe(false);
    expect(isSafeColor("rgb(...)")).toBe(false);
    expect(isSafeColor("rgb(,,)")).toBe(false);
    expect(isSafeColor("rgb(/////)")).toBe(false);
    expect(isSafeColor("rgb(   )")).toBe(false);
    expect(isSafeColor("rgb(%)")).toBe(false);
    expect(isSafeColor("hsl(1)")).toBe(false);
    expect(isSafeColor("rgb(1,2,3,4,5,6)")).toBe(false);
  });

  it("still accepts the legal spellings the tightening could have broken", () => {
    expect(isSafeColor("#1234")).toBe(true); // 4-digit hex with alpha
    expect(isSafeColor("#12345678")).toBe(true);
    expect(isSafeColor("rgb(0 0 0)")).toBe(true); // space-separated
    expect(isSafeColor("rgb(0 0 0 / 50%)")).toBe(true);
    expect(isSafeColor("rgb(100%, 0%, 0%)")).toBe(true);
    expect(isSafeColor("hsl(240deg 100% 50%)")).toBe(true);
    expect(isSafeColor("rgb(.5, 0, 0)")).toBe(true);
  });

  it("rejects non-strings and empty input rather than coercing", () => {
    expect(isSafeColor(null)).toBe(false);
    expect(isSafeColor(undefined)).toBe(false);
    expect(isSafeColor(42)).toBe(false);
    expect(isSafeColor({})).toBe(false);
    expect(isSafeColor("")).toBe(false);
  });

  it("rejects oversized strings before the grammar ever runs", () => {
    // The RGB/HSL patterns backtrack quadratically on a long digit run that
    // never satisfies the separator — a hostile page posting
    // "rgb(999…999)" at 200k chars freezes the SHELL's main thread inside
    // the filter. A real CSS colour is under 64 chars; longer is hostile by
    // construction and must fail on length alone.
    expect(isSafeColor("rgb(" + "9".repeat(200000) + ")")).toBe(false);
    expect(isSafeColor("hsl(" + "9".repeat(200000) + ")")).toBe(false);
    // Padding a valid colour past the cap is also rejected — trailing
    // whitespace is the only >64-char form a legitimate sender could emit,
    // and the sampler never pads.
    expect(isSafeColor("rgb(1, 2, 3)" + " ".repeat(100))).toBe(false);
  });
});

describe("buildAmbientGradient", () => {
  it("spreads stops evenly across the width", () => {
    expect(buildAmbientGradient(["rgb(0, 0, 0)", "rgb(255, 255, 255)"])).toBe(
      "linear-gradient(90deg, rgb(0, 0, 0) 0.0%, rgb(255, 255, 255) 100.0%)",
    );
  });

  it("places a middle stop at the midpoint", () => {
    const out = buildAmbientGradient(["#000", "#888", "#fff"]);
    expect(out).toBe("linear-gradient(90deg, #000 0.0%, #888 50.0%, #fff 100.0%)");
  });

  it("emits a flat fill for a single sample, not a one-stop gradient", () => {
    // (i / (n-1)) would be 0/0 = NaN here; a NaN% stop makes the whole
    // declaration invalid and the band silently stops painting.
    expect(buildAmbientGradient(["rgb(9, 9, 9)"])).toBe("rgb(9, 9, 9)");
  });

  it("drops unsafe entries and keeps the rest", () => {
    const out = buildAmbientGradient(["rgb(1, 2, 3)", "url(evil)", "#fff"]);
    expect(out).toBe("linear-gradient(90deg, rgb(1, 2, 3) 0.0%, #fff 100.0%)");
  });

  it("returns null when every entry is unsafe", () => {
    expect(buildAmbientGradient(["url(evil)", "var(--x)"])).toBeNull();
  });

  it("returns null for empty, missing, or non-array input", () => {
    expect(buildAmbientGradient([])).toBeNull();
    expect(buildAmbientGradient(undefined)).toBeNull();
    expect(buildAmbientGradient(null)).toBeNull();
    expect(buildAmbientGradient("rgb(1, 2, 3)")).toBeNull();
    expect(buildAmbientGradient({ 0: "#fff", length: 1 })).toBeNull();
  });

  it("bounds the samples it will look at — the sampler sends 7, hostile pages send millions", () => {
    const out = buildAmbientGradient(Array(1000).fill("#abc"));
    expect((out!.match(/#abc/g) ?? []).length).toBe(32);
  });
});

describe("applySiteAccent", () => {
  // Same trust boundary as the band: the accent arrives by postMessage from
  // previewed content. The derivation only ever emits an rgba() string it
  // constructs from parsed integers, so what reaches CSS is shell-built —
  // but hostile input must still be treated as absence, never written.
  it("writes the veil derived from a valid, customized accent", () => {
    const root = document.createElement("div");
    applySiteAccent(root, CUSTOM_ACCENT, "light");
    const veil = root.style.getPropertyValue("--moss-site-veil");
    expect(veil).toMatch(/^rgba\(\d+, \d+, \d+, 0\.12\)$/);
  });

  it("clears the veil on null (token-less page, folder switch)", () => {
    const root = document.createElement("div");
    applySiteAccent(root, CUSTOM_ACCENT, "light");
    applySiteAccent(root, null, "light");
    expect(root.style.getPropertyValue("--moss-site-veil")).toBe("");
  });

  it("treats hostile or non-normalized input as absence", () => {
    const root = document.createElement("div");
    applySiteAccent(root, CUSTOM_ACCENT, "light");
    for (const bad of [
      "url(https://evil.example/x.png)",
      "var(--anything)",
      "red; position: fixed; inset: 0",
      // Valid colour grammar but not the bridge's normalized rgb() form —
      // the derivation parses strictly, so these clear rather than pass.
      "#2d5a2d",
      "hsl(120, 33%, 26%)",
    ]) {
      applySiteAccent(root, bad, "light");
      expect(root.style.getPropertyValue("--moss-site-veil")).toBe("");
      applySiteAccent(root, CUSTOM_ACCENT, "light");
    }
  });

  it("treats moss's own default accent as absence — the root-cause fix", () => {
    // Any site without a `.moss/theme/style.css` override inherits moss's
    // own baked-in default (#2d5a2d light / #6a9a5a dark), which read as
    // "moss brand", not "site identity" (the reported bug: the mobile glow
    // was always green).
    const root = document.createElement("div");
    applySiteAccent(root, CUSTOM_ACCENT, "light");
    applySiteAccent(root, "rgb(45, 90, 45)", "light");
    expect(root.style.getPropertyValue("--moss-site-veil")).toBe("");
  });

  it("re-derives for the theme it is given (light lifts, dark glows)", () => {
    const root = document.createElement("div");
    applySiteAccent(root, CUSTOM_ACCENT, "light");
    const light = root.style.getPropertyValue("--moss-site-veil");
    applySiteAccent(root, CUSTOM_ACCENT, "dark");
    const dark = root.style.getPropertyValue("--moss-site-veil");
    expect(light).not.toBe(dark);
  });
});
