/** The `key=value` attributes in a `:::hero` opening line's `{…}` block, as spans in that line. */

import { parseAttrKvSpans } from '@symbiosis-lab/moss-syntax';

export interface HeroAttr {
  key: string;
  /** The span of `key=value` in the line, quotes included. */
  from: number;
  to: number;
  /** The value, unquoted. */
  value: string;
}

export interface HeroAttrs {
  /** Where `{` and `}` are; null when there is no block, or it does not close and parse on this line. */
  open: number | null;
  close: number | null;
  attrs: HeroAttr[];
}

export function heroAttrs(line: string): HeroAttrs {
  const open = line.indexOf('{');
  if (open < 0) return { open: null, close: null, attrs: [] };
  const kv = parseAttrKvSpans(line.slice(open));
  if (!kv) return { open, close: null, attrs: [] };
  const attrs = kv.map((a) => ({
    key: a.key,
    from: open + a.keyFrom,
    to: open + a.to + (line[open + a.to] === '"' ? 1 : 0),
    value: a.value,
  }));
  return { open, close: line.indexOf('}', attrs.at(-1)?.to ?? open), attrs };
}
