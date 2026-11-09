import { describe, test, expect, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { CmEditor } from '../cm-editor';
import { HERO_FRAME_REQUEST, HERO_WORDS } from '../session/instrument-events';

import { host } from "../../host";

const emit = vi.fn();
host.bus = { emit, on: () => () => {} };

// Wiring checks on the whole editor: the full extension stack, not one module.
describe('cm-editor wiring', () => {
  test('a heading renders cm-lp-h1 and no writing-mode control exists', () => {
    const container = document.createElement('div');
    const editor = new CmEditor({ content: '# Title', container, onChange: () => {} });
    expect(container.querySelector('.cm-lp-h1')).not.toBeNull();
    expect(container.querySelector('.mode-control')).toBeNull(); // no such class after deletion
    editor.destroy();
  });

  // Every character is on screen:
  // no decoration of the full editor replaces text. An ATOMIC replace
  // that only feeds `EditorView.atomicRanges` is not rendered, so the assertion
  // reads what the view DRAWS (the decorations facet), not every decoration the
  // editor has.
  test('no rendered replace decoration hides text, whatever the document holds', () => {
    const doc = [
      '# Heading', '', '> quote', '', '- [ ] task', '- [x] done', '',
      'a [link](https://example.com) and [[Wiki|alias]] and a footnote[^1] and `code` and **bold** and ~~gone~~ and $x$.',
      '', '![pic](images/a.png) inline ![[b.png]] in prose', '', '![[images/logo.png]]', '',
      '{==comment==}{>>note<<} and {++add++} {--del--} {~~old~>new~~}', '',
      '> [!note] callout', '> body', '', '| a | b |', '| - | - |', '| 1 | 2 |', '',
      '```js', 'code', '```', '', '$$', 'x', '$$', '', ':::grid 2', 'left', '+++', 'right', ':::', '', '---', '', '[^1]: the note', '',
    ].join('\n');
    const container = document.createElement('div');
    document.body.append(container);
    const editor = new CmEditor({ content: doc, container, onChange: () => {}, getFromFile: () => 'a.md', getProjectRoot: () => '/x' });
    const view: EditorView = editor.getView();
    const hiding: string[] = [];
    for (const source of view.state.facet(EditorView.decorations)) {
      const set: DecorationSet = typeof source === 'function' ? source(view) : source;
      set.between(0, view.state.doc.length, (from, to, deco) => {
        // A replace is a "point" decoration spanning text; a widget at a position has from === to.
        if ((deco as unknown as { point?: boolean }).point === true && from < to) hiding.push(`${from}-${to} ${JSON.stringify(view.state.sliceDoc(from, to).slice(0, 30))}`);
      });
    }
    editor.destroy();
    expect(hiding).toEqual([]);
  });

  test('an instrument edit reaches onChange as the end of a gesture; typing does not', () => {
    const container = document.createElement('div');
    const seen: boolean[] = [];
    const editor = new CmEditor({ content: 'text', container, onChange: (gestureEnd) => seen.push(gestureEnd) });
    const view = editor.getView();
    view.dispatch({ changes: { from: 0, insert: 'a' }, userEvent: 'input.type' });
    view.dispatch({ changes: { from: 0, insert: 'b' }, userEvent: 'input.instrument' });
    editor.destroy();
    expect(seen).toEqual([false, true]);
  });

  test('a caret on a hero picture opens the focus-point instrument', () => {
    const container = document.createElement('div');
    document.body.append(container);
    const editor = new CmEditor({ content: ':::hero\n![[a.jpg]]\n:::\n', container, onChange: () => {}, getFromFile: () => 'a.md', getProjectRoot: () => '/x' });
    const view = editor.getView();
    view.dispatch({ selection: { anchor: view.state.doc.line(2).from } });
    editor.destroy();
    expect(emit).toHaveBeenCalledWith(HERO_FRAME_REQUEST, { path: 'a.md', line: 1 });
  });

  test('a caret on a hero opening line tells the shell to draw the words box', () => {
    const container = document.createElement('div');
    document.body.append(container);
    const editor = new CmEditor({ content: 'intro\n:::hero {image=a.jpg}\nTitle\n:::\n', container, onChange: () => {}, getFromFile: () => 'a.md', getProjectRoot: () => '/x' });
    const view = editor.getView();
    view.dispatch({ selection: { anchor: view.state.doc.line(2).from } });
    editor.destroy();
    expect(emit).toHaveBeenCalledWith(HERO_WORDS, { path: 'a.md', line: 2 });
  });
});
