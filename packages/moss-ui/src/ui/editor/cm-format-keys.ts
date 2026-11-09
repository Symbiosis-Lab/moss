// Keyboard shortcuts for inline markdown formatting in the CM6 editor.
// Provides Cmd+B (bold), Cmd+I (italic), Cmd+Shift+S (strikethrough),
// Cmd+~ (inline code), Cmd+K (link) — all with smart toggle-off.
//
// Key strings use CM6's "Mod-" prefix (Meta on Mac, Ctrl on Win/Linux).
// Inline code uses "Mod-~" (event.key for Shift+Backtick on US layout)
// rather than "Mod-Shift-`" to work across keyboard layouts.
//
// The same module owns the line-type operations: `setLineType` (which the heading
// and quote toggles below call), Backspace on an indicator, and the indicator's
// click menu (see "Line type" below), and Tab / Shift-Tab on a list item (see
// "List indent"), and Alt-↑/↓ (move the block under the caret, see "Block move").

import { EditorView, keymap } from '@codemirror/view';
import { EditorSelection, Prec, type ChangeSpec, type EditorState, type Extension } from '@codemirror/state';
import { isolateHistory } from '@codemirror/commands';
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { t } from "../i18n/index";
import { openCtxMenu, type CtxMenuEntry } from "../shared/ui/ctx-menu";
import { hangsOnLine, indicatorEndingAt, type Hang } from './cm-hangs';
import type { SyntaxNode } from './cm-markdown-style';
import { columnsOf, indentOf, itemOnLine, markerColumn, type Tree } from './cm-lists';
import { wrapSelection, detectToggle } from './selection-toolbar';
import type { FormatAction } from './selection-toolbar';

/** No-selection insert: marker pairs and cursor offsets. */
const INSERT_MAP: Record<FormatAction, { text: string; cursorOffset: number }> = {
  bold:   { text: '****',  cursorOffset: 2 },
  italic: { text: '**',   cursorOffset: 1 },
  strike: { text: '~~~~', cursorOffset: 2 },
  code:   { text: '``',   cursorOffset: 1 },
  link:   { text: '[]()', cursorOffset: 1 },
};

/**
 * If `selected` itself INCLUDES the marker pair for `action`, return the inner
 * text — selecting `**bold**` whole and pressing Mod-b must unwrap, not
 * double-wrap. Returns null when the selection isn't marker-inclusive.
 */
export function inclusiveInner(selected: string, action: FormatAction): string | null {
  if (action === 'link') {
    const m = /^\[([^\]]*)\]\([^)]*\)$/.exec(selected);
    return m ? m[1] : null;
  }
  const markerMap: Record<Exclude<FormatAction, 'link'>, string> = {
    bold: '**', italic: '*', strike: '~~', code: '`',
  };
  const marker = markerMap[action];
  const mLen = marker.length;
  if (selected.length < mLen * 2) return null;
  if (!selected.startsWith(marker) || !selected.endsWith(marker)) return null;
  // Italic on a `**bold**` selection is a bold context, not italic — fall
  // through to wrap (producing ***bold-italic***), mirroring detectToggle.
  if (marker === '*' && (selected.startsWith('**') || selected.endsWith('**'))) return null;
  return selected.slice(mLen, selected.length - mLen);
}

/**
 * The ONE inline-format engine: smart toggle-off, word-expansion at a
 * collapsed caret, marker-pair insert, marker-inclusive unwrap. Exported so
 * the insert bar's format buttons dispatch the exact same behavior as ⌘B —
 * the two surfaces can never diverge.
 */
