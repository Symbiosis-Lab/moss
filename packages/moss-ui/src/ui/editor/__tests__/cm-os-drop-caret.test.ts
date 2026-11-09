import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { osDropCaret, osDropCaretField, setOsDropCaret } from '../cm-os-drop-caret';

describe('cm-os-drop-caret StateField', () => {
  test('effect sets and clears the caret position', () => {
    let s = EditorState.create({ doc: 'hello', extensions: [osDropCaret()] });

    // Set the caret to position 3
    s = s.update({ effects: setOsDropCaret.of(3) }).state;
    expect(s.field(osDropCaretField)).toBe(3);

    // Clear the caret
    s = s.update({ effects: setOsDropCaret.of(null) }).state;
    expect(s.field(osDropCaretField)).toBe(null);
  });

  test('initial state has no caret position', () => {
    const s = EditorState.create({ doc: 'hello', extensions: [osDropCaret()] });
    expect(s.field(osDropCaretField)).toBe(null);
  });

  test('multiple effects — last one wins within a single update', () => {
    let s = EditorState.create({ doc: 'hello world', extensions: [osDropCaret()] });
    s = s.update({ effects: [setOsDropCaret.of(2), setOsDropCaret.of(7)] }).state;
    // The loop in update() processes effects in order; the final value is 7
    expect(s.field(osDropCaretField)).toBe(7);
  });

  test('position 0 is valid (not confused with falsy)', () => {
    let s = EditorState.create({ doc: 'hello', extensions: [osDropCaret()] });
    s = s.update({ effects: setOsDropCaret.of(0) }).state;
    expect(s.field(osDropCaretField)).toBe(0);
  });
});
