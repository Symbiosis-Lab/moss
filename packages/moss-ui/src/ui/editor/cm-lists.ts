// List decoration: the nested-list guide (below, `listGuides`) and the wrapped-row hanging indent.
//
// Wrapped-row hanging indent for list items: every visual row of an item's text
// starts at the item's text column (after `- `, `1. `, `- [ ] ` and its indent),
// so wrapped text never sits left of its own marker and the nested-list guide
// (cm-markdown-style.css `.cm-lp-guided::after`) runs in clear space.
//
// The mechanism, in the order the options were tried:
//   - never negative `text-indent`: CodeMirror reads the first rendered line's
//     text-indent for selection rectangles, which breaks them after a scroll.
//   - not `padding-left: W` either: the same code reads that line's padding-left
//     as the left edge of every fully selected line, so a hung first line would
//     shift the selection of all the plain lines under it. The line gets
//     `margin-left: W` instead (CM does not read margins). Not a border: Chromium
//     snaps borders to whole device pixels, up to 1px short of W.
//   - a zero-width box (`cm-li-shim`) at the start of the item's prefix (indent +
//     marker) with inline `margin-left: -W`, so on the first row it sits where it always did and the
//     text follows it, while wrapped rows start at the margin's edge. W is the
//     prefix's REAL rendered width: no CSS unit knows it in a proportional
//     font, so a view plugin measures each distinct prefix once and writes it
//     as a custom property (`--lpw-<key>`) on the content element. The
//     decorations only name the property, so a measurement never rebuilds them.
//
// Presentation stays a pure function of the document, never of the selection.

import { type EditorState, type Extension, type Line } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { Decoration, EditorView, ViewPlugin, WidgetType, type ViewUpdate } from '@codemirror/view';

/** A syntax-tree node, and a decoration with its range: the shapes every decoration builder here shares (cm-markdown-style.ts re-exports the node type). */
export type Tree = ReturnType<typeof syntaxTree>;
export type SyntaxNode = ReturnType<Tree['resolveInner']>;
export interface DecoRange { from: number; to: number; deco: Decoration }

/** Characters of a line's container prefix (`> ` per enclosing quote): list columns count from after it. */
export const prefixLength = (text: string): number => /^(?: {0,3}>[ \t]?)*/.exec(text)![0].length;

/** An item's first-line prefix: indent, marker, the spaces after it, and a task's `[ ] `. */
const ITEM_PREFIX = /^[ \t]*(?:[-*+]|\d{1,9}[.)])(?:[ \t]+(?:\[[ xX]\](?=[ \t]|$)[ \t]*)?)?/;

/**
 * A prefix is measured per font: a heading line (`- # Title`) sets a larger
 * font-size on the whole line (`.cm-lp-h<n>`), prefix included, so the same
 * `- ` is a different width there. The key is the prefix, plus `#h<n>` on such
 * a line; everything else in a list line (quote, task, emphasis) leaves the font alone.
 */
const HEADING = /^ATXHeading(\d)$/;
function keyOf(state: EditorState, line: { from: number; number: number }, prefix: string, textStart: number): string {
  for (let n: SyntaxNode | null = syntaxTree(state).resolveInner(textStart, 1); n; n = n.parent) {
    const m = HEADING.exec(n.name);
    if (m && state.doc.lineAt(n.from).number === line.number) return `${prefix}#h${m[1]}`;
  }
  return prefix;
}
/** A key as a custom-property name: its characters' code points, so any text is a valid identifier. */
const propOf = (key: string): string => `--lpw-${[...key].map((c) => c.codePointAt(0)!.toString(36)).join('_')}`;
/** Before the first measure (and in a test without layout) a prefix is its length in 0.6em: close enough that nothing visibly jumps. */
const guess = (prefix: string): string => `${(0.6 * prefix.length).toFixed(1)}em`;
const widthOf = (key: string): string => `var(${propOf(key)},${guess(key.split('#')[0])})`;

