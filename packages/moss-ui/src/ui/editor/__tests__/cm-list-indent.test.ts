import { describe, test, expect, afterEach } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { GFM } from '@lezer/markdown';
import { EditorView, runScopeHandlers } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { history, undo } from '@codemirror/commands';
import { ensureSyntaxTree } from '@codemirror/language';
import { formatKeysExtension } from '../cm-format-keys';

let view: EditorView;
afterEach(() => { view?.destroy(); document.body.innerHTML = ''; });

/** `|` marks the caret; `«` `»` mark a selection. */
function mount(src: string): void {
  const a = src.indexOf('«'), b = src.indexOf('»');
  const sel = a >= 0 ? { anchor: a, head: b - 1 } : { anchor: src.indexOf('|') };
  const doc = src.replace(/[«»|]/g, '');
  const parent = document.createElement('div');
  document.body.appendChild(parent);
  view = new EditorView({
    parent,
    state: EditorState.create({ doc, selection: sel, extensions: [markdown({ extensions: [GFM] }), history(), formatKeysExtension()] }),
  });
  ensureSyntaxTree(view.state, doc.length, 1000);
}
const press = (shift = false): boolean =>
  runScopeHandlers(view, new KeyboardEvent('keydown', { key: 'Tab', shiftKey: shift, bubbles: true, cancelable: true }), 'editor');
const doc = (): string => view.state.doc.toString();

describe('Tab on a list item', () => {
  test.each([
    ['- a\n- b|', '- a\n  - b'],
    ['* a\n* b|', '* a\n  * b'],
    ['- a\n- |b', '- a\n  - b'],
    ['- a\n|- b', '- a\n  - b'],
    ['1. a\n2. b|', '1. a\n   2. b'],
    ['- a\n- [ ] b|', '- a\n  - [ ] b'],
    ['- [x] a\n- [ ] b|', '- [x] a\n  - [ ] b'],
    ['10. a\n11. b|', '10. a\n    11. b'],
  ])('indents %j by the previous sibling\'s content column', (src, want) => {
    mount(src);
    expect(press()).toBe(true);
    expect(doc()).toBe(want);
  });

  test('the caret stays on the same character', () => {
    mount('- a\n- bc|d');
    press();
    expect(view.state.sliceDoc(view.state.selection.main.head - 2, view.state.selection.main.head)).toBe('bc');
  });

  test('a nested child moves with its parent', () => {
    mount('- a\n- b|\n  - c\n    - d\n- e');
    press();
    expect(doc()).toBe('- a\n  - b\n    - c\n      - d\n- e');
  });

  test('a continuation line of the item moves with it', () => {
    mount('- a\n- b|\n  more');
    press();
    expect(doc()).toBe('- a\n  - b\n    more');
  });

  test('a selection spanning several items indents each one, children once', () => {
    mount('- a\n- «b\n  - c\n- d»\n- e');
    press();
    expect(doc()).toBe('- a\n  - b\n    - c\n  - d\n- e');
  });

  test('the first item of a list, and a top-level item on Shift-Tab, have nowhere to go: nothing changes, the key is still consumed', () => {
    mount('- a|\n- b');
    expect(press()).toBe(true);
    expect(press(true)).toBe(true);
    expect(doc()).toBe('- a\n- b');
  });

  test('a list inside a quote indents after the `>`', () => {
    mount('> - a\n> - b|');
    press();
    expect(doc()).toBe('> - a\n>   - b');
  });

  test('outside a list Tab is left alone (focus moves as before)', () => {
    mount('para|\n\n# heading');
    expect(press()).toBe(false);
    expect(doc()).toBe('para\n\n# heading');
  });

  test('a read-only editor does not indent', () => {
    const parent = document.createElement('div');
    document.body.appendChild(parent);
    view = new EditorView({ parent, state: EditorState.create({ doc: '- a\n- b', selection: { anchor: 7 }, extensions: [markdown({ extensions: [GFM] }), formatKeysExtension(), EditorState.readOnly.of(true)] }) });
    expect(press()).toBe(false);
  });
});

