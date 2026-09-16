import { describe, test, expect } from 'vitest';
import { EditorState, EditorSelection } from '@codemirror/state';
import { nodeTouchesSelection } from '../cm-active-lines.js';

function stateWith(doc: string, anchor: number, head = anchor) {
  return EditorState.create({ doc, selection: { anchor, head } });
}

describe('nodeTouchesSelection', () => {
  const doc = 'a **b** c'; // bold node "**b**" spans [2,7]

  test('collapsed cursor inside the node reveals (true)', () => {
    expect(nodeTouchesSelection(stateWith(doc, 4), 2, 7)).toBe(true);
  });
  test('collapsed cursor at the start edge reveals (non-strict, true)', () => {
    expect(nodeTouchesSelection(stateWith(doc, 2), 2, 7)).toBe(true);
  });
  test('collapsed cursor at the end edge reveals (non-strict, true)', () => {
    expect(nodeTouchesSelection(stateWith(doc, 7), 2, 7)).toBe(true);
  });
  test('collapsed cursor outside the node does not reveal (false)', () => {
    expect(nodeTouchesSelection(stateWith(doc, 0), 2, 7)).toBe(false);
    expect(nodeTouchesSelection(stateWith(doc, 8), 2, 7)).toBe(false);
  });
  test('selection partially overlapping the node reveals (true)', () => {
    expect(nodeTouchesSelection(stateWith(doc, 0, 4), 2, 7)).toBe(true);
  });
  test('multi-cursor: a secondary range touching the node reveals (true)', () => {
    // CM6 keeps only the main range unless allowMultipleSelections is enabled.
    const state = EditorState.create({
      doc,
      selection: EditorSelection.create(
        [EditorSelection.cursor(0), EditorSelection.cursor(4)],
        0,
      ),
      extensions: [EditorState.allowMultipleSelections.of(true)],
    });
    expect(state.selection.ranges.length).toBe(2); // guard: both cursors kept
    expect(nodeTouchesSelection(state, 2, 7)).toBe(true);
  });
});
