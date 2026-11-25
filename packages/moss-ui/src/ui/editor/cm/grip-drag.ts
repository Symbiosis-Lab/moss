// Dragging a block by its margin grip. The grips are a hung opening fence (its
// colons) and a heading's hung `#`: dragging one lifts the whole block or section,
// an insertion line shows the nearest gap between the siblings of the same run,
// and letting go moves it there in one transaction. The editor's one definition
// of a block, its siblings and a move arrives through `GripOps`
// (cm-format-keys.ts), so Alt-↑/↓ and a drag move text the same way.
//
// A press that never travels `THRESHOLD` px is a click: `press` decides what that
// means. Everything follows from the document; the caret is not consulted.

import { Decoration, EditorView, ViewPlugin, type DecorationSet } from '@codemirror/view';
import { EditorState, StateEffect, StateField, type Extension } from '@codemirror/state';

interface Span { from: number; to: number }

export interface GripOps {
  /** A click on the grip (no drag). */
  press(view: EditorView, grip: HTMLElement): void;
  /** The siblings of the unit on the line at `lineFrom`, and the unit's index. */
  siblingsAt(state: EditorView['state'], lineFrom: number): { siblings: Span[]; at: number } | null;
  move(view: EditorView, siblings: readonly Span[], at: number, gap: number): void;
}

const THRESHOLD = 4;

const lift = StateEffect.define<Span | null>();

const LIFTED = Decoration.line({ class: 'cm-grip-lifted' });

/** The unit being dragged, its lines dimmed in place. */
const liftedField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const e of tr.effects) {
      if (!e.is(lift)) continue;
      if (!e.value) return Decoration.none;
      const lines = [];
      for (let n = tr.state.doc.lineAt(e.value.from).number; n <= tr.state.doc.lineAt(e.value.to).number; n++) lines.push(LIFTED.range(tr.state.doc.line(n).from));
      return Decoration.set(lines);
    }
    return value.map(tr.changes);
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** Screen y of every gap in `siblings`, from where the neighbouring lines are painted: gap `k` is before sibling `k`, the last after the final one. NaN where a line is not laid out. */
function gapYs(view: EditorView, siblings: readonly Span[]): number[] {
  const top = (s: Span) => view.coordsAtPos(s.from, 1)?.top ?? NaN;
  const bottom = (s: Span) => view.coordsAtPos(s.to, -1)?.bottom ?? NaN;
  const ys = siblings.map((s, k) => (k === 0 ? top(s) : (bottom(siblings[k - 1]) + top(s)) / 2));
  return [...ys, bottom(siblings[siblings.length - 1])];
}

/** The gesture in progress on a view, so destroying the view can end it. */
const active = new WeakMap<EditorView, () => void>();

function gesture(view: EditorView, event: PointerEvent, grip: HTMLElement, siblings: Span[], at: number, ops: GripOps): void {
  const doc0 = view.state.doc;
  const unit = siblings[at];
  const line = document.createElement('div');
  line.className = 'cm-grip-insertion';
  let dragging = false;
  let gap = -1;

  const showLine = (clientY: number) => {
    const ys = gapYs(view, siblings);
    let best = -1;
    ys.forEach((g, k) => { if (Number.isFinite(g) && (best < 0 || Math.abs(g - clientY) < Math.abs(ys[best] - clientY))) best = k; });
    gap = best === at || best === at + 1 ? -1 : best; // its own gaps: dropping there changes nothing
    if (gap < 0) { line.remove(); return; }
    const box = view.dom.getBoundingClientRect(), content = view.contentDOM.getBoundingClientRect();
    const inset = parseFloat(getComputedStyle(view.contentDOM.querySelector('.cm-line') ?? view.contentDOM).paddingLeft) || 0;
    line.style.top = `${ys[gap] - box.top}px`;
    line.style.left = `${content.left + inset - box.left}px`;
    line.style.width = `${content.width - 2 * inset}px`;
    view.dom.append(line);
  };

  const onMove = (e: PointerEvent) => {
    if (e.pointerId !== event.pointerId) return;
    if (view.state.doc !== doc0) return finish(false); // the siblings were read from the document as it was
    if (!dragging) {
      if (Math.hypot(e.clientX - event.clientX, e.clientY - event.clientY) < THRESHOLD) return;
      dragging = true;
      view.dom.classList.add('cm-grip-dragging');
      view.dispatch({ effects: lift.of(unit) });
    }
    showLine(e.clientY);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    finish(false);
  };
  const onUp = (e: PointerEvent) => { if (e.pointerId === event.pointerId) finish(e.type === 'pointerup'); };
  const onLost = () => finish(false);
  const finish = (commit: boolean, destroyed = false) => {
    active.delete(view);
    view.dom.removeEventListener('pointermove', onMove);
    view.dom.removeEventListener('pointerup', onUp);
    view.dom.removeEventListener('pointercancel', onUp);
    view.dom.removeEventListener('lostpointercapture', onLost);
    view.dom.ownerDocument.removeEventListener('keydown', onKey, true);
    try { view.dom.releasePointerCapture(event.pointerId); } catch { /* not captured */ }
    line.remove();
    view.dom.classList.remove('cm-grip-dragging');
    if (destroyed) return;
    if (dragging) view.dispatch({ effects: lift.of(null) });
    if (!commit || view.state.doc !== doc0) return;
    if (!dragging) { ops.press(view, grip); return; }
    if (gap >= 0) ops.move(view, siblings, at, gap);
    view.focus();
  };

  try { view.dom.setPointerCapture(event.pointerId); } catch { /* pointer already gone */ }
  view.dom.addEventListener('pointermove', onMove);
  view.dom.addEventListener('pointerup', onUp);
  view.dom.addEventListener('pointercancel', onUp);
  view.dom.addEventListener('lostpointercapture', onLost);
  active.set(view, () => finish(false, true));
  view.dom.ownerDocument.addEventListener('keydown', onKey, true);
}

export function gripDragExtension(ops: GripOps): Extension {
  const gripOf = (event: Event, view: EditorView): HTMLElement | null =>
    view.state.readOnly ? null : (event.target as Element | null)?.closest?.<HTMLElement>('.cm-hang[data-grip]') ?? null;
  return [
    liftedField,
    ViewPlugin.define((view) => ({ destroy: () => active.get(view)?.() })),
    // Only an editable editor shows the grab cursor.
    EditorView.editorAttributes.compute([EditorState.readOnly], (state) => ({ class: state.readOnly ? '' : 'cm-grippable' })),
    EditorView.domEventHandlers({
      pointerdown(event, view) {
        const grip = gripOf(event, view);
        if (!grip || event.button !== 0 || !event.isPrimary) return false;
        const lineFrom = view.state.doc.lineAt(view.posAtDOM(grip)).from;
        const run = ops.siblingsAt(view.state, lineFrom);
        if (!run || run.siblings[run.at].from !== lineFrom) return false;
        gesture(view, event, grip, run.siblings, run.at, ops);
        return true;
      },
      // The grip sits outside the text: CM6 would otherwise start a selection from the margin.
      mousedown(event, view) {
        if (!gripOf(event, view) || event.button !== 0) return false;
        event.preventDefault();
        return true;
      },
    }),
  ];
}
