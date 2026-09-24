import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { shortcodeBlockConfig } from '../../index.js';
import { editorFocusField, setEditorFocusedEffect, isRevealSuspended } from '../cm-editor-focus.js';
import { sourceModeField, setSourceModeEffect, revealInputsChanged } from '../cm-source-mode.js';
import { getActiveLines, nodeTouchesSelection } from '../cm-active-lines.js';
import { collectShortcodeBlocks, isBlockActive } from '../cm-shortcode-block.js';

const doc = 'a **b** c\n\nsecond paragraph';

/** A markdown state with the caret on the bold node, and the focus field
 *  installed — as a host that opted in has it. */
function unfocused(text = doc, anchor = 3): EditorState {
  return EditorState.create({
    doc: text,
    selection: { anchor },
    extensions: [editorFocusField, sourceModeField, markdown({ extensions: [shortcodeBlockConfig] })],
  });
}

const focus = (state: EditorState, on = true): EditorState =>
  state.update({ effects: setEditorFocusedEffect.of(on) }).state;

describe('an unfocused editor reveals nothing', () => {
  test('no line is active, though the caret sits on line 1', () => {
    expect(getActiveLines(unfocused())).toEqual(new Set());
    expect(getActiveLines(focus(unfocused()))).toEqual(new Set([1]));
  });

  test('no node touches the selection, though the caret is inside it', () => {
    expect(nodeTouchesSelection(unfocused(), 2, 7)).toBe(false);
    expect(nodeTouchesSelection(focus(unfocused()), 2, 7)).toBe(true);
  });

  test('a shortcode block the caret is inside stays rendered', () => {
    const sc = ':::grid {cols=2}\nbody\n:::\n\nafter';
    const state = unfocused(sc, 18);
    const [block] = collectShortcodeBlocks(state);
    expect(isBlockActive(state, block)).toBe(false);
    expect(isBlockActive(focus(state), block)).toBe(true);
  });

  test('losing focus again suspends reveals again', () => {
    expect(nodeTouchesSelection(focus(focus(unfocused()), false), 2, 7)).toBe(false);
  });

  test('source mode shows raw source regardless of focus', () => {
    const source = unfocused().update({ effects: setSourceModeEffect.of(true) }).state;
    expect(getActiveLines(source)).toEqual(new Set([1, 2, 3]));
  });

  test('a state without the field reveals at the caret, as before', () => {
    const plain = EditorState.create({ doc, selection: { anchor: 3 } });
    expect(isRevealSuspended(plain)).toBe(false);
    expect(getActiveLines(plain)).toEqual(new Set([1]));
  });
});

describe('the reveal rebuild gate', () => {
  test('fires when focus comes or goes, and not for an effect that changes nothing', () => {
    expect(revealInputsChanged(unfocused().update({ effects: setEditorFocusedEffect.of(true) }))).toBe(true);
    expect(revealInputsChanged(focus(unfocused()).update({ effects: setEditorFocusedEffect.of(false) }))).toBe(true);
    expect(revealInputsChanged(unfocused().update({ effects: setEditorFocusedEffect.of(false) }))).toBe(false);
  });
});
