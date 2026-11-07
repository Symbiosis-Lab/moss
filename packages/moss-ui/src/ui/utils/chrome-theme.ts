/**
 * Pure helpers to choose the floating-chrome appearance ("light"|"dark") from
 * the previewed page's effective background color. No DOM access — unit-tested.
 *
 * Why a contrast-maximizing threshold (~0.179) instead of 0.5: we want the glyph
 * color (dark on "light" chrome, light on "dark" chrome) to maximize WCAG contrast
 * against the backdrop. The crossover where black vs white text give equal contrast
 * is relative luminance ~0.179 ((L+0.05)^2 = 0.05*1.05). A dead-band avoids flip
 * oscillation as the user navigates between near-threshold pages.
 */

const THRESHOLD = 0.179;
const DEAD_BAND = 0.06;

function parseRgb(color: string): [number, number, number] | null {
  const c = color.trim();
  let m = /^#([0-9a-fA-F]{3})$/.exec(c);
  if (m) {
    const h = m[1];
    return [parseInt(h[0] + h[0], 16), parseInt(h[1] + h[1], 16), parseInt(h[2] + h[2], 16)];
  }
  m = /^#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$/.exec(c);
  if (m) {
    const h = m[1];
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  m = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/.exec(c);
  if (m) {
    return [Number(m[1]), Number(m[2]), Number(m[3])];
  }
  return null;
}

export function relativeLuminance(color: string): number | null {
  const rgb = parseRgb(color);
  if (!rgb) return null;
  // Guard against non-numeric channels (e.g. percentage rgb "rgb(100% 50% 0%)",
  // which parseRgb's number coercion turns into NaN). Mirror intended theme rather
  // than computing a bogus luminance.
  if (rgb.some((v) => Number.isNaN(v))) return null;
  const lin = rgb.map((v) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

export function effectiveChromeTheme(
  color: string | null | undefined,
  previous: "light" | "dark" | null,
): "light" | "dark" | null {
  if (!color) return null;
  const L = relativeLuminance(color);
  if (L == null) return null;
  if (L > THRESHOLD + DEAD_BAND) return "light";
  if (L < THRESHOLD - DEAD_BAND) return "dark";
  // Inside the dead-band: keep the previously-committed theme to avoid oscillation.
  if (previous) return previous;
  return L >= THRESHOLD ? "light" : "dark";
}
