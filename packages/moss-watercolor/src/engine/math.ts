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