export function formatMark(view: EditorView, action: FormatAction): boolean {
  const { from, to } = view.state.selection.main;

  if (from === to) {
    // Collapsed cursor inside a word: expand to the word and toggle it —
    // Cmd+B mid-word must not inject a bare `****` (iA/Obsidian convention).
    // Link keeps the insert-pair behavior (no word-expansion for Cmd+K).
    const word = action !== 'link' ? view.state.wordAt(from) : null;
    if (word) {
      const toggle = detectToggle(view.state.doc, word.from, word.to, action);
      if (toggle.isToggle) {
        const inner = view.state.doc.sliceString(word.from, word.to);
        const openLen = word.from - toggle.removeFrom;
        view.dispatch({
          changes: { from: toggle.removeFrom, to: toggle.removeTo, insert: inner },
          selection: { anchor: Math.max(toggle.removeFrom, from - openLen) },
        });
        return true;
      }
      const wordText = view.state.doc.sliceString(word.from, word.to);
      const wrapped = wrapSelection(wordText, action);
      const openLen = (wrapped.length - wordText.length) / 2;
      view.dispatch({
        changes: { from: word.from, to: word.to, insert: wrapped },
        selection: { anchor: from + openLen }, // caret keeps its text position
      });
      return true;
    }

    // No word at the cursor — insert marker pair with cursor inside.
    const { text, cursorOffset } = INSERT_MAP[action];
    view.dispatch({
      changes: { from, to, insert: text },
      selection: { anchor: from + cursorOffset },
    });
    return true;
  }

  const selected = view.state.doc.sliceString(from, to);

  // Selection INCLUDES the markers (`**bold**` selected whole) → unwrap.
  const inner = inclusiveInner(selected, action);
  if (inner !== null) {
    view.dispatch({
      changes: { from, to, insert: inner },
      selection: { anchor: from + inner.length },
    });
    return true;
  }

  const toggle = detectToggle(view.state.doc, from, to, action);
  if (toggle.isToggle) {
    // Toggle-off: remove surrounding markers, keep inner text.
    view.dispatch({
      changes: { from: toggle.removeFrom, to: toggle.removeTo, insert: selected },
      selection: { anchor: toggle.removeFrom + selected.length },
    });
    return true;
  }

  // Wrap selection with markers. For a link, land the cursor INSIDE the `()`
  // — the URL is what the user types next.
  const wrapped = wrapSelection(selected, action);
  const anchor = action === 'link' ? from + wrapped.length - 1 : from + wrapped.length;
  view.dispatch({
    changes: { from, to, insert: wrapped },
    selection: { anchor },
  });
  return true;
}

/**
 * Toggle the caret's line as a heading of `level` (H2 by default — the insert
 * bar's format-page counterpart to the empty-line page's H2 chip). A line that
 * is already that level loses the heading; any other heading level is replaced,
 * never stacked to `## # Title`. A heading inside a quote (`> ## x`) keeps its
 * `> `. Operates on the caret's line only — bound to Mod-Alt-1…6, so a
 * multi-line selection is out of scope. The prefix rewrite itself is `setLineType`.
 */
export function toggleHeading(view: EditorView, level = 2): boolean {
  const lineFrom = view.state.doc.lineAt(view.state.selection.main.head).from;
  const h = hangsOnLine(view.state, lineFrom).find((x) => x.kind === 'heading');
  if (h?.level === level) setLineType(view, lineFrom, { kind: 'paragraph' }, h);
  else setLineType(view, lineFrom, { kind: 'heading', level });
  view.focus();
  return true;
}

/**
 * Toggle the caret's line as a `> ` blockquote — the format-page counterpart
 * to the insert page's "bq" chip. Quoting a heading keeps the heading
 * (`## x` → `> ## x`); un-quoting a quoted heading keeps the heading. Operates
 * on the caret's line only, matching toggleHeading's scope.
 */
export function toggleBlockquote(view: EditorView): boolean {
  const lineFrom = view.state.doc.lineAt(view.state.selection.main.head).from;
  const q = hangsOnLine(view.state, lineFrom).find((x) => x.kind === 'quote');
  if (q) setLineType(view, lineFrom, { kind: 'paragraph' }, q);
  else setLineType(view, lineFrom, { kind: 'quote' });
  view.focus();
  return true;
}

export function formatKeysExtension(): Extension {
  return [blockMoveKeymap(),
    // Above minimalSetup's defaultKeymap, whose Mod-i (selectParentSyntax) runs
    // first at equal precedence and almost always succeeds.
    Prec.high(keymap.of([{ key: 'Mod-i', run: (view) => formatMark(view, 'italic') }])),
    keymap.of([
    { key: 'Tab', run: (view) => indentListItems(view), shift: (view) => indentListItems(view, true) },
    { key: 'Mod-b',       run: (view) => formatMark(view, 'bold') },
    { key: 'Mod-Shift-s', run: (view) => formatMark(view, 'strike') },
    { key: 'Mod-~',       run: (view) => formatMark(view, 'code') },
    { key: 'Mod-k',       run: (view) => formatMark(view, 'link') },
    // Mod-Shift-9: Notion's blockquote binding — the closest thing to a
    // cross-app convention for this action (Bear/Typora diverge on the key).
    { key: 'Mod-Shift-9', run: (view) => toggleBlockquote(view) },
    // Mod-Alt-1…6: Typora's Cmd+N (heading N) with Alt added to dodge the
    // Cmd+N tab-switch collision Bear's Cmd+Alt+N scheme works around; the
    // same line-type choices the indicator menu offers (cm-format-keys.ts).
    // Known caveat: on Windows/Linux this is Ctrl+Alt+N, which AltGr keyboard
    // layouts (German, French, Nordic, Polish, ...) intercept as a character
    // composition key rather than passing it through — the format-page button
    // and the indicator menu are the fallback.
    ...[1, 2, 3, 4, 5, 6].map((level) => ({ key: `Mod-Alt-${level}`, run: (view: EditorView) => toggleHeading(view, level) })),
  ])];
}

