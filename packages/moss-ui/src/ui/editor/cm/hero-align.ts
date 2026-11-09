/**
 * Where a hero's words sit, as the `align=` attribute on its opening line:
 * absent is the start, `align=end` the inline end, `align=center` the middle.
 * Only that attribute changes; everything else on the line stays as written.
 */

import type { HeroAlign } from '../session/instrument-events';
import { heroAttrs } from './hero-opener';

/** `line` with `align` as the hero's alignment; unchanged when its attributes run over several lines. */
export function heroLineWithAlign(line: string, align: HeroAlign): string {
  const h = heroAttrs(line);
  if (h.open !== null && h.close === null) return line;
  const word = align === 'start' ? null : `align=${align}`;
  const [first, ...rest] = h.attrs.filter((a) => a.key === 'align');
  if (!first && !word) return line;

  // Edit from the end, so earlier offsets stay true; a removed attribute takes its following space with it.
  let out = line;
  for (const a of rest.reverse()) out = out.slice(0, a.from) + out.slice(a.to).trimStart();
  if (!first) out = h.open === null ? `${line.trimEnd()} {${word}}` : `${out.slice(0, h.close!)} ${word}${out.slice(h.close!)}`;
  else out = out.slice(0, first.from) + (word ? word + out.slice(first.to) : out.slice(first.to).trimStart());

  const r = heroAttrs(out);
  if (r.open === null || r.close === null) return out;
  const inner = out.slice(r.open + 1, r.close).trim();
  return inner ? `${out.slice(0, r.open + 1)}${inner}${out.slice(r.close)}` : out.slice(0, r.open).trimEnd() + out.slice(r.close + 1);
}
