// Where a selection's head may rest on a hung line, and the keys that move it
// there. A caret is never
// in the margin; a selection's head may rest at a hung line's start, where it
// covers the break above the line and not the marker.

import { EditorSelection, EditorState, Prec, type Extension, type SelectionRange } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { hangsIn, indicatorEndingAt, textStartOf } from './cm-hangs';

/**
 * A caret is never in the margin of a hung line, `[line.from, textStart)`. A
 * selection's head may rest at the line start: it then covers the break above
 * the line and not the marker, and one more step takes the marker. Any other
 * head in the margin goes where the caret can be drawn. `fitHead` returns the
 * range so moved, or null when it already is fit.
 *
 *   an empty caret                    -> the text start (a click in the margin, Home,
 *                                        ↑/↓ landing on the line, → from the line above)
 *   a selection from above            -> unchanged at the line start (Shift-→ over the break,
 *                                        a triple click, a drag ending in the margin);
 *                                        any other margin head, the text start
 *   a selection from this line's text,
 *   or from a later line              -> the end of the previous line (Shift-←,
 *                                        a drag up); on line 1, the document start
 *
 * The anchor is never moved, so a triple click or a whole-line selection still
 * contains the indicator's characters and copy gives the full source. A head at
 * the text start is given assoc 1, so it sits on the text side and not at the
 * indicator's right edge in the margin.
 *
 * Only the main range: the editor never allows multiple selections
 * (`allowMultipleSelections` is not set), so there is no other range to fit.
 */
function fitHead(state: EditorState, r: SelectionRange): SelectionRange | null {
  const line = state.doc.lineAt(r.head);
  const textStart = textStartOf(hangsIn(state, line.from, line.to));
  if (textStart < 0 || r.head > textStart || (r.head === textStart && r.assoc === 1)) return null;
  const backwards = !r.empty && r.head < textStart && r.anchor >= line.from;
  if (!r.empty && !backwards && r.head === line.from) return null;
  // On line 1 nothing precedes it: the document start, the one margin head allowed, or the line could never be selected whole.
  const head = backwards ? (line.number > 1 ? line.from - 1 : line.from) : textStart;
  if (r.empty) return EditorSelection.cursor(head, 1, r.bidiLevel ?? undefined, r.goalColumn);
  return EditorSelection.range(r.anchor, head, r.goalColumn, r.bidiLevel ?? undefined, head === textStart ? 1 : undefined);
}

/**
 * A filter, not a plugin: it rewrites the transaction's own selection in
 * place, so there is no second transaction, no microtask window where the
 * wrong side is drawn, and nothing to race a key event. It asks the tree about
 * the head's line alone, so a keystroke costs one line, not the document.
 * (← from the text start is the one motion that cannot be told from Home here,
 * so it is `stepLeftOffIndicator`'s.)
 * Composition: a transaction CM tags `input.type.compose` is passed through
 * untouched, so the filter never rewrites the selection mid-composition (the
 * next ordinary transaction pins it).
 */
const pinCaretToTextSide = EditorState.transactionFilter.of((tr) => {
  if ((!tr.selection && !tr.docChanged) || tr.isUserEvent('input.type.compose')) return tr;
  const sel = tr.newSelection;
  const fitted = fitHead(tr.docChanged ? tr.state : tr.startState, sel.main);
  return fitted ? [tr, { selection: EditorSelection.create([fitted], 0), sequential: true }] : tr;
});

/**
 * ← (and ⌥←) from the first character of an indicator's text goes to the end
 * of the previous line, as it does from any line start. CM6 alone would step
 * over the indicator to the line start, which is not a caret position, and
 * `pinCaretToTextSide` would put the caret straight back. The first line has
 * nothing before it: the caret stays. Every other position moves as the default
 * command moves it.
 */
function stepLeftOffIndicator(view: EditorView): boolean {
  if (view.composing) return false;
  const r = view.state.selection.main;
  if (!r.empty || !indicatorEndingAt(view.state, r.head)) return false;
  const line = view.state.doc.lineAt(r.head);
  const selection = line.number > 1 ? EditorSelection.cursor(line.from - 1) : r;
  view.dispatch({ selection, scrollIntoView: true, userEvent: 'select' });
  return true;
}

/**
 * Shift-Home (⇧⌘←, ⇧⌃A): select to the start of the line's text. CM6's own
 * command lands on the line start, inside the indicator; the filter would then
 * read a head coming from the right as "← from the text start" and carry the
 * selection to the previous line. Home itself needs nothing here (an empty
 * caret goes to the text start).
 */
function extendToTextStart(view: EditorView): boolean {
  if (view.composing) return false;
  const r = view.state.selection.main;
  const moved = view.moveToLineBoundary(r, false);
  const line = view.state.doc.lineAt(moved.head);
  const textStart = textStartOf(hangsIn(view.state, line.from, line.to));
  if (textStart < 0 || moved.head >= textStart) return false;
  const selection = EditorSelection.range(r.anchor, textStart, r.goalColumn, r.bidiLevel ?? undefined, 1);
  view.dispatch({ selection, scrollIntoView: true, userEvent: 'select' });
  return true;
}

