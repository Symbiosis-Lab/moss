// What is a hang: the one reader of the margin's indicators, shared by the
// decorations and atomic ranges (cm-markdown-style.ts), the caret rules
// (cm-head-fit.ts) and the line-type keys (cm-format-keys.ts).

import type { EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';

// ── What is a hang ───────────────────────────────────────────────────────
//
// One owner: `hangsIn(state, from, to)` reads the hung prefixes of the lines in
// a range from the syntax tree, and everything that cares reads it — the
// `cm-hang` decorations and the atomic caret ranges (both built for the rendered
// viewport), and the caret rules (cm-head-fit.ts; one line at a time, so a keystroke
// never walks the whole document).

/** A hung prefix. `from`/`to` is what is drawn into the margin; `atomFrom`
 * is where the one caret step over it begins — the line start when only
 * indentation precedes the prefix (`  - item`: indent + marker are one step,
 * while the marker alone hangs and the text keeps its indent). */
export interface Hang {
  from: number;
  to: number;
  atomFrom: number;
  /** What the indicator says about its line, and how deep: a heading's `#` count, a quote's `>` count. */
  kind: 'heading' | 'quote';
  level: number;
}

const isHeading = (name: string): boolean => /^ATXHeading[1-6]$/.test(name);

/**
 * The hung indicators of the lines in `[from, to]`, from the syntax tree: a
 * heading's `#` run and a quote's `>` run, in document order. Nested
 * `QuoteMark`s on one line concatenate into one contiguous run (`> > x` hangs
 * "> > " as one span) rather than overlapping — a mark only extends the run when
 * it starts exactly where the previous one on that line ended, which
 * `iterate`'s top-down visit guarantees lands in nesting order. A heading inside
 * a quote (`> ## x`) is a second hang on the same line, after the quote's.
 * Nothing else hangs: a list marker, a fence or a footnote definition stays in
 * the text column. `from`/`to` must be line boundaries.
 */
export function hangsIn(state: EditorState, from: number, to: number): Hang[] {
  const out: Hang[] = [];
  const runs = new Map<number, { from: number; to: number }>();
  const span = (at: number, end: number, kind: Hang['kind'], level: number) => {
    const line = state.doc.lineAt(at);
    const indentOnly = state.doc.sliceString(line.from, at).trim() === '';
    out.push({ from: at, to: end, atomFrom: indentOnly ? line.from : at, kind, level });
  };
  syntaxTree(state).iterate({
    from,
    to,
    enter(node) {
      if (node.name === 'QuoteMark') {
        if (node.from < from || node.from > to) return;
        const line = state.doc.lineAt(node.from);
        const run = runs.get(line.number);
        if (run ? node.from !== run.to : state.doc.sliceString(line.from, node.from).trim() !== '') return;
        let end = node.to;
        if (state.doc.sliceString(end, end + 1) === ' ') end++;
        runs.set(line.number, { from: run?.from ?? node.from, to: end });
        return;
      }
      if (isHeading(node.name)) {
        const cur = node.node.cursor();
        if (cur.firstChild() && cur.name === 'HeaderMark') {
          span(cur.from, state.doc.sliceString(cur.to, cur.to + 1) === ' ' ? cur.to + 1 : cur.to, 'heading', cur.to - cur.from);
        }
        return false;
      }
    },
  });
  for (const run of runs.values()) span(run.from, run.to, 'quote', (state.doc.sliceString(run.from, run.to).match(/>/g) ?? []).length);
  return out.sort((a, b) => a.from - b.from);
}

/** The hung indicators on the line holding `pos` — decided from that line alone. */
export function hangsOnLine(state: EditorState, pos: number): Hang[] {
  const line = state.doc.lineAt(pos);
  return hangsIn(state, line.from, line.to);
}

/** Where the text of a hung line starts (the end of its last indicator), or -1 when nothing hangs on it. */
export const textStartOf = (hangs: readonly Hang[]): number => hangs.length ? hangs[hangs.length - 1].to : -1;

/** The hangs of every line in the given ranges, widened to whole lines. */
export function hangsInRanges(state: EditorState, ranges: readonly { from: number; to: number }[]): Hang[] {
  return ranges.flatMap((r) => hangsIn(state, state.doc.lineAt(r.from).from, state.doc.lineAt(r.to).to));
}

/** The indicator whose text starts at `pos` (the caret at the first character of a heading's or quote's text), if any. */
export function indicatorEndingAt(state: EditorState, pos: number): Hang | null {
  const hangs = hangsOnLine(state, pos);
  return textStartOf(hangs) === pos ? hangs[hangs.length - 1] : null;
}
