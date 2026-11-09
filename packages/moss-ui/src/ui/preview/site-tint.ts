/**
 * Site-identity tint derivation — pure colour math, no DOM.
 *
 * The bridge reports the previewed site's accent (`bridge/site-accent.ts`);
 * this turns it into the identity veil colour the shell paints behind the
 * desktop chrome.
 *
 * Two rules matter. First, **transfer colour, never luminance.** Accents are
 * WCAG-normalised for text contrast, which makes them dark in light theme
 * (default `#2d5a2d`, okL ≈ 0.43). Painted into a light surface (okL ≈ 0.96)
 * they darken it — a stain, where the design wants light. So the derivation
 * keeps only the accent's hue and (capped) chroma, and takes lightness from
 * the app theme's surface. The cap also bounds neon author accents, which at
 * full chroma over a near-black dark surface would glow radioactive.
 *
 * Second, **moss's own default accent is not a site identity.** It is
 * checked against the raw reported channels and treated as absence, so the
 * chrome veil activates only for sites with a genuine custom accent.
 *
 * Everything is computed here in TypeScript rather than with CSS
 * `color-mix()`/relative-color: base-Monterey WebKit predates both (where
 * they would resolve the band to *transparent* via
 * invalid-at-computed-value-time), and jsdom can unit-test math it could
 * never test as CSS resolution.
 *
 * Conversion matrices are Björn Ottosson's reference oklab implementation.
 */

/** Tint lightness per app theme: at the light surface's lightness (recolour,
 *  never darken), a lift above the dark surface (a glow, never a hole). */
export const TINT_L = { light: 0.96, dark: 0.3 } as const;

/** Chroma ceiling for the derived tint, in oklch C units. The default green
 *  sits around 0.06; neon accents reach 0.25+ and must not arrive whole. */
export const TINT_C_MAX = 0.07;

/** Identity veil strength at the window's top edge. Floor: visible in an A/B
 *  screenshot with a customized accent on a white page. Ceiling: the veil
 *  fades — a flat coat at any strength reads as dirt (Safari 15). */
export const VEIL_ALPHA = 0.12;

/** moss's own baked-in default `--moss-color-accent` (tokens.json), in the
 *  bridge's normalized rgb() channel form. Any site without a
 *  `.moss/theme/style.css` override inherits one of these verbatim, so a
 *  tint derived from them would read as "moss's brand," not the site's own —
 *  treated identically to no accent at all. */
const DEFAULT_ACCENT_RGB: readonly (readonly [number, number, number])[] = [
  [45, 90, 45], // light: #2d5a2d
  [106, 154, 90], // dark: #6a9a5a
];

function isDefaultAccent(r: number, g: number, b: number): boolean {
  return DEFAULT_ACCENT_RGB.some(([dr, dg, db]) => r === dr && g === dg && b === db);
}

const srgbToLinear = (c: number) => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
};

const linearToSrgb = (c: number) => {
  const v = c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
  return v * 255;
};

/** sRGB (0..255) → oklch. H in degrees, 0 for achromatic colours. */
export function srgbToOklch(
  r: number,
  g: number,
  b: number,
): { L: number; C: number; H: number } {
  const lr = srgbToLinear(r);
  const lg = srgbToLinear(g);
  const lb = srgbToLinear(b);

  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);

  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;

  const C = Math.hypot(a, bb);
  const H = C < 1e-6 ? 0 : ((Math.atan2(bb, a) * 180) / Math.PI + 360) % 360;
  return { L, C, H };
}

/** oklch → sRGB (0..255, clamped). May be out of gamut before clamping —
 *  callers that care use inGamut() and reduce chroma instead. */
export function oklchToSrgb(L: number, C: number, H: number): [number, number, number] {
  const rad = (H * Math.PI) / 180;
  const a = C * Math.cos(rad);
  const bb = C * Math.sin(rad);

  const l = (L + 0.3963377774 * a + 0.2158037573 * bb) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * bb) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * bb) ** 3;

  const lr = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const lg = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const lb = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  return [clamp(linearToSrgb(lr)), clamp(linearToSrgb(lg)), clamp(linearToSrgb(lb))];
}

function inGamut(L: number, C: number, H: number): boolean {
  const rad = (H * Math.PI) / 180;
  const a = C * Math.cos(rad);
  const bb = C * Math.sin(rad);
  const l = (L + 0.3963377774 * a + 0.2158037573 * bb) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * bb) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * bb) ** 3;
  const lr = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const lg = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const lb = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
  return [lr, lg, lb].every((v) => v >= -1e-4 && v <= 1.0001);
}

/**
 * The bridge's normalized forms and nothing else: three integer channels,
 * optional alpha, closed and anchored. Trailing garbage after a valid prefix
 * is treated as absence. (Disjoint character classes keep this linear — no
 * backtracking blowup on hostile input.)
 */
const RGB_INPUT = /^rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)\s*(?:[,/]\s*[\d.]+%?)?\s*\)$/;

/** A real channel is 0-255; hostile grammar-passing input is clamped, not trusted. */
const channel = (v: string) => Math.min(255, Number(v));

/**
 * Derive the identity veil colour from a bridge-reported accent, or null
 * when there is none (including no accent, or moss's own default accent).
 *
 * Input is the normalized `rgb(...)` the bridge posts (anything else fails
 * the caller's whitelist before reaching here; the parse guard is defence in
 * depth). Chroma is capped, then reduced further if the (theme-lightness,
 * chroma, hue) point falls outside sRGB — desaturating preserves the hue,
 * where channel-clamping would shift it.
 */
export function deriveSiteTint(
  accent: string | null | undefined,
  theme: "light" | "dark",
): string | null {
  if (!accent) return null;
  const m = RGB_INPUT.exec(accent.trim());
  if (!m) return null;

  const r0 = channel(m[1]);
  const g0 = channel(m[2]);
  const b0 = channel(m[3]);
  if (isDefaultAccent(r0, g0, b0)) return null;

  const { C, H } = srgbToOklch(r0, g0, b0);
  const L = TINT_L[theme];
  let c = Math.min(C, TINT_C_MAX);
  while (c > 0.001 && !inGamut(L, c, H)) c *= 0.7;

  const [r, g, b] = oklchToSrgb(L, c, H);
  return `rgba(${r}, ${g}, ${b}, ${VEIL_ALPHA})`;
}