// ── List indent ────────────────────────────────────────────────────────────
// Tab and Shift-Tab on a list item: indent it one level, or outdent it, with its
// nested children and continuation lines moving along.
//
// The editor never binds Tab: nothing in minimalSetup/defaultKeymap does.
// Tab on a list item is handled by `indentBulletList`, which works on the
// syntax tree instead of a line regex, so it also handles `1.`, tasks, several
// selected items and a nested child under its parent.
//
// What counts as one level is what pulldown-cmark (the published page's parser)
// nests: a child belongs to the item above it when its marker starts at or past that
// item's CONTENT column, so an item moves in by its previous sibling's marker width
// (2 for `- `, 3 for `1. `, 4 for `10. `) and out by the distance to its parent's
// marker. The first item of a list has no sibling to nest under, so Tab leaves it be.
//
// Tab outside a list is not bound here and does what it always did (moves focus).
// Inside a list it is captured, so keyboard users leave with CM6's own Escape-then-Tab
// (view.inputState.tabFocusMode), which the Tab binding below does not run for.

// A parse that does not finish in 100ms leaves a partial tree: lines past its edge resolve to nothing, so the key does nothing there rather than waiting on a long document.
const treeUpTo = (state: EditorState, upto: number): Tree => ensureSyntaxTree(state, upto, 100) ?? syntaxTree(state);

/** Columns from an item's marker to its first content character (`- ` is 2, `1. ` is 3). */
function contentOffset(item: SyntaxNode, view: EditorView): number {
  const mark = item.firstChild!;
  const line = view.state.doc.lineAt(mark.from);
  const gap = /^[ ]{0,4}/.exec(line.text.slice(mark.to - line.from))![0].length;
  return mark.to - mark.from + Math.max(gap, 1);
}

/** The item above `item` in the same list; a quote's `>` between two lines of one list is a sibling node to skip. */
function itemBefore(item: SyntaxNode): SyntaxNode | null {
  let before = item.prevSibling;
  while (before?.name === 'QuoteMark') before = before.prevSibling;
  return before?.name === 'ListItem' ? before : null;
}

/** The innermost list item each non-blank selected line belongs to, document order. One range: the editor never allows several (allowMultipleSelections is not set). */
function itemsOnSelection(view: EditorView): SyntaxNode[] {
  const { state } = view;
  const range = state.selection.main;
  const tree = treeUpTo(state, range.to);
  const found = new Map<number, SyntaxNode>();
  for (let n = state.doc.lineAt(range.from).number; n <= state.doc.lineAt(range.to).number; n++) {
    const line = state.doc.line(n);
    if (!line.text.slice(indentOf(line).at - line.from).trim()) continue;
    const item = itemOnLine(tree, line);
    if (item) found.set(item.from, item);
  }
  return [...found.values()].sort((a, b) => a.from - b.from);
}

/**
 * Indent (or, with `outdent`, outdent) every selected list item by one level. Returns
 * true when the selection touches a list item, so Tab is consumed there even when the
 * item cannot move (the first item of a list, a top-level item on Shift-Tab).
 */
