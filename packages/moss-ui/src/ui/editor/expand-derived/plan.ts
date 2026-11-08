// The pure half of "Expand": which lines of a page a backend probe says would be
// replaced, and the smallest edit that applies one of those replacements.
//
// `expandDerived` returns the WHOLE page with every ready derived embed written
// out. The editor needs two things from that string, neither of which is the
// string itself: which embed lines changed (readiness), and a minimal change
// set for one line (so the caret, scroll position and the undo step stay local
// to the line the author clicked). Both come from one line diff.

export interface LineHunk {
  /** Lines `[oldFrom, oldTo)` of the old text became lines `[newFrom, newTo)` of the new. */
  oldFrom: number;
  oldTo: number;
  newFrom: number;
  newTo: number;
}

export interface TextChange {
  from: number;
  to: number;
  insert: string;
}

/** A page this large is diffed as one hunk past the prefix/suffix trim rather than in O(n*m). */
const LCS_CELL_LIMIT = 4_000_000;

/** Line-level diff of `a` to `b`: common head and tail trimmed, the middle matched by LCS. */
export function diffLines(a: readonly string[], b: readonly string[]): LineHunk[] {
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
  const aMid = a.slice(head, a.length - tail);
  const bMid = b.slice(head, b.length - tail);
  if (aMid.length === 0 && bMid.length === 0) return [];
  if (aMid.length * bMid.length > LCS_CELL_LIMIT || aMid.length === 0 || bMid.length === 0) {
    return [{ oldFrom: head, oldTo: head + aMid.length, newFrom: head, newTo: head + bMid.length }];
  }
  const w = bMid.length + 1;
  const table = new Uint32Array((aMid.length + 1) * w);
  for (let i = aMid.length - 1; i >= 0; i--) {
    for (let j = bMid.length - 1; j >= 0; j--) {
      table[i * w + j] = aMid[i] === bMid[j]
        ? table[(i + 1) * w + j + 1] + 1
        : Math.max(table[(i + 1) * w + j], table[i * w + j + 1]);
    }
  }
  const hunks: LineHunk[] = [];
  let i = 0;
  let j = 0;
  let open: LineHunk | null = null;
  const close = (): void => { if (open) { hunks.push(open); open = null; } };
  while (i < aMid.length || j < bMid.length) {
    if (i < aMid.length && j < bMid.length && aMid[i] === bMid[j]) {
      close(); i++; j++; continue;
    }
    open ??= { oldFrom: head + i, oldTo: head + i, newFrom: head + j, newTo: head + j };
    if (j >= bMid.length || (i < aMid.length && table[(i + 1) * w + j] >= table[i * w + j + 1])) {
      i++; open.oldTo = head + i;
    } else {
      j++; open.newTo = head + j;
    }
  }
  close();
  return hunks;
}

/** Which of `candidates` (0-based line indexes) fall inside a changed range of the old text. */
export function readyLines(candidates: readonly number[], hunks: readonly LineHunk[]): number[] {
  return candidates.filter((line) => hunks.some((h) => line >= h.oldFrom && line < h.oldTo));
}

/**
 * The hunks that make up the expansion of the one embed on `target`: the hunk
 * that covers it, and — only when it is the page's first ready embed — every
 * hunk before it, because that is where an inferred title or description is
 * written. A later embed never carries the front matter.
 */
export function hunksForLine(hunks: readonly LineHunk[], target: number, isFirstReady: boolean): LineHunk[] {
  const own = hunks.findIndex((h) => target >= h.oldFrom && target < h.oldTo);
  if (own < 0) return [];
  return hunks.filter((h, k) => k === own || (isFirstReady && k < own));
}

/** `hunks` as character edits over `oldLines.join('\n')`, in ascending order. */
export function changesFor(oldLines: readonly string[], newLines: readonly string[], hunks: readonly LineHunk[]): TextChange[] {
  const starts: number[] = [];
  let at = 0;
  for (const line of oldLines) { starts.push(at); at += line.length + 1; }
  const total = oldLines.join('\n').length;
  return hunks.map((h) => {
    const added = newLines.slice(h.newFrom, h.newTo);
    if (h.oldTo < oldLines.length) {
      return { from: starts[h.oldFrom], to: starts[h.oldTo], insert: added.map((l) => `${l}\n`).join('') };
    }
    // The hunk runs to the last line, which has no newline of its own.
    if (added.length === 0 && h.oldFrom > 0) return { from: starts[h.oldFrom] - 1, to: total, insert: '' };
    return { from: starts[h.oldFrom], to: total, insert: added.join('\n') };
  });
}

/** `lines` with each index in `others` swapped for an inert placeholder, plus the way back. */
export function maskLines(lines: readonly string[], others: readonly number[]): { masked: string; restore: (text: string) => string } {
  const out = lines.slice();
  const keep = new Map<string, string>();
  others.forEach((idx, n) => {
    const tag = `<!-- moss-keep-${n} -->`;
    keep.set(tag, lines[idx]);
    out[idx] = tag;
  });
  return {
    masked: out.join('\n'),
    restore: (text) => text.split('\n').map((l) => keep.get(l) ?? l).join('\n'),
  };
}

/**
 * The fragment of a `file#fragment` embed target when it names derived text,
 * else null. A markdown note's `#Heading` is a section, not a derivation.
 */
export function derivedFragmentOf(target: string): string | null {
  const hash = target.lastIndexOf('#');
  if (hash <= 0) return null;
  const file = target.slice(0, hash);
  const ext = /\.([A-Za-z0-9]+)$/.exec(file)?.[1]?.toLowerCase();
  if (!ext || ext === 'md' || ext === 'markdown') return null;
  const fragment = target.slice(hash + 1).trim();
  // `page=3`, `zoom=80`, `toolbar=0`: a viewer's parameters, not a derivation.
  if (!fragment || fragment.includes('=')) return null;
  return fragment;
}

/** Document event the file tree fires after it re-reads the folder derivations. */
export const DERIVATIONS_REFRESHED = 'moss:derivations-refreshed';
