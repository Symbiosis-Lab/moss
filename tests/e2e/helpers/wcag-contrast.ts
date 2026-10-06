/**
 * WCAG 2.x relative luminance and contrast — the one formula every render
 * gate that checks text-over-image legibility needs (hero-tone, now
 * hero-overlay-legibility), extracted so the two don't carry silently
 * driftable copies of the same constants.
 */

/** WCAG relative luminance of a single sRGB 0-255 channel value. */
function channelLuminance(c: number): number {
  const v = c / 255;
  return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
}

/** WCAG relative luminance of an sRGB colour (each channel 0-255). */
export function relativeLuminance(r: number, g: number, b: number): number {
  return 0.2126 * channelLuminance(r) + 0.7152 * channelLuminance(g) + 0.0722 * channelLuminance(b);
}

/** WCAG relative luminance parsed straight from a `getComputedStyle` colour string. */
export function luminanceOfCss(css: string): number {
  const nums = css.match(/[\d.]+/g);
  if (!nums || nums.length < 3) throw new Error(`not a colour: ${css}`);
  const [r, g, b] = nums.slice(0, 3).map(Number);
  return relativeLuminance(r, g, b);
}

/** WCAG 2.x contrast ratio between two relative luminances. */
export function contrastRatio(l1: number, l2: number): number {
  const [lighter, darker] = l1 > l2 ? [l1, l2] : [l2, l1];
  return (lighter + 0.05) / (darker + 0.05);
}

/** Parses `rgb(r, g, b)` / `rgba(r, g, b, a)` as `getComputedStyle` reports it. */
export function parseColor(css: string): { r: number; g: number; b: number; a: number } {
  const nums = css.match(/[\d.]+/g);
  if (!nums || nums.length < 3) throw new Error(`not a colour: ${css}`);
  const [r, g, b, a] = nums.map(Number);
  return { r, g, b, a: a ?? 1 };
}