export function indentListItems(view: EditorView, outdent = false): boolean {
  if (view.state.readOnly) return false;
  const items = itemsOnSelection(view);
  if (!items.length) return false;
  const { doc } = view.state;

  /** line number -> columns to add (positive) or remove (negative); the outermost moving item wins, so a child selected with its parent moves once, with it. */
  const shift = new Map<number, number>();
  for (const item of items) {
    let by = 0;
    if (outdent) {
      const parent = item.parent?.parent;
      if (parent?.name !== 'ListItem') continue;
      by = -Math.max(markerColumn(item, view.state) - markerColumn(parent, view.state), 1);
    } else {
      const before = itemBefore(item);
      if (!before) continue;
      by = contentOffset(before, view);
    }
    for (let n = doc.lineAt(item.from).number; n <= doc.lineAt(item.to).number; n++) if (!shift.has(n)) shift.set(n, by);
  }

  const changes: ChangeSpec[] = [];
  for (const [n, by] of shift) {
    const line = doc.line(n);
    const { at, chars } = indentOf(line);
    if (by > 0) {
      // After the existing indentation, so a tab keeps its stop and the line gains exactly `by` columns.
      if (line.text.slice(at - line.from).trim()) changes.push({ from: at + chars, insert: ' '.repeat(by) });
    } else if (chars) {
      // Take characters off the END of the indentation (what Tab added last); a tab that overshoots is traded for the spaces it leaves.
      const ws = line.text.slice(at - line.from, at - line.from + chars);
      let end = chars;
      while (end > 0 && columnsOf(ws) - columnsOf(ws.slice(0, end)) < -by) end--;
      const over = columnsOf(ws) - columnsOf(ws.slice(0, end)) + by;
      changes.push({ from: at + end, to: at + chars, insert: ' '.repeat(Math.max(0, over)) });
    }
  }
  if (changes.length) {
    const set = view.state.changes(changes);
    // Its own undo step: history would otherwise join an `input.*` event to typing within 500ms, and one undo would take the typed character with the indent.
    view.dispatch({ changes: set, selection: view.state.selection.map(set, 1), userEvent: outdent ? 'delete.dedent' : 'input.indent', annotations: isolateHistory.of('full'), scrollIntoView: true });
  }
  return true;
}

// ── Block move ─────────────────────────────────────────────────────────────
// Alt-↑ / Alt-↓: move the block under the caret, with its children, past its
// neighbour at the same level (the outliner convention: Obsidian's Outliner
// plugin, Logseq, Workflowy). Without this, `defaultKeymap`'s moveLineUp/Down
// moved a single line, which tears an item from its children and a fence from
// its closing line.
//
// `blockUnitAt` is the one definition of "the block": whole lines, the innermost
// unit containing the caret.
//   - a list item: its line, its wrapped and lazy lines and every nested child;
//     a block inside an item (a fence, a quote) is part of the item;
//   - a heading line: the heading and its section, up to the next heading of the
//     same or a higher level;
//   - any other top-level block (paragraph, fenced code, `$$`, quote or callout,
//     table, rule, ...): its own lines.
// Its siblings are the units one level up the same parent: the items of one list,
// the sections of one heading level under the same parent heading, the blocks of
// one section. A block never crosses a heading and an item never leaves its list,
// so a move cannot change anything's depth.

/** Whole lines: `from` is a line start, `to` a line end. */
export interface Span { from: number; to: number }

/** A unit and the sibling run it sits in; `key` names the run so two units can be told to share it. */
interface Level { key: string; siblings: Span[]; at: number }

/** The document's top-level blocks and headings, read once per key press. */
interface Blocks { state: EditorState; tree: Tree; kids: SyntaxNode[]; heads: { i: number; level: number }[] }

const HEADING = /^(?:ATX|Setext)Heading(\d)$/;
const headingLevel = (c: SyntaxNode): number => Number(HEADING.exec(c.name)?.[1] ?? 0);

const childrenOf = (node: SyntaxNode): SyntaxNode[] => {
  const out: SyntaxNode[] = [];
  for (let c = node.firstChild; c; c = c.nextSibling) out.push(c);
  return out;
};

/** The lines a node covers (a node can end at the newline after its last line). */
const linesOf = (state: EditorState, node: SyntaxNode): Span => ({
  from: state.doc.lineAt(node.from).from,
  to: state.doc.lineAt(Math.max(node.from, node.to - 1)).to,
});

function blocksOf(state: EditorState): Blocks {
  const tree = treeUpTo(state, state.doc.length);
  const kids = childrenOf(tree.topNode);
  return { state, tree, kids, heads: kids.map((c, i) => ({ i, level: headingLevel(c) })).filter((h) => h.level) };
}