const decoCache = new Map<string, Decoration>();
const cached = (id: string, make: () => Decoration): Decoration => {
  let d = decoCache.get(id);
  if (!d) {
    if (decoCache.size > 400) decoCache.clear();
    decoCache.set(id, d = make());
  }
  return d;
};
/** The line holding the item's text column, as the width of `column`'s prefix; a continuation line also names its own leading whitespace, which the shim pulls back by (never more than the column). */
const lineHang = (column: string, own?: string) => cached(`L${column}\0${own ?? ''}`, () => Decoration.line({
  class: 'cm-li-hang',
  attributes: { style: `--lp-hang:${widthOf(column)}${own ? `;--lp-pfx:${widthOf(own)}` : ''}` },
}));
/** Names a prefix to be measured; it carries no style, so it can never move a caret position. */
const markPrefix = (key: string) => cached(`M${key}`, () => Decoration.mark({ class: 'cm-li-pfx', attributes: { 'data-lpk': key } }));
/**
 * A tab in a prefix, fixed to the `n` space widths it spans from its column. A
 * native tab runs to the next stop counted from the line box's left edge, and
 * the shim moves the prefix to -W, so the same prefix would end on a different
 * stop than the one that was measured. As an inline-block with `tab-size: n` the
 * tab's width is the same wherever it starts (CommonMark's 4-column stops, which
 * the guide's marker columns also count).
 */
const markTab = (n: number, indent: boolean) => cached(`T${n}${indent}`, () => Decoration.mark({
  class: 'cm-li-tab',
  // `tab-size` leaves letter-spacing out, and engines differ on whether the tab's own glyph takes it (`.cm-li-tab .cm-li-ind` zeroes it): the n columns' extra comes as a margin instead (see INDENT).
  attributes: { style: `tab-size:${n}${indent ? `;margin-right:calc(${n} * var(--moss-list-indent))` : ''}` },
}));

/**
 * The pull-back: a zero-width inline box at the start of the prefix with a
 * negative `margin-left` (cm-markdown-style.css `.cm-li-shim`). It is a widget
 * rather than a margin on the prefix mark because a caret at the very start of
 * a margined span is measured before the margin, so the same position was drawn
 * a whole prefix apart depending on the key that reached it. After the box the
 * browser has no such ambiguity.
 */
class Shim extends WidgetType {
  eq() { return true; }
  toDOM() { const el = document.createElement('span'); el.className = 'cm-li-shim'; el.setAttribute('aria-hidden', 'true'); return el; }
}
const SHIM = Decoration.widget({ widget: new Shim(), side: -1 });

/**
 * The indentation before an item's marker, rendered wider (`--moss-list-indent`
 * of extra `letter-spacing` per space, cm-markdown-style.css `.cm-li-ind`) so a
 * nesting level reads as deeper than its 2 or 3 source spaces. Purely visual:
 * the characters are untouched and every one is still a caret position. It sits
 * inside the measured prefix, so the hang and the guides (whose column box
 * carries the same letter-spacing) follow it with no second measurer. Not
 * `word-spacing`: WebKit then reports a space's rectangle one extra to the right
 * of where the next character's starts, so the same caret position is drawn at
 * two x depending on the arrow that reached it. A continuation line's own
 * whitespace is left alone: it hangs to the item's column, which is already
 * as wide as the item's text starts.
 */
const INDENT = Decoration.mark({ class: 'cm-li-ind' });

