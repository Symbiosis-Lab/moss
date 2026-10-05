// Shared by the solver and its default preset's tuning curves.
export const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));
export const smooth = (a: number, b: number, t: number): number => { const x = clamp01((t - a) / (b - a)); return x * x * (3 - 2 * x); };

/**
 * How much of the wash's own look a position shows: 0 exactly at either end of
 * a leg, where the canvas must be the print itself (a host hands off to its own
 * page there), rising to 1 over `margin` of the leg at each end. Refraction, the
 * pigment's colour and the paper's clearing are all scaled by this one number;
 * nothing else decides how much of them to show. A margin that is not a positive
 * number (a preset that left it out) means no fade: the full look everywhere
 * inside the leg, so the envelope can never be NaN and blank the whole wash.
 */
export const endpointPresence = (p: number, margin: number): number =>
  margin > 0 ? smooth(0, margin, p) * smooth(0, margin, 1 - p) : p > 0 && p < 1 ? 1 : 0;


/**
 * How much of its pigment a leg still shows when its ground is transparent: 1
 * exactly at either end, easing down to `floor` at the middle of the leg and
 * back up, with no flat stretch and no corner anywhere. It scales the
 * pigment's optical thickness, so the print thins to a translucent film rather
 * than being cut out, and the next one thickens back out of it (the paper
 * itself is already gone by the end of the endpoint margin). Symmetric, so a
 * leg played backward looks the same. A floor that is not a number leaves the
 * pigment unthinned.
 */
export const drain = (p: number, floor: number): number => {
  const f = Number.isFinite(floor) ? clamp01(floor) : 1;
  const s = Math.sin(Math.PI * clamp01(p));
  return 1 - (1 - f) * s * s;
};

/**
 * Whether a page colour (sRGB, 0..1 per channel) is dark enough that the wash
 * must be drawn the other way round: light pigment on a dark sheet rather than
 * shadow taken out of a light one. Decided once, from the page's luma.
 */
export const isDarkPage = (rgb: readonly number[]): boolean =>
  0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2] < 0.5;

export type Colour = string | readonly [number, number, number];

/**
 * A page colour as sRGB 0..1: `#rgb`, `#rrggbb` (either case), a CSS computed
 * colour `rgb(r, g, b)` / `rgba(r, g, b, 1)` (0..255 integers, as
 * `getComputedStyle(...).backgroundColor` returns), or an `[r, g, b]` triple
 * already in that range. Anything else, or a triple with a non-finite channel,
 * is white, so a host whose custom property is empty or exotic gets the light
 * page and never a NaN in a shader uniform. A non-empty string that is not
 * understood (a translucent rgba, `oklch(...)`, `#rrggbbaa`) warns once per
 * distinct string, so a mis-drawn theme is never silent.
 */
const warned = new Set<string>();
export const parseColour = (c: Colour): [number, number, number] => {
  if (typeof c !== 'string') return c.length === 3 && c.every(Number.isFinite) ? [clamp01(c[0]), clamp01(c[1]), clamp01(c[2])] : [1, 1, 1];
  const s = c.trim();
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(s);
  if (m) {
    const h = m[1].length === 3 ? m[1].replace(/./g, '$&$&') : m[1], n = parseInt(h, 16);
    return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
  }
  const f = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*1(?:\.0*)?\s*)?\)$/i.exec(s);
  if (f && [f[1], f[2], f[3]].every((v) => +v <= 255)) return [+f[1] / 255, +f[2] / 255, +f[3] / 255];
  if (s && !warned.has(s)) { warned.add(s); console.warn(`moss-watercolor: cannot read the colour "${s}"; drawing against white`); }
  return [1, 1, 1];
};
