/** A country's level name from the legend, or the "Not rated" words. */
export const levelName = (path: SVGPathElement | undefined, levels: string[], notRated: string): string =>
  path?.dataset.level ? levels[+path.dataset.level - 1] : notRated;