/** The hang of one list item: its first line and every continuation line that is not a nested item's own. */
export function listHang(item: SyntaxNode, state: EditorState, from: number, to: number, decos: DecoRange[]): void {
  const mark = item.firstChild;
  if (!mark) return;
  const first = state.doc.lineAt(mark.from);
  const rest = first.text.slice(prefixLength(first.text));
  const lead = /^[ \t]*/.exec(rest)![0];
  if (mark.from - first.from !== prefixLength(first.text) + lead.length) return; // `- - a`: the inner marker is not at the line start; the outer item owns the line
  const prefix = ITEM_PREFIX.exec(rest)?.[0];
  if (!prefix) return;
  const base = first.from + prefixLength(first.text);
  const column = keyOf(state, first, prefix, base + prefix.length);
  const skip: [number, number][] = [];
  for (let c = item.firstChild; c; c = c.nextSibling) if (c.name === 'BulletList' || c.name === 'OrderedList') skip.push([state.doc.lineAt(c.from).number, state.doc.lineAt(c.to).number]);
  const lo = state.doc.lineAt(from).number, hi = state.doc.lineAt(to).number;
  for (let ln = first.number; ln <= state.doc.lineAt(item.to).number; ln++) {
    if (ln < lo || ln > hi || (ln !== first.number && skip.some(([a, b]) => ln >= a && ln <= b))) continue;
    const line = state.doc.line(ln), isFirst = ln === first.number;
    const body = isFirst ? '' : line.text.slice(prefixLength(line.text));
    if (!isFirst && !body.trim()) continue; // a blank line holds no text to align
    const at = isFirst ? base : line.from + prefixLength(line.text);
    const text = isFirst ? prefix : /^[ \t]*/.exec(body)![0];
    const own = text && keyOf(state, line, text, at + text.length);
    decos.push({ from: line.from, to: line.from, deco: lineHang(column, isFirst ? undefined : own || undefined) });
    if (!text) continue;
    const leadEnd = isFirst ? lead.length : 0; // the widened indent: only a first line's, before its marker
    decos.push({ from: at, to: at, deco: SHIM }, { from: at, to: at + text.length, deco: markPrefix(own) });
    if (leadEnd) decos.push({ from: at, to: at + leadEnd, deco: INDENT });
    for (let i = 0, col = 0; i < text.length; i++) {
      const tab = text[i] === '\t', n = tab ? 4 - (col % 4) : 1;
      if (tab) decos.push({ from: at + i, to: at + i + 1, deco: markTab(n, i < leadEnd) });
      col += n;
    }
  }
}

/** Width of `text` in columns when it starts at column `from`: a tab runs to the next stop of 4 (CommonMark), anything else is one column. */
export const columnsOf = (text: string, from = 0): number => [...text].reduce((col, ch) => (ch === '\t' ? col + 4 - (col % 4) : col + 1), from) - from;

/** Where a line's list indentation sits: `at` is its first character past the quote prefix, `chars` its length in characters (spaces and tabs). */
export const indentOf = (line: Line): { at: number; chars: number } => {
  const at = line.from + prefixLength(line.text);
  return { at, chars: /^[ \t]*/.exec(line.text.slice(at - line.from))![0].length };
};

/** The innermost list item a non-blank line belongs to (its own, a continuation's or a lazy line's), or null: the one lookup Tab and Alt-↑/↓ share. */
export function itemOnLine(tree: Tree, line: Line): SyntaxNode | null {
  const { at, chars } = indentOf(line);
  for (let node: SyntaxNode | null = tree.resolveInner(at + chars, 1); node; node = node.parent) if (node.name === 'ListItem') return node;
  return null;
}

/** The column of a list item's marker, counted from after the line's quote prefix. A nested item's node begins past its parent's content, so the marker, not the item, is what is measured. */
export function markerColumn(item: SyntaxNode, state: EditorState): number {
  const line = state.doc.lineAt(item.firstChild!.from);
  return columnsOf(line.text.slice(prefixLength(line.text), item.firstChild!.from - line.from));
}

/** Slots a line can hold a guide in; `.cm-lp-guided::after` in the stylesheet draws exactly this many (a test reads the stylesheet and fails if they differ). */
export const GUIDE_SLOTS = 5;

/**
 * The guide of a nested list: a thin line under a parent's marker, from the
 * bottom of the parent's line down through its block, to the top of the next
 * item at the parent's level. A blank line ends the block and the guide with
 * it, so items separated by one are never joined. It is a LINE decoration on
 * every line of that run, lazy-continuation lines included. Each parent writes its marker column into
 * the slot of its own depth (`--lp-g0` for a top-level parent, `--lp-g1` for its
 * child ...): CodeMirror joins the `style` of line decorations on one line, so a
 * grandchild's line carries one slot per ancestor. The stylesheet turns columns
 * into pixels, which a decoration cannot know in a proportional font.
 */
function listGuides(parent: SyntaxNode, state: EditorState, from: number, to: number, decos: DecoRange[]): void {
  let nested = false;
  for (let c = parent.firstChild; c && !nested; c = c.nextSibling) nested = c.name === 'BulletList' || c.name === 'OrderedList';
  if (!nested) return;
  let depth = 0;
  for (let p = parent.parent; p; p = p.parent) if (p.name === 'ListItem') depth++;
  if (depth >= GUIDE_SLOTS) return;
  const column = markerColumn(parent, state);
  const deco = cached(`G${depth}:${column}`, () => Decoration.line({ class: 'cm-lp-guided', attributes: { style: `--lp-g${depth}:${column}` } }));
  const first = state.doc.lineAt(parent.from).number + 1, last = state.doc.lineAt(parent.to).number;
  let end = first - 1;
  while (end < last && state.doc.line(end + 1).text.trim()) end++;
  const lo = Math.max(first, state.doc.lineAt(from).number), hi = Math.min(end, state.doc.lineAt(to).number);
  for (let ln = lo; ln <= hi; ln++) decos.push({ from: state.doc.line(ln).from, to: state.doc.line(ln).from, deco });
}

