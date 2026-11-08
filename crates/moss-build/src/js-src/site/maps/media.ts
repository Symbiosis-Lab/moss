/** What the maps' scripts ask of the page's media queries. */

/** The width from which a map has room for the panel beside it; the stylesheet's `(width < 56rem)` blocks mirror it. */
export const WIDE = "(min-width: 56rem)";

export const media = (query: string): boolean => !!window.matchMedia?.(query).matches;

export const reducedMotion = (): boolean => media("(prefers-reduced-motion: reduce)");