function levelOf({ state, tree, kids, heads }: Blocks, lineFrom: number): Level | null {
  const line = state.doc.lineAt(lineFrom);
  const probe = indentOf(line);
  if (!line.text.slice(probe.at - line.from).trim()) return null; // a blank line is nobody's

  const item = itemOnLine(tree, line);
  if (item) {
    const list = item.parent!;
    const items = childrenOf(list).filter((c) => c.name === 'ListItem');
    return { key: `L${list.from}`, siblings: items.map((c) => linesOf(state, c)), at: items.findIndex((c) => c.from === item.from) };
  }

  let found: SyntaxNode | null = tree.resolveInner(probe.at + probe.chars, 1);
  while (found && found.parent?.parent) found = found.parent; // the child of the Document
  if (!found || found.name === 'Document') return null;
  const block: SyntaxNode = found;
  const k = kids.findIndex((c) => c.from === block.from && c.name === block.name);
  const mine = headingLevel(block);

  if (mine) {
    // Sections of this level under the same parent heading. A section runs to the block before the next heading of its level or a higher one.
    const hk = heads.findIndex((h) => h.i === k);
    let lo = hk, hi = hk;
    while (lo > 0 && heads[lo - 1].level >= mine) lo--;
    while (hi < heads.length - 1 && heads[hi + 1].level >= mine) hi++;
    const same = heads.slice(lo, hi + 1).filter((h) => h.level === mine);
    const sections = same.map((h) => {
      const next = heads.find((o) => o.i > h.i && o.level <= mine);
      return { from: state.doc.lineAt(kids[h.i].from).from, to: linesOf(state, kids[next ? next.i - 1 : kids.length - 1]).to };
    });
    return { key: `H${mine}@${lo}`, siblings: sections, at: same.findIndex((h) => h.i === k) };
  }

  // A plain block: its siblings are the blocks of the same section, so it never crosses a heading.
  let lo = k, hi = k;
  while (lo > 0 && !headingLevel(kids[lo - 1])) lo--;
  while (hi < kids.length - 1 && !headingLevel(kids[hi + 1])) hi++;
  return { key: `S${kids[lo].from}`, siblings: kids.slice(lo, hi + 1).map((c) => linesOf(state, c)), at: k - lo };
}

/** The whole-line span of the innermost block containing `pos`, or null on a blank line. */
export function blockUnitAt(state: EditorState, pos: number): Span | null {
  const level = levelOf(blocksOf(state), state.doc.lineAt(pos).from);
  return level && level.siblings[level.at];
}

/**
 * Move the unit(s) under the selection one sibling up or down: with a selection,
 * the units its lines touch move together. Returns true even when nothing moves
 * (a boundary, a selection spanning two levels) so the key never falls through to
 * moving a bare line.
 */
export function moveBlock(view: EditorView, dir: -1 | 1): boolean {
  const { state } = view;
  if (state.readOnly) return false;
  const blocks = blocksOf(state);
  const range = state.selection.main;
  const { doc } = state;
  // A selection that ends at a line start has not touched that line.
  const end = !range.empty && doc.lineAt(range.to).from === range.to ? range.to - 1 : range.to;

  const touched: { level: Level; unit: Span }[] = [];
  for (let n = doc.lineAt(range.from).number; n <= doc.lineAt(end).number; n++) {
    const level = levelOf(blocks, doc.line(n).from);
    if (level) touched.push({ level, unit: level.siblings[level.at] });
  }
  // A child selected together with its parent moves with it: only the outermost units count.
  const top = touched.filter((a) => !touched.some((b) => b.unit.from <= a.unit.from && a.unit.to <= b.unit.to && b.unit.to - b.unit.from > a.unit.to - a.unit.from));
  if (!top.length || top.some((u) => u.level.key !== top[0].level.key)) return true;

  const run = top[0].level.siblings;
  const lo = top[0].level.at, hi = top[top.length - 1].level.at;
  const moving = { from: run[lo].from, to: run[hi].to };
  const other = run[dir < 0 ? lo - 1 : hi + 1];
  if (!other) return true;

  const [a, b] = dir < 0 ? [other, moving] : [moving, other]; // a is the earlier span
  const gap = doc.sliceString(a.to, b.from);
  const text = doc.sliceString(b.from, b.to) + gap + doc.sliceString(a.from, a.to);
  const shift = (dir < 0 ? -1 : 1) * (other.to - other.from + gap.length);
  view.dispatch({
    changes: { from: a.from, to: b.to, insert: text },
    selection: EditorSelection.single(range.anchor + shift, range.head + shift),
    userEvent: 'move.line',
    annotations: isolateHistory.of('full'),
    scrollIntoView: true,
  });
  return true;
}