/** All the decoration a list item owns: its guide (if it has nested children) and its hang. */
export function listDecorations(item: SyntaxNode, state: EditorState, from: number, to: number, decos: DecoRange[]): void {
  listGuides(item, state, from, to, decos);
  listHang(item, state, from, to, decos);
}

// ── Measuring ────────────────────────────────────────────────────────────

/** Measure cycles a key may go unseen (scrolled out of the viewport) before its property is dropped; coming back inside this window costs no re-measure. */
const IDLE_CYCLES = 40;

/** What the view plugin has measured: key -> cycles since a line using it was rendered. The widths themselves live in the custom properties on the content element, valid for one font. */
const listHangMeasure = ViewPlugin.fromClass(
  class {
    private idle = new Map<string, number>();
    private font = '';
    private readonly refresh = () => { this.font = ''; this.view.requestMeasure(this.request); };
    private readonly request = {
      key: this,
      read: (view: EditorView) => {
        const cs = getComputedStyle(view.contentDOM);
        const font = [cs.fontFamily, cs.fontSize, cs.fontWeight, cs.fontStyle, cs.letterSpacing].join('|');
        const stale = font !== this.font;
        const found = new Map<string, number>(), seen = new Set<string>();
        for (const el of view.contentDOM.querySelectorAll<HTMLElement>('.cm-li-pfx')) {
          const key = el.dataset.lpk!;
          seen.add(key);
          // Skip a key already written, unless the property is gone (CodeMirror may rewrite the content element's style).
          if (found.has(key) || (!stale && view.contentDOM.style.getPropertyValue(propOf(key)))) continue;
          // The browser may report one box per fragment (the marker, the space); a prefix split across rows has no width.
          const rects = Array.from(el.getClientRects());
          if (rects.length && rects.every((r) => Math.abs(r.top - rects[0].top) < 2)) found.set(key, Math.max(...rects.map((r) => r.right)) - Math.min(...rects.map((r) => r.left)));
        }
        return { font, stale, found, seen };
      },
      write: ({ font, stale, found, seen }: { font: string; stale: boolean; found: Map<string, number>; seen: Set<string> }, view: EditorView) => {
        const style = view.contentDOM.style;
        if (stale) {
          for (const key of this.idle.keys()) style.removeProperty(propOf(key));
          this.idle.clear();
          this.font = font;
        }
        for (const [key, w] of found) {
          // Whole pixels: WebKit places a fractionally pulled-back run up to 1px too far left (its character rects read that far off the text), and a fraction here would buy at most half a pixel of exactness.
          style.setProperty(propOf(key), `${Math.round(w)}px`);
          this.idle.set(key, 0);
        }
        for (const [key, n] of this.idle) {
          if (seen.has(key)) this.idle.set(key, 0);
          else if (n >= IDLE_CYCLES) { style.removeProperty(propOf(key)); this.idle.delete(key); }
          else this.idle.set(key, n + 1);
        }
        // Row counts may have changed under the new widths; CM re-reads heights once.
        if (found.size) view.requestMeasure();
      },
    };
    constructor(private readonly view: EditorView) {
      view.requestMeasure(this.request);
      document.fonts?.addEventListener('loadingdone', this.refresh);
    }
    update(u: ViewUpdate) {
      // The tree advancing (the parser works in chunks) is what decorates a list that was below the parsed part.
      if (u.docChanged || u.viewportChanged || u.geometryChanged || syntaxTree(u.state) !== syntaxTree(u.startState)) u.view.requestMeasure(this.request);
    }
    destroy() {
      document.fonts?.removeEventListener('loadingdone', this.refresh);
    }
  },
);

export const listHangExtension = (): Extension => listHangMeasure;
