import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { shortcodeBlockConfig } from '../../index.js';
import {
  sourceModeField,
  setSourceModeEffect,
  isSourceMode,
  sourceModeChanged,
} from '../cm-source-mode.js';
import { getActiveLines, isNodeActive, nodeTouchesSelection } from '../cm-active-lines.js';
import { collectShortcodeBlocks, isBlockActive } from '../cm-shortcode-block.js';

function mdState(doc: string, anchor = 0): EditorState {
  return EditorState.create({
    doc,
    selection: { anchor },
    extensions: [sourceModeField, markdown({ extensions: [shortcodeBlockConfig] })],
  });
}

/** Apply the mode toggle the way the UI does: an effect-only transaction. */
function withSourceMode(state: EditorState, on = true): EditorState {
  return state.update({ effects: setSourceModeEffect.of(on) }).state;
}

describe('sourceModeField', () => {
  test('a fresh state rests in live view (create() is false)', () => {
    expect(isSourceMode(mdState('hello'))).toBe(false);
  });

  test('the toggle effect flips the field without touching the doc', () => {
    const on = withSourceMode(mdState('hello'));
    expect(isSourceMode(on)).toBe(true);
    expect(on.doc.toString()).toBe('hello');
    expect(isSourceMode(withSourceMode(on, false))).toBe(false);
  });

  test('a state without the field reads as live view (non-markdown editors)', () => {
    expect(isSourceMode(EditorState.create({ doc: 'x' }))).toBe(false);
  });

  test('sourceModeChanged is true exactly for the flipping transaction', () => {
    const tr = mdState('hello').update({ effects: setSourceModeEffect.of(true) });
    expect(sourceModeChanged(tr)).toBe(true);
    const noop = tr.state.update({ selection: { anchor: 1 } });
    expect(sourceModeChanged(noop)).toBe(false);
  });
});

describe('source mode forces the shared reveal predicates', () => {
  const doc = 'a **b** c\n\nsecond paragraph';

  test('getActiveLines returns every line of the document', () => {
    const state = withSourceMode(mdState(doc, 0));
    expect(getActiveLines(state)).toEqual(new Set([1, 2, 3]));
    // and isNodeActive therefore holds anywhere
    expect(isNodeActive(state, doc.length - 3, doc.length, getActiveLines(state))).toBe(true);
  });

  test('nodeTouchesSelection is true for a node the caret is nowhere near', () => {
    const state = withSourceMode(mdState(doc, doc.length)); // caret at the end
    expect(nodeTouchesSelection(state, 2, 7)).toBe(true); // bold "**b**"
    // sanity: same caret in live view does NOT touch it
    expect(nodeTouchesSelection(mdState(doc, doc.length), 2, 7)).toBe(false);
  });

  test('isBlockActive reveals a shortcode block the caret is outside of', () => {
    const scDoc = ':::grid {cols=2}\nbody\n:::\n\nafter';
    const live = mdState(scDoc, scDoc.length); // caret on the trailing line
    const [block] = collectShortcodeBlocks(live);
    expect(block).toBeDefined();
    expect(isBlockActive(live, block)).toBe(false);
    expect(isBlockActive(withSourceMode(live), block)).toBe(true);
  });
});