/** Above `defaultKeymap`'s Alt-↑/↓ (moveLineUp/Down), which `minimalSetup` registers first. */
function blockMoveKeymap(): Extension {
  return Prec.high(keymap.of([
    { key: 'Alt-ArrowUp', run: (view) => moveBlock(view, -1) },
    { key: 'Alt-ArrowDown', run: (view) => moveBlock(view, 1) },
  ]));
}

// ── Line type ──────────────────────────────────────────────────────────────
// Editing a line's type through its margin indicator. The indicator — a heading's
// `##`, a quote's `>` — is real source
// drawn in the margin by cm-markdown-style.ts, and the caret never enters it, so
// it is edited from the text start and by pointer:
//
//   Backspace (Ctrl-H on a Mac) at the start of the text
//                                        removes ONE level (`###` -> `##` -> `#`
//                                        -> paragraph; `> ` -> paragraph), one
//                                        undo step each
//   ⌘Backspace at the start of the text  the one exception: every indicator goes
//                                        at once and the line is a paragraph
//   click on the indicator               opens a menu: Heading 1-3, Paragraph,
//                                        Quote; a pick rewrites the clicked
//                                        indicator in one transaction (a heading
//                                        inside a quote, `> ## x`, has two)
//
// Typing `## ` or `> ` at a line start needs nothing here: it is an ordinary
// edit that the style module then hangs. Keyboard parity for the menu is
// the Mod-Alt-1..6 and Mod-Shift-9 bindings in formatKeysExtension above.

type LineType = { kind: 'heading'; level: number } | { kind: 'paragraph' } | { kind: 'quote' };

const PARAGRAPH: LineType = { kind: 'paragraph' };

const prefixFor = (type: LineType): string =>
  type.kind === 'heading' ? '#'.repeat(type.level) + ' ' : type.kind === 'quote' ? '> ' : '';

/** True when `h` already is `type` (a nested quote counts as a quote only at depth one, so choosing Quote flattens it). */
const isType = (h: Hang, type: LineType): boolean =>
  type.kind !== 'paragraph' && h.kind === type.kind && h.level === (type.kind === 'heading' ? type.level : 1);

/**
 * The one owner of rewriting a line's heading/quote prefix, in one transaction,
 * with the caret keeping its place in the text. The keyboard toggles, the insert
 * bar and the indicator menu all come here.
 *
 * A line can carry two indicators: a quote's `> ` and, after it, a heading's
 * `## ` (`> ## x`). `target` names the one being rewritten:
 *   - the menu passes the indicator that was clicked: it becomes `type`, and the
 *     line's other indicator of the same kind (a heading, when a quote was
 *     clicked and Heading is picked) is dropped so the result never says the same
 *     thing twice; Paragraph removes only the clicked indicator;
 *   - the toggles pass nothing for Heading/Quote: the line's own heading (or
 *     quote) is replaced, and one that is missing is added — a heading after any
 *     quote, a quote before any heading — so the other indicator stays; Paragraph
 *     without a target removes every indicator.
 * Returns false when nothing would change.
 */
export function setLineType(view: EditorView, lineFrom: number, type: LineType, target?: Hang): boolean {
  const { state } = view;
  const hangs = hangsOnLine(state, lineFrom);
  const slot = target ?? (type.kind === 'paragraph' ? undefined : hangs.find((h) => h.kind === type.kind));
  if (slot && isType(slot, type)) return false;
  const insert = prefixFor(type);
  const specs: { from: number; to: number; insert: string }[] = [];
  if (type.kind === 'paragraph') {
    for (const h of target ? [target] : hangs) specs.push({ from: h.from, to: h.to, insert: '' });
  } else if (slot) {
    specs.push({ from: slot.from, to: slot.to, insert });
    for (const h of hangs) if (h !== slot && h.kind === type.kind) specs.push({ from: h.from, to: h.to, insert: '' });
  } else {
    const at = type.kind === 'heading' ? (hangs.length ? hangs[hangs.length - 1].to : lineFrom) : (hangs.length ? hangs[0].from : lineFrom);
    specs.push({ from: at, to: at, insert });
  }
  if (!specs.length) return false;
  const changes = state.changes(specs.sort((a, b) => a.from - b.from));
  view.dispatch({ changes, selection: state.selection.map(changes, 1), userEvent: 'input.format', scrollIntoView: true });
  return true;
}