/**
 * CM6 resolves a vertical move by probing pixel coordinates, and a line outside
 * the rendered viewport has only an ESTIMATED position. A tall block widget
 * (a picture below its source line) breaks the usual guarantee that the caret's
 * neighbour is rendered: the viewport is a pixel margin around the visible
 * area, so it can end at the picture's own line while the next text line lies a
 * whole picture further down, and the move then lands several lines away. So
 * before a vertical move, a neighbouring line that is not rendered is brought
 * into the viewport (a scroll target extends it). The move's own scroll puts
 * the caret back in view. Only the main range is rendered for: a scroll target
 * is one position (a second dispatch would replace the first's viewport), and
 * the main range is the one the user is looking at.
 */
function renderNeighbour(view: EditorView, head: number, forward: boolean): void {
  const { doc } = view.state;
  const n = doc.lineAt(head).number + (forward ? 1 : -1);
  if (n < 1 || n > doc.lines) return;
  const line = doc.line(n);
  if (line.from >= view.viewport.from && line.to <= view.viewport.to) return;
  view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: 'nearest' }) });
}

/**
 * ArrowUp/ArrowDown. CM6 resolves a vertical move by pixel, and a hung
 * indicator is drawn in the margin, so the goal column can fall on it; CM6 then
 * skips the atomic range to whichever side it likes and drops the goal column,
 * so the column drifts by the indicator's width and ArrowUp does not retrace
 * ArrowDown. Here the goal column is kept explicitly, and `pinCaretToTextSide`
 * lands the caret on the text side both ways.
 */
function moveLine(forward: boolean) {
  return (view: EditorView): boolean => {
    if (view.composing) return false; // never hijack an IME candidate nav
    const r = view.state.selection.main;
    if (!r.empty) return false; // CM6 collapses a selection to its end
    renderNeighbour(view, r.head, forward);
    const moved = view.moveVertically(r, forward);
    let next: SelectionRange;
    if (moved.head === r.head) next = view.moveToLineBoundary(r, forward); // first/last line: as cursorLineUp/Down
    else {
      const coords = view.coordsAtPos(r.head, r.assoc || (forward ? 1 : -1));
      const goal = r.goalColumn ?? (coords ? coords.left - view.contentDOM.getBoundingClientRect().left : undefined);
      next = EditorSelection.cursor(moved.head, moved.assoc, moved.bidiLevel ?? undefined, goal);
    }
    view.dispatch({ selection: EditorSelection.create([next], 0), scrollIntoView: true, userEvent: 'select' });
    return true;
  };
}

/**
 * A vertical extension forward (Shift-↓, Shift-PageDown, Shift-⌘↓) from a caret
 * at a hung line's text start anchors at the line start, so Delete takes the
 * indicator too; Shift-→ stays text-only. Returns false: the extension is CM6's.
 */
function anchorAtIndicator(view: EditorView): boolean {
  const r = view.state.selection.main;
  if (view.composing || !r.empty || !indicatorEndingAt(view.state, r.head)) return false;
  const from = view.state.doc.lineAt(r.head).from;
  view.dispatch({ selection: EditorSelection.range(from, r.head, r.goalColumn, r.bidiLevel ?? undefined, 1), userEvent: 'select' });
  return false;
}

/**
 * Shift-ArrowUp/Down keep CM6's own extension, plus: the neighbouring line is
 * rendered first (as moveLine), and a head landing in a hung indicator is set
 * here rather than left to the filter. Shift-↓ from a line start stops at the
 * next line start (the break, not the marker); otherwise it goes to the text
 * start. Shift-↑ onto line 1 stops at the document start (see fitHead).
 */
function extendLine(forward: boolean) {
  return (view: EditorView): boolean => {
    if (view.composing) return false;
    if (forward) anchorAtIndicator(view);
    const r = view.state.selection.main;
    renderNeighbour(view, r.head, forward);
    const moved = view.moveVertically(r, forward);
    const line = view.state.doc.lineAt(moved.head);
    const textStart = textStartOf(hangsIn(view.state, line.from, line.to));
    if (moved.head === r.head || textStart < 0) return false;
    const toLineStart = forward ? r.head === view.state.doc.lineAt(r.head).from : line.number === 1 && moved.head <= textStart;
    if (!toLineStart && moved.head >= textStart) return false;
    const head = toLineStart ? line.from : textStart;
    const assoc = head === textStart ? 1 : undefined;
    view.dispatch({ selection: EditorSelection.range(r.anchor, head, moved.goalColumn, moved.bidiLevel ?? undefined, assoc), scrollIntoView: true, userEvent: 'select' });
    return true;
  };
}


/** The head rules: the transaction filter that fits every selection, and the keys whose CM6 defaults would land in the margin. */
export function headFitExtension(): Extension {
  return [
    pinCaretToTextSide,
    Prec.high(keymap.of([
      { key: 'ArrowLeft', run: stepLeftOffIndicator },
      { mac: 'Ctrl-b', run: stepLeftOffIndicator },
      { key: 'Mod-ArrowLeft', mac: 'Alt-ArrowLeft', run: stepLeftOffIndicator },
      { key: 'Shift-Home', run: extendToTextStart },
      { mac: 'Shift-Cmd-ArrowLeft', run: extendToTextStart },
      { mac: 'Shift-Ctrl-a', run: extendToTextStart },
      { key: 'ArrowUp', run: moveLine(false), shift: extendLine(false) },
      { key: 'ArrowDown', run: moveLine(true), shift: extendLine(true) },
      { key: 'Shift-PageDown', run: anchorAtIndicator },
      { key: 'Shift-Mod-End', run: anchorAtIndicator },
      { mac: 'Shift-Cmd-ArrowDown', run: anchorAtIndicator },
    ])),
  ];
}