describe('Shift-Tab on a list item', () => {
  test.each([
    ['- a\n  - b|', '- a\n- b'],
    ['1. a\n   2. b|', '1. a\n2. b'],
    ['- a\n  - [ ] b|', '- a\n- [ ] b'],
    ['- a\n    - b|', '- a\n- b'],
  ])('outdents %j to its parent\'s marker column', (src, want) => {
    mount(src);
    expect(press(true)).toBe(true);
    expect(doc()).toBe(want);
  });

  test('a nested child moves with its parent', () => {
    mount('- a\n  - b|\n    - c\n- d');
    press(true);
    expect(doc()).toBe('- a\n- b\n  - c\n- d');
  });

  test('outside a list Shift-Tab is left alone', () => {
    mount('para|');
    expect(press(true)).toBe(false);
  });

  test('indent then outdent is the identity', () => {
    mount('- a\n- b|\n  - c');
    press();
    press(true);
    expect(doc()).toBe('- a\n- b\n  - c');
  });
});

describe('what moves with an item', () => {
  test('tab characters count as columns (stops of 4): the item is indented after them', () => {
    mount('- a\n\t- b\n\t- c|');
    press();
    expect(doc()).toBe('- a\n\t- b\n\t  - c');
    press(true);
    expect(doc()).toBe('- a\n\t- b\n\t- c');
  });

  test('Shift-Tab takes a tab-indented child back to its parent', () => {
    mount('- a\n\t- b|');
    press(true);
    expect(doc()).toBe('- a\n- b');
  });

  test('a tab that overshoots the outdent is traded for the spaces it leaves, to the parent\'s column', () => {
    mount('  - a\n\t- b|');
    press(true);
    expect(doc()).toBe('  - a\n  - b');
  });

  test('a fenced code block inside an item moves with it', () => {
    mount('- a\n- b|\n  ```js\n  x()\n  ```\n- c');
    press();
    expect(doc()).toBe('- a\n  - b\n    ```js\n    x()\n    ```\n- c');
  });

  test('a blank line inside an item moves nothing and gains no trailing spaces', () => {
    mount('- a\n- b|\n\n  more\n- c');
    press();
    expect(doc()).toBe('- a\n  - b\n\n    more\n- c');
  });

  test('mixed markers are two lists: Tab on the first item of the second is a no-op', () => {
    mount('- a\n1. b|');
    expect(press()).toBe(true);
    expect(doc()).toBe('- a\n1. b');
  });

  test('a list right after a paragraph, without a blank line, indents like any other', () => {
    mount('para\n- a\n- b|');
    press();
    expect(doc()).toBe('para\n- a\n  - b');
  });

  test('Shift-Tab over a selection spanning items at different levels outdents each one level', () => {
    mount('- a\n  - «b\n    - c\n  - d\n- e»');
    press(true);
    expect(doc()).toBe('- a\n- b\n  - c\n- d\n- e');
  });
});

describe('undo', () => {
  test('one undo restores the document after indenting several items', () => {
    mount('- a\n- «b\n- c»');
    press();
    expect(doc()).not.toBe('- a\n- b\n- c');
    undo(view);
    expect(doc()).toBe('- a\n- b\n- c');
  });

  test('Shift-Tab straight after a deletion is its own undo step: history joins `delete.*` events, so one undo would bring the deleted space back too', () => {
    mount('- a\n   |- b');
    view.dispatch({ changes: { from: view.state.selection.main.head - 1, to: view.state.selection.main.head }, userEvent: 'delete.backward' });
    expect(doc()).toBe('- a\n  - b');
    press(true);
    expect(doc()).toBe('- a\n- b');
    undo(view);
    expect(doc()).toBe('- a\n  - b');
  });
});