/** Backspace at the first character of a heading's or quote's text: one level off, never the whole marker at once. */
function stepBackspace(view: EditorView): boolean {
  if (view.composing) return false;
  const { state } = view;
  const r = state.selection.main; // one range: the editor never allows several (allowMultipleSelections is not set)
  const h = r.empty ? indicatorEndingAt(state, r.head) : null;
  if (!h) return false; // any other range: CM6's own Backspace
  const change: ChangeSpec =
    h.kind === 'heading' && h.level > 1 ? { from: h.from, to: h.from + 1 }
    : h.kind === 'quote' ? { from: h.from + state.doc.sliceString(h.from, h.to).lastIndexOf('>'), to: h.to }
    : { from: h.from, to: h.to };
  // Each level is its own undo step: quick presses would otherwise merge into one group and a single ⌘Z would restore all of them.
  view.dispatch({ changes: change, userEvent: 'delete.backward', annotations: isolateHistory.of('full'), scrollIntoView: true });
  return true;
}

/** ⌘Backspace (delete to line start) at the first character of a hung line's text: the one exception to one-level-at-a-time — every indicator goes and the line becomes a paragraph. Anywhere else it is CM6's own. */
function removeIndicators(view: EditorView): boolean {
  if (view.composing) return false;
  const r = view.state.selection.main;
  const hangs = hangsOnLine(view.state, r.head);
  if (!r.empty || !hangs.length || hangs[hangs.length - 1].to !== r.head) return false;
  view.dispatch({ changes: { from: hangs[0].from, to: r.head }, userEvent: 'delete.line', annotations: isolateHistory.of('full'), scrollIntoView: true });
  return true;
}

function openIndicatorMenu(view: EditorView, anchor: HTMLElement, lineFrom: number, clicked: Hang['kind']): void {
  const entry = (id: string, label: string, type: LineType): CtxMenuEntry => ({
    id,
    label,
    action: () => {
      // The clicked indicator is looked up again: the menu outlives the state it was opened on.
      const target = hangsOnLine(view.state, lineFrom).find((h) => h.kind === clicked);
      if (target) setLineType(view, lineFrom, type, target);
      view.focus();
    },
  });
  openCtxMenu({
    entries: [
      entry('heading-1', t('indicator.heading', { level: '1' }), { kind: 'heading', level: 1 }),
      entry('heading-2', t('indicator.heading', { level: '2' }), { kind: 'heading', level: 2 }),
      entry('heading-3', t('indicator.heading', { level: '3' }), { kind: 'heading', level: 3 }),
      entry('paragraph', t('indicator.paragraph'), PARAGRAPH),
      entry('quote', t('indicator.quote'), { kind: 'quote' }),
    ],
    position: { kind: 'below', anchor },
    outsideAnchor: anchor,
    ariaLabel: t('indicator.menu'),
    logSurface: 'line-indicator',
  });
}

export function lineIndicatorExtension(): Extension {
  return [
    Prec.high(keymap.of([
      { key: 'Backspace', run: stepBackspace, shift: stepBackspace },
      { mac: 'Ctrl-h', run: stepBackspace }, // the macOS backspace
      { key: 'Mod-Backspace', run: removeIndicators },
    ])),
    EditorView.domEventHandlers({
      // The indicator sits outside the text, so CM6 would map a press on it to a
      // caret in the margin. Take the press instead: caret to the text start, menu
      // open on the indicator that was pressed. A read-only editor leaves it to CM6.
      mousedown(event, view) {
        const el = (event.target as Element | null)?.closest?.<HTMLElement>('.cm-hang[data-indicator]');
        if (!el || event.button !== 0 || view.state.readOnly) return false;
        const pos = view.posAtDOM(el);
        const hangs = hangsOnLine(view.state, pos);
        const h = hangs.find((x) => pos >= x.from && pos < x.to);
        if (!h) return false;
        event.preventDefault();
        view.dispatch({ selection: { anchor: hangs[hangs.length - 1].to } });
        view.focus();
        openIndicatorMenu(view, el, view.state.doc.lineAt(pos).from, h.kind);
        return true;
      },
    }),
  ];
}
